/// Zeiss CAN-bus focus (Z) stage.
///
/// Protocol (TX `\r`, RX `\r`):
///   `HPZp\r`         → `PH{hex6}\r`   (query Z position)
///   `HPZT{hex6}\r`   → `PH\r`         (set Z position, 24-bit two's-complement hex)
///
/// Step size: 0.025 µm / step.
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, Stage};
use mm_device::types::{DeviceType, FocusDirection, PropertyValue};

use crate::hub::{ZeissHub, decode_pos, encode_pos};

const STEPS_PER_UM: f64 = 40.0; // 0.025 µm/step → 40 steps/µm

pub struct ZeissFocusStage {
    props: PropertyMap,
    hub: ZeissHub,
    initialized: bool,
    pos_um: f64,
}

impl ZeissFocusStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self { props, hub: ZeissHub::new(), initialized: false, pos_um: 0.0 }
    }

    pub fn new_with_hub(hub: ZeissHub) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self { props, hub, initialized: false, pos_um: 0.0 }
    }

    fn send(&mut self, cmd: &str) -> MmResult<String> {
        self.hub.send(cmd)
    }

    fn get_pos_steps(&mut self) -> MmResult<i32> {
        let resp = self.send("HPZp")?;
        // Response: "PH{hex6}" — strip leading "PH"
        let hex = resp.strip_prefix("PH").unwrap_or(&resp);
        decode_pos(hex)
    }

    fn set_pos_steps(&mut self, steps: i32) -> MmResult<()> {
        let cmd = format!("HPZT{}", encode_pos(steps));
        let resp = self.send(&cmd)?;
        // Expect "PH" acknowledgement
        if resp.trim_start_matches("PH").is_empty() || resp.starts_with("PH") {
            Ok(())
        } else {
            Err(MmError::LocallyDefined(format!("Zeiss Z set error: '{}'", resp)))
        }
    }
}

impl Default for ZeissFocusStage { fn default() -> Self { Self::new() } }

impl Device for ZeissFocusStage {
    fn name(&self) -> &str { "ZeissFocusStage" }
    fn description(&self) -> &str { "Zeiss CAN-bus focus Z-stage" }

    fn initialize(&mut self) -> MmResult<()> {
        if !self.hub.is_connected() { return Err(MmError::NotConnected); }
        let steps = self.get_pos_steps()?;
        self.pos_um = steps as f64 / STEPS_PER_UM;
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
    fn device_type(&self) -> DeviceType { DeviceType::Stage }
    fn busy(&self) -> bool { false }
}

impl Stage for ZeissFocusStage {
    fn set_position_um(&mut self, z: f64) -> MmResult<()> {
        let steps = (z * STEPS_PER_UM).round() as i32;
        self.set_pos_steps(steps)?;
        self.pos_um = z;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> { Ok(self.pos_um) }

    fn set_relative_position_um(&mut self, dz: f64) -> MmResult<()> {
        self.set_position_um(self.pos_um + dz)
    }

    fn home(&mut self) -> MmResult<()> { self.set_position_um(0.0) }
    fn stop(&mut self) -> MmResult<()> { Ok(()) }
    fn get_limits(&self) -> MmResult<(f64, f64)> { Ok((-5_000.0, 5_000.0)) }
    fn get_focus_direction(&self) -> FocusDirection { FocusDirection::Unknown }
    fn is_continuous_focus_drive(&self) -> bool { false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    fn stage_with(t: MockTransport) -> ZeissFocusStage {
        let hub = ZeissHub::new().with_transport(Box::new(t));
        ZeissFocusStage::new_with_hub(hub)
    }

    #[test]
    fn initialize_reads_position() {
        // HPZp → PH000190 = 400 steps = 10 µm
        let t = MockTransport::new().any("PH000190");
        let mut s = stage_with(t);
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap() - 10.0).abs() < 1e-6);
    }

    #[test]
    fn move_absolute() {
        // init at 0, then move to 25 µm = 1000 steps = 0x3E8
        let t = MockTransport::new().any("PH000000").any("PH");
        let mut s = stage_with(t);
        s.initialize().unwrap();
        s.set_position_um(25.0).unwrap();
        assert!((s.get_position_um().unwrap() - 25.0).abs() < 1e-6);
    }

    #[test]
    fn negative_position() {
        // init at -10 µm = -400 steps → hex FFFEB0... let roundtrip verify
        use crate::hub::encode_pos;
        let hex = format!("PH{}", encode_pos(-400));
        let t = MockTransport::new().any(&hex);
        let mut s = stage_with(t);
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap() - (-10.0)).abs() < 1e-6);
    }
}
