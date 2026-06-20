/// TOFRA Filter Wheel with IMS MDrive integrated controller.
///
/// Protocol (TX `\r`, RX `\r`):
///   Command: `/<ctrl><cmd>[params]R\r`  (parameter commands end with `R`)
///   Simple:  `/<ctrl><cmd>\r`
///   Response: contains `/0<status><data>` where status `@` = busy
///
/// Init (home + set motor params):
///   `/<ctrl>j16h<HC>m<RC>V<SV>v<IV>L<ACC>f0n0gD10S13G0D1gD1S03G0R\r`
///
/// Move (relative, shortest path):
///   Forward: `/<ctrl>P<steps>R\r`
///   Backward: `/<ctrl>D<steps>R\r`
///
/// Total microsteps per revolution: 3200 (j16 = 1/16 step × 200 full steps)
/// Position steps: floor(3200 / NumPos × i + 0.5) for i in 0..NumPos
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::RefCell;

const TURN_MSTEPS: i64 = 3200;
const DEFAULT_NUM_POS: u64 = 10;
const DEFAULT_HC: i64 = 5;
const DEFAULT_RC: i64 = 60;
const DEFAULT_SV: i64 = 5000;
const DEFAULT_IV: i64 = 500;
const DEFAULT_ACC: i64 = 10;

pub struct TofraFilterWheel {
    props: PropertyMap,
    transport: RefCell<Option<Box<dyn Transport>>>,
    initialized: bool,
    ctrl: String,
    num_positions: u64,
    home_offset: i64,
    slew_velocity: i64,
    init_velocity: i64,
    acceleration: i64,
    hold_current: i64,
    run_current: i64,
    position: u64,
    labels: Vec<String>,
    gate_open: bool,
    port: String,
}

