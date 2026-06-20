/// Yokogawa CSU-X1 filter wheel.
///
/// Two wheels available (1 or 2). Each has 6 positions (1-based on wire).
/// Protocol:
///   `FW_POS, <w>, <p>\r`  → `A`  (set)
///   `FW_POS, <w>, ?\r`    → `<p>\r` then `A` (query)
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

pub struct CsuXFilterWheel {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    wheel_nr: u32,       // 1 or 2
    position: Cell<u64>, // 0-based (MM)
    speed: i64,
    delay_ms: f64,
    changed_time: Cell<Instant>,
    move_busy_ms: Cell<u64>,
    num_positions: u64,
    labels: Vec<String>,
    gate_open: bool,
}

impl CsuXFilterWheel {
    pub fn new(wheel_nr: u32) -> Self {
        let num_positions: u64 = 6;
        let labels = (1..=num_positions)
            .map(|i| format!("Filter-{}", i))
            .collect();
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property(
                "WheelNumber",
                PropertyValue::Integer(wheel_nr as i64),
                false,
            )
            .unwrap();
        props
            .set_allowed_values("WheelNumber", &["1", "2"])
            .unwrap();
        props
            .define_property("State", PropertyValue::Integer(1), false)
            .unwrap();
        props
            .define_property("Label", PropertyValue::String("Filter-1".into()), false)
            .unwrap();
        props
            .define_property("Speed", PropertyValue::Integer(2), false)
            .unwrap();
        props
            .set_allowed_values("Speed", &["0", "1", "2", "3"])
            .unwrap();
        props
            .define_property("Delay", PropertyValue::Float(0.0), false)
            .unwrap();
        props.set_property_limits("Delay", 0.0, f64::MAX).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            wheel_nr,
            position: Cell::new(0),
            speed: 2,
            delay_ms: 0.0,
            changed_time: Cell::new(Instant::now()),
            move_busy_ms: Cell::new(0),
            num_positions,
            labels,
            gate_open: true,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(RefCell::new(t));
        self
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.as_ref() {
            Some(t) => f(t.borrow_mut().as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    /// Send a command and read response; strips trailing `\r` and whitespace.
    fn cmd(&self, command: &str) -> MmResult<String> {
        let full = format!("{}\r", command);
        self.call_transport(|t| {
            let r = t.send_recv(&full)?;
            Ok(r.trim().to_string())
        })
    }

    fn parse_position_response(resp: &str) -> u64 {
        resp.split(|c: char| c.is_whitespace() || c == '\r' || c == '\n')
            .filter(|s| !s.is_empty())
            .next()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(1u64)
            .saturating_sub(1)
    }

    fn query_position(&self) -> MmResult<u64> {
        let query = format!("FW_POS, {}, ?", self.wheel_nr);
        let resp = self.cmd(&query)?;
        if resp.trim_start().starts_with('N') {
            return Err(MmError::LocallyDefined(format!("CSU-X NAK: {}", resp)));
        }
        let pos = Self::parse_position_response(&resp);
        self.position.set(pos);
        Ok(pos)
    }

    fn query_speed(&self) -> MmResult<i64> {
        let query = format!("FW_SPEED, {}, ?", self.wheel_nr);
        let resp = self.cmd(&query)?;
        if resp.trim_start().starts_with('N') {
            return Err(MmError::LocallyDefined(format!("CSU-X NAK: {}", resp)));
        }
        resp.split(|c: char| c.is_whitespace() || c == '\r' || c == '\n' || c == 'A')
            .find(|s| !s.is_empty())
            .and_then(|s| s.parse::<i64>().ok())
            .ok_or(MmError::SerialInvalidResponse)
    }

    fn set_position_command(&self, pos: u64) -> MmResult<()> {
        let cmd = format!("FW_POS, {}, {}", self.wheel_nr, pos + 1);
        let mut last_err = None;
        for attempt in 0..10 {
            let resp = self.cmd(&cmd)?;
            if !resp.trim_start().starts_with('N') {
                return Ok(());
            }
            last_err = Some(resp);
            if attempt < 9 {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        Err(MmError::LocallyDefined(format!(
            "CSU-X NAK: {}",
            last_err.unwrap_or_default()
        )))
    }
}

impl Default for CsuXFilterWheel {
    fn default() -> Self {
        Self::new(1)
    }
}

impl Device for CsuXFilterWheel {
    fn name(&self) -> &str {
        "CSUX-Filter Wheel"
    }
    fn description(&self) -> &str {
        "Filter Wheel"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.query_position()?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "State" => Ok(PropertyValue::Integer(self.get_position()? as i64)),
            "Label" => Ok(PropertyValue::String(
                self.labels
                    .get(self.get_position()? as usize)
                    .cloned()
                    .unwrap_or_default(),
            )),
            "WheelNumber" => Ok(PropertyValue::Integer(self.wheel_nr as i64)),
            "Speed" => {
                if self.initialized && self.transport.is_some() {
                    Ok(PropertyValue::Integer(self.query_speed()?))
                } else {
                    Ok(PropertyValue::Integer(self.speed))
                }
            }
            "Delay" => Ok(PropertyValue::Float(self.delay_ms)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" if self.initialized => Err(MmError::InvalidPropertyValue),
            "State" => {
                let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if pos < 0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                let pos = pos as u64;
                self.set_position(pos)
            }
            "Label" => {
                let label = val.as_str().to_string();
                self.set_position_by_label(&label)
            }
            "WheelNumber" => {
                if self.initialized {
                    return Err(MmError::InvalidPropertyValue);
                }
                let wheel_nr = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if !(1..=2).contains(&wheel_nr) {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.wheel_nr = wheel_nr as u32;
                self.props.set(name, PropertyValue::Integer(wheel_nr))
            }
            "Speed" => {
                let speed = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if !(0..=3).contains(&speed) {
                    return Err(MmError::InvalidPropertyValue);
                }
                if self.initialized {
                    let resp = self.cmd(&format!("FW_SPEED, {}, {}", self.wheel_nr, speed))?;
                    if resp.trim_start().starts_with('N') {
                        return Err(MmError::LocallyDefined(format!("CSU-X NAK: {}", resp)));
                    }
                }
                self.speed = speed;
                self.props.set(name, PropertyValue::Integer(speed))
            }
            "Delay" => {
                let delay = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if delay < 0.0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.delay_ms = delay;
                self.props.set(name, PropertyValue::Float(delay))
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
        self.changed_time.get().elapsed().as_secs_f64() * 1000.0 < self.move_busy_ms.get() as f64
    }
}

impl StateDevice for CsuXFilterWheel {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::UnknownPosition);
        }
        if self.initialized {
            self.set_position_command(pos)?;
        }
        let previous = self.position.get();
        self.position.set(pos);
        self.changed_time.set(Instant::now());
        let positions_moved = previous.abs_diff(pos).min(3);
        self.move_busy_ms.set(positions_moved * 33);
        self.props
            .set("State", PropertyValue::Integer(pos as i64))?;
        self.props.set(
            "Label",
            PropertyValue::String(
                self.labels
                    .get(self.position.get() as usize)
                    .cloned()
                    .unwrap_or_default(),
            ),
        )?;
        Ok(())
    }

    fn get_position(&self) -> MmResult<u64> {
        if self.initialized && self.transport.is_some() {
            self.query_position()
        } else {
            Ok(self.position.get())
        }
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
        if pos == self.position.get() {
            self.props
                .set("Label", PropertyValue::String(label.to_string()))?;
        }
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

    #[test]
    fn initialize_reads_position() {
        let t = MockTransport::new()
            .expect("FW_POS, 1, ?\r", "3\rA")
            .expect("FW_POS, 1, ?\r", "3\rA");
        let mut fw = CsuXFilterWheel::new(1).with_transport(Box::new(t));
        fw.initialize().unwrap();
        assert_eq!(fw.get_position().unwrap(), 2); // 1-based 3 → 0-based 2
    }

    #[test]
    fn set_position_sends_1based() {
        let t = MockTransport::new()
            .expect("FW_POS, 1, ?\r", "1\rA")
            .expect("FW_POS, 1, 4\r", "A")
            .expect("FW_POS, 1, ?\r", "4\rA");
        let mut fw = CsuXFilterWheel::new(1).with_transport(Box::new(t));
        fw.initialize().unwrap();
        fw.set_position(3).unwrap();
        assert_eq!(fw.get_position().unwrap(), 3);
    }

    #[test]
    fn wheel2() {
        let t = MockTransport::new()
            .expect("FW_POS, 2, ?\r", "2\rA")
            .expect("FW_POS, 2, ?\r", "2\rA");
        let mut fw = CsuXFilterWheel::new(2).with_transport(Box::new(t));
        fw.initialize().unwrap();
        assert_eq!(fw.get_position().unwrap(), 1);
    }

    #[test]
    fn out_of_range_rejected() {
        let t = MockTransport::new().expect("FW_POS, 1, ?\r", "1\rA");
        let mut fw = CsuXFilterWheel::new(1).with_transport(Box::new(t));
        fw.initialize().unwrap();
        assert!(fw.set_position(6).is_err());
    }

    #[test]
    fn default_labels_are_one_based() {
        let fw = CsuXFilterWheel::new(1);
        assert!(fw.has_property("WheelNumber"));
        assert!(fw.has_property("State"));
        assert!(fw.has_property("Label"));
        assert!(fw.has_property("Speed"));
        assert_eq!(fw.get_position_label(0).unwrap(), "Filter-1");
        assert_eq!(fw.get_position_label(5).unwrap(), "Filter-6");
    }

    #[test]
    fn speed_property_sends_csux_command() {
        let t = MockTransport::new()
            .expect("FW_POS, 1, ?\r", "1\rA")
            .expect("FW_SPEED, 1, 3\r", "A")
            .expect("FW_SPEED, 1, ?\r", "3\rA");
        let mut fw = CsuXFilterWheel::new(1).with_transport(Box::new(t));
        fw.initialize().unwrap();
        fw.set_property("Speed", PropertyValue::Integer(3)).unwrap();
        assert_eq!(fw.get_property("Speed").unwrap(), PropertyValue::Integer(3));
    }

    #[test]
    fn speed_property_refreshes_live_after_initialize() {
        let t = MockTransport::new()
            .expect("FW_POS, 1, ?\r", "1\rA")
            .expect("FW_SPEED, 1, ?\r", "3\rA");
        let mut fw = CsuXFilterWheel::new(1).with_transport(Box::new(t));
        fw.initialize().unwrap();
        assert_eq!(fw.get_property("Speed").unwrap(), PropertyValue::Integer(3));
    }

    #[test]
    fn busy_uses_upstream_move_distance_timer() {
        let t = MockTransport::new()
            .expect("FW_POS, 1, ?\r", "1\rA")
            .expect("FW_POS, 1, 4\r", "A");
        let mut fw = CsuXFilterWheel::new(1).with_transport(Box::new(t));
        fw.initialize().unwrap();
        fw.set_property("Delay", PropertyValue::Float(0.0)).unwrap();
        fw.set_position(3).unwrap();
        assert!(fw.busy());
        std::thread::sleep(std::time::Duration::from_millis(110));
        assert!(!fw.busy());
    }

    #[test]
    fn position_set_retries_transient_nak_like_upstream_hub() {
        let t = MockTransport::new()
            .expect("FW_POS, 1, ?\r", "1\rA")
            .expect("FW_POS, 1, 2\r", "N")
            .expect("FW_POS, 1, 2\r", "A");
        let mut fw = CsuXFilterWheel::new(1).with_transport(Box::new(t));
        fw.initialize().unwrap();
        fw.set_position(1).unwrap();
        assert_eq!(fw.position.get(), 1);
    }

    #[test]
    fn port_is_locked_after_initialize() {
        let t = MockTransport::new().expect("FW_POS, 1, ?\r", "1\rA");
        let mut fw = CsuXFilterWheel::new(1).with_transport(Box::new(t));
        fw.initialize().unwrap();
        assert_eq!(
            fw.set_property("Port", PropertyValue::String("COM2".into())),
            Err(MmError::InvalidPropertyValue)
        );
    }
}
