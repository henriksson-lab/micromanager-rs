/// Zeiss MCU28 XY stage controller.
///
/// Protocol (TX `\r`, RX `\r`):
///   `NPXp\r`         → `PN{hex6}\r`   (query X position)
///   `NPYp\r`         → `PN{hex6}\r`   (query Y position)
///   `NPXT{hex6}\r`   → `PN\r`         (set X position)
///   `NPYT{hex6}\r`   → `PN\r`         (set Y position)
///
/// Step size: 0.2 µm / step for both axes.
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, XYStage};
use micromanager::types::{DeviceType, PropertyValue};

use crate::hub::{ZeissHub, decode_pos, encode_pos};

const STEPS_PER_UM: f64 = 5.0; // 0.2 µm/step → 5 steps/µm

pub struct ZeissMcu28XYStage {
    props: PropertyMap,
    hub: ZeissHub,
    initialized: bool,
    x_um: f64,
    y_um: f64,
}

impl ZeissMcu28XYStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self { props, hub: ZeissHub::new(), initialized: false, x_um: 0.0, y_um: 0.0 }
    }

    pub fn new_with_hub(hub: ZeissHub) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self { props, hub, initialized: false, x_um: 0.0, y_um: 0.0 }
    }

    fn send(&mut self, cmd: &str) -> MmResult<String> {
        self.hub.send(cmd)
    }

    fn get_axis(&mut self, axis: char) -> MmResult<i32> {
        let resp = self.send(&format!("NP{}p", axis))?;
        let hex = resp.strip_prefix("PN").unwrap_or(&resp);
        decode_pos(hex)
    }

    fn set_axis(&mut self, axis: char, steps: i32) -> MmResult<()> {
        let cmd = format!("NP{}T{}", axis, encode_pos(steps));
        let resp = self.send(&cmd)?;
        if resp.starts_with("PN") {
            Ok(())
        } else {
            Err(MmError::LocallyDefined(format!("MCU28 {} set error: '{}'", axis, resp)))
        }
    }
}

impl Default for ZeissMcu28XYStage { fn default() -> Self { Self::new() } }

impl Device for ZeissMcu28XYStage {
    fn name(&self) -> &str { "ZeissMCU28XYStage" }
    fn description(&self) -> &str { "Zeiss MCU28 XY stage controller" }

    fn initialize(&mut self) -> MmResult<()> {
        if !self.hub.is_connected() { return Err(MmError::NotConnected); }
        let xs = self.get_axis('X')?;
        let ys = self.get_axis('Y')?;
        self.x_um = xs as f64 / STEPS_PER_UM;
        self.y_um = ys as f64 / STEPS_PER_UM;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> { self.props.set(name, val) }
    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::XYStage }
    fn busy(&self) -> bool { false }
}

impl XYStage for ZeissMcu28XYStage {
    fn set_xy_position_um(&mut self, x: f64, y: f64) -> MmResult<()> {
        self.set_axis('X', (x * STEPS_PER_UM).round() as i32)?;
        self.set_axis('Y', (y * STEPS_PER_UM).round() as i32)?;
        self.x_um = x;
        self.y_um = y;
        Ok(())
    }

    fn get_xy_position_um(&self) -> MmResult<(f64, f64)> { Ok((self.x_um, self.y_um)) }

    fn set_relative_xy_position_um(&mut self, dx: f64, dy: f64) -> MmResult<()> {
        self.set_xy_position_um(self.x_um + dx, self.y_um + dy)
    }

    fn set_origin(&mut self) -> MmResult<()> {
        self.x_um = 0.0;
        self.y_um = 0.0;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> { self.set_xy_position_um(0.0, 0.0) }
    fn stop(&mut self) -> MmResult<()> { Ok(()) }
    fn get_step_size_um(&self) -> (f64, f64) { (1.0 / STEPS_PER_UM, 1.0 / STEPS_PER_UM) }
    fn get_limits_um(&self) -> MmResult<(f64, f64, f64, f64)> {
        Ok((-50_000.0, 50_000.0, -50_000.0, 50_000.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn stage_with(t: MockTransport) -> ZeissMcu28XYStage {
        let hub = ZeissHub::new().with_transport(Box::new(t));
        ZeissMcu28XYStage::new_with_hub(hub)
    }

    #[test]
    fn initialize_reads_position() {
        // NPXp → PN000064 = 100 steps = 20 µm; NPYp → PN000032 = 50 steps = 10 µm
        let t = MockTransport::new().any("PN000064").any("PN000032");
        let mut s = stage_with(t);
        s.initialize().unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 20.0).abs() < 1e-6);
        assert!((y - 10.0).abs() < 1e-6);
    }

    #[test]
    fn move_absolute() {
        let t = MockTransport::new().any("PN000000").any("PN000000").any("PN").any("PN");
        let mut s = stage_with(t);
        s.initialize().unwrap();
        s.set_xy_position_um(100.0, 200.0).unwrap();
        let (x, y) = s.get_xy_position_um().unwrap();
        assert!((x - 100.0).abs() < 1e-6);
        assert!((y - 200.0).abs() < 1e-6);
    }
}
