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
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, StateDevice};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

const TURN_MSTEPS: i64 = 3200;
const DEFAULT_NUM_POS: u64 = 10;
const DEFAULT_HC: i64 = 5;
const DEFAULT_RC: i64 = 60;
const DEFAULT_SV: i64 = 5000;
const DEFAULT_IV: i64 = 500;
const DEFAULT_ACC: i64 = 10;

pub struct TofraFilterWheel {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    ctrl: String,
    num_positions: u64,
    position: u64,
    labels: Vec<String>,
    gate_open: bool,
}

impl TofraFilterWheel {
    pub fn new() -> Self {
        let num_positions = DEFAULT_NUM_POS;
        let labels = (0..num_positions).map(|i| format!("Filter-{:02}", i + 1)).collect();
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            ctrl: "1".into(),
            num_positions,
            position: 0,
            labels,
            gate_open: true,
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

    fn check_response(resp: &str) -> MmResult<()> {
        if resp.find("/0").is_some() {
            Ok(())
        } else {
            Err(MmError::LocallyDefined(format!("bad response: {}", resp)))
        }
    }

    fn msteps_for_pos(num_pos: u64, i: u64) -> i64 {
        (TURN_MSTEPS as f64 / num_pos as f64 * i as f64 + 0.5).floor() as i64
    }
}

impl Default for TofraFilterWheel {
    fn default() -> Self { Self::new() }
}

impl Device for TofraFilterWheel {
    fn name(&self) -> &str { "TofraFilterWheel" }
    fn description(&self) -> &str { "TOFRA Filter Wheel with Integrated Controller" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let init_cmd = format!(
            "j16h{}m{}V{}v{}L{}f0n0gD10S13G0D1gD1S03G0R",
            DEFAULT_HC, DEFAULT_RC, DEFAULT_SV, DEFAULT_IV, DEFAULT_ACC
        );
        let resp = self.cmd(&init_cmd)?;
        Self::check_response(&resp)?;
        self.position = 0;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "State" => Ok(PropertyValue::Integer(self.position as i64)),
            "Label" => Ok(PropertyValue::String(
                self.labels.get(self.position as usize).cloned().unwrap_or_default()
            )),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
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

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::State }
    fn busy(&self) -> bool { false }
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
            let d2 = if d1 > 0 { d1 - TURN_MSTEPS } else { TURN_MSTEPS + d1 };
            let d = if d1.abs() > d2.abs() { d2 } else { d1 };
            let move_cmd = if d > 0 {
                format!("P{}R", d)
            } else {
                format!("D{}R", -d)
            };
            let resp = self.cmd(&move_cmd)?;
            Self::check_response(&resp)?;
        }
        self.position = pos;
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> { Ok(self.position) }
    fn get_number_of_positions(&self) -> u64 { self.num_positions }

    fn get_position_label(&self, pos: u64) -> MmResult<String> {
        self.labels.get(pos as usize).cloned().ok_or(MmError::UnknownPosition)
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

    fn set_gate_open(&mut self, open: bool) -> MmResult<()> { self.gate_open = open; Ok(()) }
    fn get_gate_open(&self) -> MmResult<bool> { Ok(self.gate_open) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

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
}
