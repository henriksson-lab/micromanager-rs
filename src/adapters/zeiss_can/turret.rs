/// Zeiss CAN-bus turret/filter-wheel devices (StateDevice).
///
/// Protocol (TX `\r`, RX `\r`):
///   `HPCr{id},1\r`      → `PH{pos}\r`      (query current position)
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
    Reflector = 1,
    Objective = 2,
    ExternalFilterWheel = 4,
    FilterWheel1 = 7,
    FilterWheel2 = 8,
    Condenser = 32,
    BasePort = 38,
    SidePort = 39,
    LampMirror = 51,
    Optovar = 6,
    TubeLens = 36,
}

impl TurretId {
    pub fn id(self) -> u8 {
        self as u8
    }
}

use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, StateDevice};
use crate::types::{DeviceType, PropertyValue};
use std::cell::Cell;

use super::hub::ZeissHub;

pub struct ZeissTurret {
    props: PropertyMap,
    hub: ZeissHub,
    initialized: bool,
    turret_id: u8,
    num_positions: u64,
    current_pos: Cell<u64>,
    name: String,
    labels: Vec<String>,
    gate_open: bool,
}

impl ZeissTurret {
    pub fn new(turret: TurretId, num_positions: u64) -> Self {
        let name = turret.default_name().to_string();
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        let labels = (0..num_positions).map(|i| format!("{}", i)).collect();
        Self {
            props,
            hub: ZeissHub::new(),
            initialized: false,
            turret_id: turret.id(),
            num_positions,
            current_pos: Cell::new(0),
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

    fn send(&self, cmd: &str) -> MmResult<String> {
        self.hub.send(cmd)
    }

    fn read_position(&self) -> MmResult<u64> {
        let resp = self.send(&format!("HPCr{},1", self.turret_id))?;
        let pos = ZeissHub::parse_prefixed_i64(&resp, "PH")?;
        if pos <= 0 {
            return Ok(0);
        }
        Ok((pos as u64).saturating_sub(1))
    }

    fn read_max_position(&self) -> MmResult<u64> {
        let query = if self.hub.firmware() == "MF" { 2 } else { 3 };
        let resp = self.send(&format!("HPCr{},{}", self.turret_id, query))?;
        let max = ZeissHub::parse_prefixed_i64(&resp, "PH")?;
        if max <= 0 {
            return Err(MmError::UnknownPosition);
        }
        Ok(max as u64)
    }

    fn read_presence(&self) -> MmResult<bool> {
        let resp = self.send(&format!("HPCr{},0", self.turret_id))?;
        match ZeissHub::parse_prefixed_i64(&resp, "PH")? {
            0 => Ok(false),
            1 | 2 => Ok(true),
            _ => Err(MmError::SerialInvalidResponse),
        }
    }

    fn read_busy(&self) -> MmResult<bool> {
        let group = self.busy_group().ok_or(MmError::InvalidPropertyValue)?;
        let resp = self.send(&format!("HPSb{}", group))?;
        let status = ZeissHub::parse_prefixed_i64(&resp, "PH")? as u8;
        Ok(((status >> self.busy_bit().unwrap_or(0)) & 1) != 0)
    }

    fn busy_group(&self) -> Option<u8> {
        match self.turret_id {
            1 | 2 | 7 | 8 | 32 | 6 | 36 => Some(1),
            4 | 38 | 39 | 51 => Some(2),
            _ => None,
        }
    }

    fn busy_bit(&self) -> Option<u8> {
        match self.turret_id {
            1 => Some(0),
            2 => Some(1),
            7 => Some(2),
            8 => Some(3),
            32 => Some(4),
            6 | 36 => Some(5),
            38 => Some(0),
            39 => Some(1),
            51 => Some(6),
            4 => Some(7),
            _ => None,
        }
    }
}

impl TurretId {
    fn default_name(self) -> &'static str {
        match self {
            TurretId::Reflector => "ZeissReflectorTurret",
            TurretId::Objective => "ZeissObjectives",
            TurretId::FilterWheel1 => "ZeissFilterWheel1",
            TurretId::FilterWheel2 => "ZeissFilterWheel2",
            TurretId::Condenser => "ZeissCondenser",
            TurretId::BasePort => "ZeissBasePortSlider",
            TurretId::SidePort => "ZeissSidePortTurret",
            TurretId::LampMirror => "ZeissExcitationLampSwitcher",
            TurretId::ExternalFilterWheel => "ZeissExternalFilterWheel",
            TurretId::Optovar => "ZeissOptovar",
            TurretId::TubeLens => "ZeissTubelens",
        }
    }

    fn label_prefix(self) -> &'static str {
        match self {
            TurretId::Reflector => "Dichroic",
            _ => "Position",
        }
    }
}

