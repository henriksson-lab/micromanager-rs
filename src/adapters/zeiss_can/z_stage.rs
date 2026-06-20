/// Zeiss CAN-bus focus (Z) stage.
///
/// Protocol (TX `\r`, RX `\r`):
///   `HPZp\r`         → `PH{hex6}\r`   (query Z position)
///   `HPZT{hex6}\r`   → `PH\r`         (set Z position, 24-bit two's-complement hex)
///
/// Step size: 0.025 µm / step.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Stage};
use crate::types::{DeviceType, FocusDirection, PropertyValue};
use std::cell::Cell;

use super::hub::{decode_pos, encode_pos, ZeissHub};

const STEPS_PER_UM: f64 = 40.0; // 0.025 µm/step → 40 steps/µm

pub struct ZeissFocusStage {
    props: PropertyMap,
    hub: ZeissHub,
    initialized: bool,
    pos_um: Cell<f64>,
    step_size_um: Cell<f64>,
    focus_firmware: String,
    lower_limit: f64,
    upper_limit: f64,
}

impl ZeissFocusStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property(
                "StepSize (um)",
                PropertyValue::Float(1.0 / STEPS_PER_UM),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("StepSize (um)", &["0.025", "0.050"])
            .unwrap();
        props
            .define_property("Focus firmware", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("Position", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property("Load Position", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .set_allowed_values("Load Position", &["0", "1"])
            .unwrap();
        Self {
            props,
            hub: ZeissHub::new(),
            initialized: false,
            pos_um: Cell::new(0.0),
            step_size_um: Cell::new(1.0 / STEPS_PER_UM),
            focus_firmware: String::new(),
            lower_limit: 0.0,
            upper_limit: 1000.0,
        }
    }

    pub fn new_with_hub(hub: ZeissHub) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property(
                "StepSize (um)",
                PropertyValue::Float(1.0 / STEPS_PER_UM),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("StepSize (um)", &["0.025", "0.050"])
            .unwrap();
        props
            .define_property("Focus firmware", PropertyValue::String(String::new()), true)
            .unwrap();
        props
            .define_property("Position", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property("Load Position", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .set_allowed_values("Load Position", &["0", "1"])
            .unwrap();
        Self {
            props,
            hub,
            initialized: false,
            pos_um: Cell::new(0.0),
            step_size_um: Cell::new(1.0 / STEPS_PER_UM),
            focus_firmware: String::new(),
            lower_limit: 0.0,
            upper_limit: 1000.0,
        }
    }

    fn send(&self, cmd: &str) -> MmResult<String> {
        self.hub.send(cmd)
    }

    fn get_pos_steps(&self) -> MmResult<i32> {
        let resp = self.send("HPZp")?;
        // Response: "PH{hex6}" — strip leading "PH"
        let hex = resp.strip_prefix("PH").unwrap_or(&resp);
        decode_pos(hex)
    }

    fn set_pos_steps(&self, steps: i32) -> MmResult<()> {
        let cmd = format!("HPZT{}", encode_pos(steps));
        self.hub.execute(&cmd)
    }

    fn get_focus_firmware_version(&self) -> MmResult<String> {
        let resp = self.send("HPTv0")?;
        Ok(resp
            .strip_prefix("PH")
            .ok_or(MmError::SerialInvalidResponse)?
            .to_string())
    }

    fn read_limit(&self, command: &str) -> MmResult<f64> {
        let resp = self.send(command)?;
        let hex = resp
            .strip_prefix("PH")
            .ok_or(MmError::SerialInvalidResponse)?;
        Ok(decode_pos(hex)? as f64 * self.step_size_um.get())
    }

    fn read_load_position(&self) -> MmResult<i64> {
        let resp = self.send("HPZw")?;
        let state = ZeissHub::parse_prefixed_i64(&resp, "PH")?;
        Ok(if state == 0 || state == 4 { 1 } else { 0 })
    }
}

impl Default for ZeissFocusStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ZeissFocusStage {
    fn name(&self) -> &str {
        "ZeissFocusStage"
    }
    fn description(&self) -> &str {
        "Zeiss CAN-bus focus Z-stage"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if !self.hub.is_connected() {
            return Err(MmError::NotConnected);
        }
        self.focus_firmware = self.get_focus_firmware_version()?;
        if let Some(entry) = self.props.entry_mut("Focus firmware") {
            entry.value = PropertyValue::String(self.focus_firmware.clone());
        }
        self.upper_limit = self.read_limit("HPZu")?;
        self.lower_limit = self.read_limit("HPZl")?;
        let steps = self.get_pos_steps()?;
        self.pos_um.set(steps as f64 * self.step_size_um.get());
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Position" => Ok(PropertyValue::Float(self.get_position_um()?)),
            "StepSize (um)" => Ok(PropertyValue::Float(self.step_size_um.get())),
            "Load Position" => Ok(PropertyValue::Integer(self.read_load_position()?)),
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Position" => {
                let z = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_position_um(z)?;
                self.props.set(name, PropertyValue::Float(z))
            }
            "StepSize (um)" => {
                let step = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if step != 0.025 && step != 0.050 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.step_size_um.set(step);
                self.props.set(name, PropertyValue::Float(step))
            }
            "Load Position" => {
                let state = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.hub
                    .execute(&format!("HPZW{}", if state == 0 { 1 } else { 0 }))?;
                self.props.set(name, PropertyValue::Integer(state))
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
        DeviceType::Stage
    }
    fn busy(&self) -> bool {
        false
    }
}

impl Stage for ZeissFocusStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let steps = (z / self.step_size_um.get()).round() as i32;
        self.set_pos_steps(steps)?;
        self.pos_um.set(z);
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> {
        let pos = self.get_pos_steps()? as f64 * self.step_size_um.get();
        self.pos_um.set(pos);
        Ok(pos)
    }

    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        self.set_position_um(self.get_position_um()? + dz)
    }

    fn home(&mut self) -> MmResult<()> {
        Err(MmError::NotSupported)
    }
    fn stop(&mut self) -> MmResult<()> {
        Ok(())
    }
    fn get_limits(&self) -> MmResult<(f64, f64)> {
        Ok((self.lower_limit, self.upper_limit))
    }
    fn get_focus_direction(&self) -> FocusDirection {
        FocusDirection::Unknown
    }
    fn is_continuous_focus_drive(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn stage_with(t: MockTransport) -> ZeissFocusStage {
        let hub = ZeissHub::new().with_transport(Box::new(t));
        ZeissFocusStage::new_with_hub(hub)
    }

    #[test]
    fn initialize_reads_position() {
        // HPZp → PH000190 = 400 steps = 10 µm
        let t = MockTransport::new()
            .expect("HPTv0\r", "PHAP2_09")
            .expect("HPZu\r", "PH000000")
            .expect("HPZl\r", "PH000000")
            .expect("HPZp\r", "PH000190")
            .expect("HPZp\r", "PH000190");
        let mut s = stage_with(t);
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap() - 10.0).abs() < 1e-6);
    }

    #[test]
    fn move_absolute() {
        // init at 0, then move to 25 µm = 1000 steps = 0x3E8
        let t = MockTransport::new()
            .expect("HPTv0\r", "PHAP2_09")
            .expect("HPZu\r", "PH000000")
            .expect("HPZl\r", "PH000000")
            .expect("HPZp\r", "PH000000")
            .expect("HPZp\r", "PH0003E8");
        let mut s = stage_with(t);
        s.initialize().unwrap();
        s.set_position_um(25.0).unwrap();
        assert!((s.get_position_um().unwrap() - 25.0).abs() < 1e-6);
    }

    #[test]
    fn negative_position() {
        // init at -10 µm = -400 steps → hex FFFEB0... let roundtrip verify
        use super::super::hub::encode_pos;
        let hex = format!("PH{}", encode_pos(-400));
        let t = MockTransport::new()
            .expect("HPTv0\r", "PHAP2_09")
            .expect("HPZu\r", "PH000000")
            .expect("HPZl\r", "PH000000")
            .expect("HPZp\r", &hex)
            .expect("HPZp\r", &hex);
        let mut s = stage_with(t);
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap() - (-10.0)).abs() < 1e-6);
    }

    #[test]
    fn home_is_unsupported() {
        let t = MockTransport::new()
            .expect("HPTv0\r", "PHAP2_09")
            .expect("HPZu\r", "PH000000")
            .expect("HPZl\r", "PH000000")
            .expect("HPZp\r", "PH000000");
        let mut s = stage_with(t);
        s.initialize().unwrap();
        assert_eq!(s.home().unwrap_err(), MmError::NotSupported);
    }
}