impl TofraFilterWheel {
    pub fn new() -> Self {
        let num_positions = DEFAULT_NUM_POS;
        let labels = (0..num_positions)
            .map(|i| format!("Filter-{:02}", i + 1))
            .collect();
        let mut props = PropertyMap::new();
        props
            .define_pre_init_property("Port", PropertyValue::String(String::new()))
            .unwrap();
        props
            .define_pre_init_property("NumPos", PropertyValue::Integer(DEFAULT_NUM_POS as i64))
            .unwrap();
        props
            .define_pre_init_property("ControllerName", PropertyValue::String("1".into()))
            .unwrap();
        props
            .define_pre_init_property("HomeOffset", PropertyValue::Integer(0))
            .unwrap();
        props
            .define_pre_init_property("SlewVelocity", PropertyValue::Integer(DEFAULT_SV))
            .unwrap();
        props
            .define_pre_init_property("InitVelocity", PropertyValue::Integer(DEFAULT_IV))
            .unwrap();
        props
            .define_pre_init_property("Acceleration", PropertyValue::Integer(DEFAULT_ACC))
            .unwrap();
        props
            .define_pre_init_property("HoldCurrent", PropertyValue::Integer(DEFAULT_HC))
            .unwrap();
        props
            .define_pre_init_property("RunCurrent", PropertyValue::Integer(DEFAULT_RC))
            .unwrap();
        Self {
            props,
            transport: RefCell::new(None),
            initialized: false,
            ctrl: "1".into(),
            num_positions,
            home_offset: 0,
            slew_velocity: DEFAULT_SV,
            init_velocity: DEFAULT_IV,
            acceleration: DEFAULT_ACC,
            hold_current: DEFAULT_HC,
            run_current: DEFAULT_RC,
            position: 0,
            labels,
            gate_open: true,
            port: String::new(),
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = RefCell::new(Some(t));
        self
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.borrow_mut().as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&self, command: &str) -> MmResult<String> {
        let full = format!("/{}{}\r", self.ctrl, command);
        self.call_transport(|t| Ok(t.send_recv(&full)?.trim().to_string()))
    }

    fn check_response(resp: &str) -> MmResult<()> {
        if resp.find("/0").is_some() {
            Ok(())
        } else {
            Err(MmError::LocallyDefined(format!("bad response: {}", resp)))
        }
    }

    fn parse_status(resp: &str) -> MmResult<char> {
        let ind = resp
            .find("/0")
            .ok_or_else(|| MmError::LocallyDefined(format!("bad response: {}", resp)))?;
        resp[ind + 2..]
            .chars()
            .next()
            .ok_or_else(|| MmError::LocallyDefined(format!("bad response: {}", resp)))
    }

    fn msteps_for_pos(num_pos: u64, i: u64) -> i64 {
        (TURN_MSTEPS as f64 / num_pos as f64 * i as f64 + 0.5).floor() as i64
    }

    fn clear_port(&self) -> MmResult<()> {
        self.call_transport(|t| t.purge())
    }

    fn rebuild_labels(&mut self) {
        self.labels = (0..self.num_positions)
            .map(|i| format!("Filter-{:02}", i + 1))
            .collect();
    }
}

impl Default for TofraFilterWheel {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for TofraFilterWheel {
    fn name(&self) -> &str {
        "TOFRA Filter Wheel"
    }
    fn description(&self) -> &str {
        "TOFRA Filter Wheel with Integrated Controller 10, 12, 18 or 22 pos."
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.borrow().is_none() {
            return Err(MmError::NotConnected);
        }
        self.rebuild_labels();
        let home = match self.home_offset.cmp(&0) {
            std::cmp::Ordering::Greater => format!("D{}", self.home_offset),
            std::cmp::Ordering::Less => format!("P{}", -self.home_offset),
            std::cmp::Ordering::Equal => String::new(),
        };
        let init_cmd = format!(
            "j16h{}m{}V{}v{}L{}f0n0gD10S13G0D1gD1S03G0{}R",
            self.hold_current,
            self.run_current,
            self.slew_velocity,
            self.init_velocity,
            self.acceleration,
            home
        );
        self.clear_port()?;
        let resp = self.cmd(&init_cmd)?;
        Self::check_response(&resp)?;
        self.position = 0;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Port" => Ok(PropertyValue::String(self.port.clone())),
            "NumPos" => Ok(PropertyValue::Integer(self.num_positions as i64)),
            "ControllerName" => Ok(PropertyValue::String(self.ctrl.clone())),
            "HomeOffset" => Ok(PropertyValue::Integer(self.home_offset)),
            "SlewVelocity" => Ok(PropertyValue::Integer(self.slew_velocity)),
            "InitVelocity" => Ok(PropertyValue::Integer(self.init_velocity)),
            "Acceleration" => Ok(PropertyValue::Integer(self.acceleration)),
            "HoldCurrent" => Ok(PropertyValue::Integer(self.hold_current)),
            "RunCurrent" => Ok(PropertyValue::Integer(self.run_current)),
            "State" => Ok(PropertyValue::Integer(self.position as i64)),
            "Label" => Ok(PropertyValue::String(
                self.labels
                    .get(self.position as usize)
                    .cloned()
                    .unwrap_or_default(),
            )),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" if self.initialized => {
                if let Some(e) = self.props.entry_mut("Port") {
                    e.value = PropertyValue::String(self.port.clone());
                }
                Ok(())
            }
            "Port" => {
                self.port = val.as_str().to_string();
                self.props
                    .set(name, PropertyValue::String(self.port.clone()))
            }
            "ControllerName" if !self.initialized => {
                self.ctrl = val.as_str().to_string();
                self.props
                    .set(name, PropertyValue::String(self.ctrl.clone()))
            }
            "NumPos" if !self.initialized => {
                let n = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if n <= 0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.num_positions = n as u64;
                self.rebuild_labels();
                self.props.set(name, PropertyValue::Integer(n))
            }
            "HomeOffset" if !self.initialized => {
                self.home_offset = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Integer(self.home_offset))
            }
            "SlewVelocity" if !self.initialized => {
                self.slew_velocity = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Integer(self.slew_velocity))
            }
            "InitVelocity" if !self.initialized => {
                self.init_velocity = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Integer(self.init_velocity))
            }
            "Acceleration" if !self.initialized => {
                self.acceleration = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Integer(self.acceleration))
            }
            "HoldCurrent" if !self.initialized => {
                self.hold_current = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Integer(self.hold_current))
            }
            "RunCurrent" if !self.initialized => {
                self.run_current = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.props
                    .set(name, PropertyValue::Integer(self.run_current))
            }
            "State" => {
                let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u64;
                self.set_position(pos)
            }
            "Label" => {
                let label = val.as_str().to_string();
                self.set_position_by_label(&label)
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
        DeviceType::State
    }
    fn busy(&self) -> bool {
        self.cmd("Q")
            .and_then(|resp| Self::parse_status(&resp))
            .map(|status| status == '@')
            .unwrap_or(false)
    }
}

