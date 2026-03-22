/// TOFRA Z-Drive with IMS MDrive integrated controller.
///
/// Protocol (TX `\r`, RX `\r`):
///   Init:    `/<ctrl>j256h<HC>m<RC>V<slvel>v<invel>L<accel>n<n>R\r`
///   Query:   `/<ctrl>?0\r`         → `/0<status><steps>`
///   Abs:     `/<ctrl>A<steps>R\r`  → `/0<status>`
///   Rel +:   `/<ctrl>P<steps>R\r`  → `/0<status>`
///   Rel -:   `/<ctrl>D<steps>R\r`  → `/0<status>`
///   Stop:    `/<ctrl>T\r`          → `/0<status>`
///   Origin:  `/<ctrl>z0R\r`        → `/0<status>`
///   Home:    `/<ctrl>Z1000000000R\r` → `/0<status>`
///
/// Response format: find `/0` at index `ind`, status at `ind+2`, data from `ind+3`.
/// Status `@` = busy.
///
/// Step size: FullTurnUm / (256 × MotorSteps)
/// Defaults: FullTurnUm=100 µm, MotorSteps=400 → 0.0009765625 µm/step
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Stage};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, FocusDirection, PropertyValue};

const DEFAULT_FULL_TURN_UM: f64 = 100.0;
const DEFAULT_MOTOR_STEPS: f64 = 400.0;
const DEFAULT_HC: i64 = 5;
const DEFAULT_RC: i64 = 25;
const DEFAULT_SLEW_VEL_UM: f64 = 40.0;
const DEFAULT_INIT_VEL_UM: f64 = 4.0;
const DEFAULT_ACCEL_UM: f64 = 1.0;

pub struct TofraZStage {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    ctrl: String,
    step_size_um: f64,
    position_um: f64,
}

impl TofraZStage {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            ctrl: "2".into(),
            step_size_um: DEFAULT_FULL_TURN_UM / (256.0 * DEFAULT_MOTOR_STEPS),
            position_um: 0.0,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t);
        self
    }

    fn call_transport<R, F>(&mut self, f: F) -> MmResult<R>
    where F: FnOnce(&mut dyn Transport) -> MmResult<R> {
        match self.transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let full = format!("/{}{}\r", self.ctrl, command);
        self.call_transport(|t| Ok(t.send_recv(&full)?.trim().to_string()))
    }

    /// Parse response: find `/0`, status at ind+2, data from ind+3.
    fn parse_pos(resp: &str) -> MmResult<i64> {
        let ind = resp.find("/0").ok_or_else(|| MmError::LocallyDefined(format!("bad response: {}", resp)))?;
        let data = resp.get(ind + 3..).unwrap_or("").trim();
        data.parse::<i64>().map_err(|_| MmError::LocallyDefined(format!("bad data: {}", resp)))
    }

    fn check_response(resp: &str) -> MmResult<()> {
        if resp.find("/0").is_some() {
            Ok(())
        } else {
            Err(MmError::LocallyDefined(format!("bad response: {}", resp)))
        }
    }
}

impl Default for TofraZStage {
    fn default() -> Self { Self::new() }
}

