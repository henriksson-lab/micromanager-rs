/// Zeiss CAN-bus turret/filter-wheel devices (StateDevice).
///
/// Protocol (TX `\r`, RX `\r`):
///   `HPCr{id},0\r`      → `PH{pos}\r`      (query current position)
///   `HPCR{id},{pos}\r`  → `PH\r`           (set position)
///   `HPSb1\r`           → `PH{byte}\r`      (group-1 busy status bitmask)
///   `HPSb2\r`           → `PH{byte}\r`      (group-2 busy status bitmask)
///
/// Busy groups:
///   Group 1: reflector, objectives, filter1-4, condenser
///   Group 2: base port, side port, lamp mirror, external filters, optovar, tube lens
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TurretId {
    Reflector     = 1,
    Objective     = 2,
    FilterWheel1  = 10,
    FilterWheel2  = 11,
    FilterWheel3  = 12,
    FilterWheel4  = 13,
    FilterWheel5  = 20,
    FilterWheel6  = 21,
    FilterWheel7  = 22,
    FilterWheel8  = 23,
    Condenser     = 3,
    BasePort      = 30,
    SidePort      = 31,
    LampMirror    = 32,
    Optovar       = 33,
    TubeLens      = 34,
}

impl TurretId {
    pub fn id(self) -> u8 { self as u8 }
}

use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, StateDevice};
use micromanager::types::{DeviceType, PropertyValue};

use crate::hub::ZeissHub;

pub struct ZeissTurret {
    props: PropertyMap,
    hub: ZeissHub,
    initialized: bool,
    turret_id: u8,
    num_positions: u64,
    current_pos: u64,
    name: String,
    labels: Vec<String>,
    gate_open: bool,
}

impl ZeissTurret {
    pub fn new(turret: TurretId, num_positions: u64) -> Self {
        let name = format!("ZeissTurret{}", turret.id());
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        let labels = (0..num_positions).map(|i| format!("{}", i)).collect();
        Self {
            props,
            hub: ZeissHub::new(),
            initialized: false,
            turret_id: turret.id(),
            num_positions,
            current_pos: 0,
            name,
            labels,
            gate_open: true,
        }
    }

    pub fn new_with_hub(turret: TurretId, num_positions: u64, hub: ZeissHub) -> Self {
        let mut s = Self::new(turret, num_positions);
        s.hub = hub;
        s
    }

    fn send(&mut self, cmd: &str) -> MmResult<String> {
        self.hub.send(cmd)
    }
}

impl Device for ZeissTurret {
    fn name(&self) -> &str { &self.name }
    fn description(&self) -> &str { "Zeiss CAN-bus turret / filter wheel" }

    fn initialize(&mut self) -> MmResult<()> {
        if !self.hub.is_connected() { return Err(MmError::NotConnected); }
        let id = self.turret_id;
        let resp = self.send(&format!("HPCr{},0", id))?;
        // Response: "PH{pos}" → strip "PH"
        let pos_str = resp.strip_prefix("PH").unwrap_or(&resp).trim().to_string();
        let pos: u64 = pos_str.parse().unwrap_or(0);
        self.current_pos = pos.saturating_sub(1); // Zeiss 1-indexed → 0-indexed
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
    fn device_type(&self) -> DeviceType { DeviceType::State }
    fn busy(&self) -> bool { false }
}

impl StateDevice for ZeissTurret {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::LocallyDefined(format!("Turret position {} out of range", pos)));
        }
        let id = self.turret_id;
        let zeiss_pos = pos + 1; // Zeiss is 1-indexed
        let resp = self.send(&format!("HPCR{},{}", id, zeiss_pos))?;
        if resp.starts_with("PH") {
            self.current_pos = pos;
            Ok(())
        } else {
            Err(MmError::LocallyDefined(format!("Turret set error: '{}'", resp)))
        }
    }

    fn get_position(&self) -> MmResult<u64> { Ok(self.current_pos) }

    fn get_number_of_positions(&self) -> u64 { self.num_positions }

    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.labels.get(pos as usize).cloned()
            .ok_or(MmError::UnknownPosition)
    }

    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let pos = self.labels.iter().position(|l| l == label)
            .ok_or_else(|| MmError::UnknownLabel(label.to_string()))? as u64;
        self.set_position(pos)
    }

    fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()> {
        if pos >= self.num_positions { return Err(MmError::UnknownPosition); }
        self.labels[pos as usize] = label.to_string();
        Ok(())
    }

    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.gate_open = open;
        Ok(())
    }

    fn get_gate_open(&self) -> MmResult<bool> { Ok(self.gate_open) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn turret_with(t: MockTransport) -> ZeissTurret {
        let hub = ZeissHub::new().with_transport(Box::new(t));
        ZeissTurret::new_with_hub(TurretId::Objective, 6, hub)
    }

    #[test]
    fn initialize_reads_position() {
        // HPCr2,0 → PH3 (Zeiss position 3 → 0-indexed position 2)
        let t = MockTransport::new().any("PH3");
        let mut s = turret_with(t);
        s.initialize().unwrap();
        assert_eq!(s.get_position().unwrap(), 2);
    }

    #[test]
    fn set_position() {
        let t = MockTransport::new().any("PH1").any("PH");
        let mut s = turret_with(t);
        s.initialize().unwrap();
        s.set_position(3).unwrap();
        assert_eq!(s.get_position().unwrap(), 3);
    }

    #[test]
    fn out_of_range_fails() {
        let t = MockTransport::new().any("PH1");
        let mut s = turret_with(t);
        s.initialize().unwrap();
        assert!(s.set_position(10).is_err());
    }
}