impl StateDevice for TofraFilterWheel {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::UnknownPosition);
        }
        if self.initialized && pos != self.position {
            let cur_steps = Self::msteps_for_pos(self.num_positions, self.position);
            let tgt_steps = Self::msteps_for_pos(self.num_positions, pos);
            let d1 = tgt_steps - cur_steps;
            let d2 = if d1 > 0 {
                d1 - TURN_MSTEPS
            } else {
                TURN_MSTEPS + d1
            };
            let d = if d1.abs() > d2.abs() { d2 } else { d1 };
            let move_cmd = if d > 0 {
                format!("P{}R", d)
            } else {
                format!("D{}R", -d)
            };
            self.clear_port()?;
            let resp = self.cmd(&move_cmd)?;
            Self::check_response(&resp)?;
        }
        self.position = pos;
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> {
        Ok(self.position)
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

    fn init_cmd() -> String {
        format!(
            "/1j16h{}m{}V{}v{}L{}f0n0gD10S13G0D1gD1S03G0R\r",
            DEFAULT_HC, DEFAULT_RC, DEFAULT_SV, DEFAULT_IV, DEFAULT_ACC
        )
    }

    fn make_init_transport() -> MockTransport {
        MockTransport::new().expect(&init_cmd(), "/00")
    }

    #[test]
    fn initialize() {
        let mut fw = TofraFilterWheel::new().with_transport(Box::new(make_init_transport()));
        fw.initialize().unwrap();
        assert_eq!(fw.get_position().unwrap(), 0);
        assert_eq!(fw.get_number_of_positions(), 10);
    }

    #[test]
    fn move_forward() {
        // Position 0→3: steps = floor(3200/10*3+0.5)=960 - floor(3200/10*0+0.5)=0 = 960
        // |960| < |960-3200|=2240, so d=960, forward: P960R
        let t = make_init_transport().expect("/1P960R\r", "/00");
        let mut fw = TofraFilterWheel::new().with_transport(Box::new(t));
        fw.initialize().unwrap();
        fw.set_position(3).unwrap();
        assert_eq!(fw.get_position().unwrap(), 3);
    }

    #[test]
    fn move_backward_shortest() {
        // Position 0→9: steps = 2880 - 0 = 2880
        // d1=2880, d2=2880-3200=-320, |2880|>|-320|, so d=d2=-320, backward: D320R
        let t = make_init_transport().expect("/1D320R\r", "/00");
        let mut fw = TofraFilterWheel::new().with_transport(Box::new(t));
        fw.initialize().unwrap();
        fw.set_position(9).unwrap();
        assert_eq!(fw.get_position().unwrap(), 9);
    }

    #[test]
    fn labels() {
        let t = make_init_transport().any("/00");
        let mut fw = TofraFilterWheel::new().with_transport(Box::new(t));
        fw.initialize().unwrap();
        fw.set_position_label(2, "DAPI").unwrap();
        fw.set_position_by_label("DAPI").unwrap();
        assert_eq!(fw.get_position().unwrap(), 2);
    }

    #[test]
    fn out_of_range() {
        let mut fw = TofraFilterWheel::new().with_transport(Box::new(make_init_transport()));
        fw.initialize().unwrap();
        assert!(fw.set_position(10).is_err());
    }

    #[test]
    fn no_transport_error() {
        assert!(TofraFilterWheel::new().initialize().is_err());
    }

    #[test]
    fn busy_polls_controller_status() {
        let t = make_init_transport().expect("/1Q\r", "/0@");
        let mut fw = TofraFilterWheel::new().with_transport(Box::new(t));
        fw.initialize().unwrap();
        assert!(fw.busy());
    }

    #[test]
    fn preinit_config_changes_homing_command_and_port_reverts_after_init() {
        let t = MockTransport::new()
            .expect("/7j16h10m65V6000v600L8f0n0gD10S13G0D1gD1S03G0D42R\r", "/00");
        let mut fw = TofraFilterWheel::new().with_transport(Box::new(t));
        fw.set_property("Port", PropertyValue::String("COM1".into()))
            .unwrap();
        fw.set_property("ControllerName", PropertyValue::String("7".into()))
            .unwrap();
        fw.set_property("NumPos", PropertyValue::Integer(12))
            .unwrap();
        fw.set_property("HomeOffset", PropertyValue::Integer(42))
            .unwrap();
        fw.set_property("SlewVelocity", PropertyValue::Integer(6000))
            .unwrap();
        fw.set_property("InitVelocity", PropertyValue::Integer(600))
            .unwrap();
        fw.set_property("Acceleration", PropertyValue::Integer(8))
            .unwrap();
        fw.set_property("HoldCurrent", PropertyValue::Integer(10))
            .unwrap();
        fw.set_property("RunCurrent", PropertyValue::Integer(65))
            .unwrap();
        fw.initialize().unwrap();
        fw.set_property("Port", PropertyValue::String("COM2".into()))
            .unwrap();
        assert_eq!(
            fw.get_property("Port").unwrap(),
            PropertyValue::String("COM1".into())
        );
        assert_eq!(fw.get_number_of_positions(), 12);
    }

    #[test]
    fn negative_home_offset_uses_reverse_home_segment() {
        let t = MockTransport::new()
            .expect("/1j16h5m60V5000v500L10f0n0gD10S13G0D1gD1S03G0P4R\r", "/00");
        let mut fw = TofraFilterWheel::new().with_transport(Box::new(t));
        fw.set_property("HomeOffset", PropertyValue::Integer(-4))
            .unwrap();
        fw.initialize().unwrap();
    }
}
