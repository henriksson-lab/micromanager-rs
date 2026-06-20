/// Zeiss MCU28 XY stage controller.
///
/// Protocol (TX `\r`, RX `\r`):
///   `NPXp\r`         → `PN{hex6}\r`   (query X position)
///   `NPYp\r`         → `PN{hex6}\r`   (query Y position)
///   `NPXT{hex6}\r`   → `PN\r`         (set X position)
///   `NPYT{hex6}\r`   → `PN\r`         (set Y position)
///
/// Step size: 0.2 µm / step for both axes.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, XYStage};
use crate::types::{DeviceType, PropertyValue};
use std::cell::Cell;

use super::hub::{decode_pos, encode_pos, ZeissHub};

const STEPS_PER_UM: f64 = 5.0; // 0.2 µm/step → 5 steps/µm

pub struct ZeissMcu28XYStage {
    props: PropertyMap,
    hub: ZeissHub,
    initialized: bool,
    x_um: Cell<f64>,
    y_um: Cell<f64>,
    firmware: String,
    step_size_um: Cell<f64>,
}

impl ZeissMcu28XYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property(
                "StepSize(um)",
                PropertyValue::Float(1.0 / STEPS_PER_UM),
                false,
            )
            .unwrap();
        props
            .define_property("XY firmware", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("X_um", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property("Y_um", PropertyValue::Float(0.0), false)
            .unwrap();
        Self {
            props,
            hub: ZeissHub::new(),
            initialized: false,
            x_um: Cell::new(0.0),
            y_um: Cell::new(0.0),
            firmware: String::new(),
            step_size_um: Cell::new(1.0 / STEPS_PER_UM),
        }
    }

    pub fn new_with_hub(hub: ZeissHub) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property(
                "StepSize(um)",
                PropertyValue::Float(1.0 / STEPS_PER_UM),
                false,
            )
            .unwrap();
        props
            .define_property("XY firmware", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("X_um", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property("Y_um", PropertyValue::Float(0.0), false)
            .unwrap();
        Self {
            props,
            hub,
            initialized: false,
            x_um: Cell::new(0.0),
            y_um: Cell::new(0.0),
            firmware: String::new(),
            step_size_um: Cell::new(1.0 / STEPS_PER_UM),
        }
    }

    fn send(&self, cmd: &str) -> MmResult<String> {
        self.hub.send(cmd)
    }

    fn get_axis(&self, axis: char) -> MmResult<i32> {
        let resp = self.send(&format!("NP{}p", axis))?;
        let hex = resp.strip_prefix("PN").unwrap_or(&resp);
        decode_pos(hex)
    }

    fn set_axis(&self, axis: char, steps: i32) -> MmResult<()> {
        let cmd = format!("NP{}T{}", axis, encode_pos(steps));
        self.hub.execute(&cmd)
    }

    fn get_xy_firmware_version(&self) -> MmResult<String> {
        let resp = self.send("NPTv0")?;
        Ok(resp
            .strip_prefix("PN")
            .ok_or(MmError::SerialInvalidResponse)?
            .to_string())
    }

    fn read_busy(&self) -> MmResult<bool> {
        for axis in ['X', 'Y'] {
            let resp = self.send(&format!("NP{}m1", axis))?;
            let body = resp
                .strip_prefix("PN")
                .ok_or(MmError::SerialInvalidResponse)?;
            let status = u8::from_str_radix(body.get(0..1).unwrap_or("0"), 16)
                .map_err(|_| MmError::SerialInvalidResponse)?;
            if (status & 0x01) != 0 {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

impl Default for ZeissMcu28XYStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ZeissMcu28XYStage {
    fn name(&self) -> &str {
        "ZeissMCU28XYStage"
    }
    fn description(&self) -> &str {
        "Zeiss MCU28 XY stage controller"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if !self.hub.is_connected() {
            return Err(MmError::NotConnected);
        }
        self.firmware = self.get_xy_firmware_version()?;
        if let Some(entry) = self.props.entry_mut("XY firmware") {
            entry.value = PropertyValue::String(self.firmware.clone());
        }
        let xs = self.get_axis('X')?;
        let ys = self.get_axis('Y')?;
        self.x_um.set(xs as f64 * self.step_size_um.get());
        self.y_um.set(ys as f64 * self.step_size_um.get());
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "X_um" => Ok(PropertyValue::Float(self.get_xy_position_um()?.0)),
            "Y_um" => Ok(PropertyValue::Float(self.get_xy_position_um()?.1)),
            "StepSize(um)" => Ok(PropertyValue::Float(self.step_size_um.get())),
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "X_um" => {
                let x = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_xy_position_um(x, self.y_um.get())?;
                self.props.set(name, PropertyValue::Float(x))
            }
            "Y_um" => {
                let y = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_xy_position_um(self.x_um.get(), y)?;
                self.props.set(name, PropertyValue::Float(y))
            }
            "StepSize(um)" => {
                let step = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.step_size_um.set(step);
                self.props.set(name, PropertyValue::Float(step))
            }
            _ => self.props.set(name, val),
        }
    }
    fn property_names(&self) -> Vec<String> {
        self.props.property_names().to_vec()
    }
    fn has_property(&self, name: &str) -> bool {
        self.props.has_property(name)
    }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType {
        DeviceType::XYStage
    }
    fn busy(&self) -> bool {
        if self.firmware.starts_with("MF") {
            return false;
        }
        self.read_busy().unwrap_or(false)
    }
}

impl XYStage for ZeissMcu28XYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        self.set_axis('X', (x / self.step_size_um.get()).round() as i32)?;
        self.set_axis('Y', (y / self.step_size_um.get()).round() as i32)?;
        self.x_um.set(x);
        self.y_um.set(y);
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> {
        let x = self.get_axis('X')? as f64 * self.step_size_um.get();
        let y = self.get_axis('Y')? as f64 * self.step_size_um.get();
        self.x_um.set(x);
        self.y_um.set(y);
        Ok((x, y))
    }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        let (x, y) = self.get_xy_position_um()?;
        self.set_xy_position_um(x + dx, y + dy)
    }

    fn set_origin(&mut self) -> MmResult<()> {
        self.hub.execute("NPXP0")?;
        self.hub.execute("NPYP0")?;
        self.x_um.set(0.0);
        self.y_um.set(0.0);
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        Ok(())
    }
    fn stop(&mut self) -> MmResult<()> {
        self.hub.execute("NPXS")?;
        self.hub.execute("NPYS")?;
        Ok(())
    }
    fn get_step_size_um(&self) -> (f64, f64) {
        (self.step_size_um.get(), self.step_size_um.get())
    }
    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Err(MmError::NotSupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn stage_with(t: MockTransport) -> ZeissMcu28XYStage {
        let hub = ZeissHub::new().with_transport(Box::new(t));
        ZeissMcu28XYStage::new_with_hub(hub)
    }

    #[test]
    fn initialize_reads_position() {
        // NPXp → PN000064 = 100 steps = 20 µm; NPYp → PN000032 = 50 steps = 10 µm
        let t = MockTransport::new()
            .expect("NPTv0\r", "PNMCU28")
            .expect("NPXp\r", "PN000064")
            .expect("NPYp\r", "PN000032")
            .expect("NPXp\r", "PN000064")
            .expect("NPYp\r", "PN000032");
        let mut s = stage_with(t);
        s.initialize().unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 20.0).abs() < 1e-6);
        assert!((y - 10.0).abs() < 1e-6);
    }

    #[test]
    fn move_absolute() {
        let t = MockTransport::new()
            .expect("NPTv0\r", "PNMCU28")
            .expect("NPXp\r", "PN000000")
            .expect("NPYp\r", "PN000000")
            .expect("NPXp\r", "PN0001F4")
            .expect("NPYp\r", "PN0003E8");
        let mut s = stage_with(t);
        s.initialize().unwrap();
        s.set_xy_position_um(100.0, 200.0).unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 100.0).abs() < 1e-6);
        assert!((y - 200.0).abs() < 1e-6);
    }

    #[test]
    fn stop_sends_both_axes() {
        let t = MockTransport::new()
            .expect("NPTv0\r", "PNMCU28")
            .expect("NPXp\r", "PN000000")
            .expect("NPYp\r", "PN000000");
        let mut s = stage_with(t);
        s.initialize().unwrap();
        s.stop().unwrap();
    }

    #[test]
    fn set_origin_sends_both_axes() {
        let t = MockTransport::new()
            .expect("NPTv0\r", "PNMCU28")
            .expect("NPXp\r", "PN000000")
            .expect("NPYp\r", "PN000000")
            .expect("NPXp\r", "PN000000")
            .expect("NPYp\r", "PN000000");
        let mut s = stage_with(t);
        s.initialize().unwrap();
        s.set_origin().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (0.0, 0.0));
    }

    #[test]
    fn home_is_noop() {
        let t = MockTransport::new()
            .expect("NPTv0\r", "PNMCU28")
            .expect("NPXp\r", "PN000064")
            .expect("NPYp\r", "PN000032")
            .expect("NPXp\r", "PN000064")
            .expect("NPYp\r", "PN000032");
        let mut s = stage_with(t);
        s.initialize().unwrap();
        s.home().unwrap();
        assert_eq!(s.get_xy_position_um().unwrap(), (20.0, 10.0));
    }
}