impl Device for TofraZStage {
    fn name(&self) -> &str { "TofraZStage" }
    fn description(&self) -> &str { "TOFRA Z-Drive with Integrated Controller" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let ss = DEFAULT_FULL_TURN_UM / (256.0 * DEFAULT_MOTOR_STEPS);
        self.step_size_um = ss;
        let slvel = (DEFAULT_SLEW_VEL_UM / ss).round() as i64;
        let invel = (DEFAULT_INIT_VEL_UM / ss).round() as i64;
        let accel = (DEFAULT_ACCEL_UM / ss).round() as i64;
        let init_cmd = format!("j256h{}m{}V{}v{}L{}n0R", DEFAULT_HC, DEFAULT_RC, slvel, invel, accel);
        let resp = self.cmd(&init_cmd)?;
        Self::check_response(&resp)?;
        let pos_resp = self.cmd("?0")?;
        let steps = Self::parse_pos(&pos_resp)?;
        self.position_um = steps as f64 * self.step_size_um;
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

impl Stage for TofraZStage {
    fn set_position_um(&mut self, pos: f64) -> MmResult<()> {
        let steps = (pos / self.step_size_um).round() as i64;
        let resp = self.cmd(&format!("A{}R", steps))?;
        Self::check_response(&resp)?;
        self.position_um = pos;
        Ok(())
    }

    fn get_position_um(&self) -> MmResult<f64> { Ok(self.position_um) }

    fn set_relative_position_um(&mut self, d: f64) -> MmResult<()> {
        if d == 0.0 { return Ok(()); }
        let steps = (d / self.step_size_um).round() as i64;
        let resp = if steps > 0 {
            self.cmd(&format!("P{}R", steps))?
        } else {
            self.cmd(&format!("D{}R", -steps))?
        };
        Self::check_response(&resp)?;
        self.position_um += d;
        Ok(())
    }

    fn home(&mut self) -> MmResult<()> {
        let resp = self.cmd("z0R")?;
        Self::check_response(&resp)?;
        self.position_um = 0.0;
        Ok(())
    }

    fn stop(&mut self) -> MmResult<()> {
        let resp = self.cmd("T")?;
        Self::check_response(&resp)?;
        Ok(())
    }

    fn get_limits(&self) -> MmResult<(f64, f64)> { Ok((0.0, 10000.0)) }
    fn get_focus_direction(&self) -> FocusDirection { FocusDirection::Unknown }
    fn is_continuous_focus_drive(&self) -> bool { false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

    fn init_cmd() -> String {
        // step_size = 100/(256*400) = 0.0009765625
        // slvel = round(40/0.0009765625) = 40960
        // invel = round(4/0.0009765625) = 4096
        // accel = round(1/0.0009765625) = 1024
        format!("/2j256h{}m{}V40960v4096L1024n0R\r", DEFAULT_HC, DEFAULT_RC)
    }

    fn make_init_transport() -> MockTransport {
        MockTransport::new()
            .expect(&init_cmd(), "/00")
            .expect("/2?0\r", "/000")
    }

    #[test]
    fn initialize() {
        let mut s = TofraZStage::new().with_transport(Box::new(make_init_transport()));
        s.initialize().unwrap();
        assert!((s.get_position_um().unwrap()).abs() < 1e-9);
    }

    #[test]
    fn move_absolute() {
        // 1.0 µm / 0.0009765625 µm/step = 1024 steps
        let t = make_init_transport().expect("/2A1024R\r", "/00");
        let mut s = TofraZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_position_um(1.0).unwrap();
        assert!((s.get_position_um().unwrap() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn move_relative_pos() {
        // 0.5 µm / 0.0009765625 = 512 steps
        let t = make_init_transport().expect("/2P512R\r", "/00");
        let mut s = TofraZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(0.5).unwrap();
        assert!((s.get_position_um().unwrap() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn move_relative_neg() {
        // -0.5 µm → D512R
        let t = make_init_transport().expect("/2D512R\r", "/00");
        let mut s = TofraZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.set_relative_position_um(-0.5).unwrap();
        assert!((s.get_position_um().unwrap() + 0.5).abs() < 1e-9);
    }

    #[test]
    fn home() {
        let t = make_init_transport().expect("/2z0R\r", "/00");
        let mut s = TofraZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.home().unwrap();
        assert!((s.get_position_um().unwrap()).abs() < 1e-9);
    }

    #[test]
    fn stop() {
        let t = make_init_transport().expect("/2T\r", "/00");
        let mut s = TofraZStage::new().with_transport(Box::new(t));
        s.initialize().unwrap();
        s.stop().unwrap();
    }

    #[test]
    fn no_transport_error() {
        assert!(TofraZStage::new().initialize().is_err());
    }
}