impl Device for ZeissTurret {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        "Zeiss CAN-bus turret / filter wheel"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if !self.hub.is_connected() {
            return Err(MmError::NotConnected);
        }
        if !self.read_presence()? {
            return Err(MmError::NotConnected);
        }
        self.num_positions = self.read_max_position()?;
        self.labels = (0..self.num_positions)
            .map(|i| {
                let prefix = match self.turret_id {
                    1 => TurretId::Reflector.label_prefix(),
                    _ => "Position",
                };
                format!("{}-{}", prefix, i + 1)
            })
            .collect();
        self.current_pos.set(self.read_position()?);
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        self.props.set(name, val)
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
        DeviceType::State
    }
    fn busy(&self) -> bool {
        if self.turret_id == TurretId::Reflector.id() {
            if matches!(self.read_position(), Ok(0)) {
                return true;
            }
        }
        self.read_busy().unwrap_or(false)
    }
}

impl StateDevice for ZeissTurret {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::LocallyDefined(format!(
                "Turret position {} out of range",
                pos
            )));
        }
        let id = self.turret_id;
        let zeiss_pos = pos + 1; // Zeiss is 1-indexed
        self.hub.execute(&format!("HPCR{},{}", id, zeiss_pos))?;
        self.current_pos.set(pos);
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> {
        let pos = self.read_position()?;
        if pos >= self.num_positions {
            return Err(MmError::UnknownPosition);
        }
        self.current_pos.set(pos);
        Ok(pos)
    }

    fn get_number_of_positions(&self) -> u64 {
        self.num_positions
    }

    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.labels
            .get(pos as usize)
            .cloned()
            .ok_or(MmError::UnknownPosition)
    }

    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let pos = self
            .labels
            .iter()
            .position(|l| l == label)
            .ok_or_else(|| MmError::UnknownLabel(label.to_string()))? as u64;
        self.set_position(pos)
    }

    fn set_position_label(&mut self, pos: u64, label: &str) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::UnknownPosition);
        }
        self.labels[pos as usize] = label.to_string();
        Ok(())
    }

    fn set_gate_open(&mut self, open: bool) -> MmResult<()> {
        self.gate_open = open;
        Ok(())
    }

    fn get_gate_open(&self) -> MmResult<bool> {
        Ok(self.gate_open)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn turret_with(t: MockTransport) -> ZeissTurret {
        let hub = ZeissHub::new().with_transport(Box::new(t));
        ZeissTurret::new_with_hub(TurretId::Objective, 6, hub)
    }

    #[test]
    fn initialize_reads_position() {
        // HPCr2,1 → PH3 (Zeiss position 3 → 0-indexed position 2)
        let t = MockTransport::new()
            .expect("HPCr2,0\r", "PH1")
            .expect("HPCr2,3\r", "PH6")
            .expect("HPCr2,1\r", "PH3")
            .expect("HPCr2,1\r", "PH3");
        let mut s = turret_with(t);
        s.initialize().unwrap();
        assert_eq!(s.get_position().unwrap(), 2);
    }

    #[test]
    fn set_position() {
        let t = MockTransport::new()
            .expect("HPCr2,0\r", "PH1")
            .expect("HPCr2,3\r", "PH6")
            .expect("HPCr2,1\r", "PH1")
            .expect("HPCr2,1\r", "PH4");
        let mut s = turret_with(t);
        s.initialize().unwrap();
        s.set_position(3).unwrap();
        assert_eq!(s.get_position().unwrap(), 3);
    }

    #[test]
    fn out_of_range_fails() {
        let t = MockTransport::new()
            .expect("HPCr2,0\r", "PH1")
            .expect("HPCr2,3\r", "PH6")
            .expect("HPCr2,1\r", "PH1");
        let mut s = turret_with(t);
        s.initialize().unwrap();
        assert!(s.set_position(10).is_err());
    }

    #[test]
    fn turret_ids_match_zeiss_can_constants() {
        assert_eq!(TurretId::Reflector.id(), 1);
        assert_eq!(TurretId::Objective.id(), 2);
        assert_eq!(TurretId::ExternalFilterWheel.id(), 4);
        assert_eq!(TurretId::Optovar.id(), 6);
        assert_eq!(TurretId::FilterWheel1.id(), 7);
        assert_eq!(TurretId::FilterWheel2.id(), 8);
        assert_eq!(TurretId::Condenser.id(), 32);
        assert_eq!(TurretId::TubeLens.id(), 36);
        assert_eq!(TurretId::BasePort.id(), 38);
        assert_eq!(TurretId::SidePort.id(), 39);
        assert_eq!(TurretId::LampMirror.id(), 51);
    }
}
