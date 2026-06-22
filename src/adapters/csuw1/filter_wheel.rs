/// Yokogawa CSU-W1 filter wheel and dichroic selector.
///
/// Protocol (TX/RX `\r`):
///   `FW_POS,<wheel>,<pos>\r`   → `A`           set filter wheel position (1-based)
///   `FW_POS, <wheel>, ?\r`     → `<pos>\rA`    query position (1-based in response)
///   `DMM_POS,1, <pos>\r`       → `A`           set dichroic position (1-based)
///   `DMM_POS,1, ?\r`           → `<pos>\rA`    query dichroic position
///
/// Positions: 1-based in serial commands, 0-based internally.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::time::{Duration, Instant};

pub struct CsuFilterWheel {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    wheel: u8,
    speed: i64,
    num_positions: u64,
    position: u64,
    labels: Vec<String>,
    delay_ms: f64,
    last_move_time: Instant,
    positions_moved: u64,
}

impl CsuFilterWheel {
    pub fn new(wheel: u8, num_positions: u64) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property(
                "Name",
                PropertyValue::String("CSUW1-Filter Wheel".into()),
                true,
            )
            .unwrap();
        props
            .define_property(
                "Description",
                PropertyValue::String("CSUW1-Filter Wheel".into()),
                true,
            )
            .unwrap();
        props
            .define_property("WheelNumber", PropertyValue::Integer(wheel as i64), false)
            .unwrap();
        props
            .set_allowed_values("WheelNumber", &["1", "2"])
            .unwrap();
        props
            .define_property("State", PropertyValue::Integer(0), false)
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
        let labels = (0..num_positions)
            .map(|i| format!("Filter-{}", i + 1))
            .collect();
        Self {
            props,
            transport: None,
            initialized: false,
            wheel,
            speed: 2,
            num_positions,
            position: 0,
            labels,
            delay_ms: 0.0,
            last_move_time: Instant::now(),
            positions_moved: 0,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t);
        self
    }

    fn call_transport<R, F>(&mut self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let full = format!("{}\r", command);
        self.call_transport(|t| {
            let r = t.send_recv(&full)?;
            Ok(r.trim().to_string())
        })
    }

    fn parse_pos_response(resp: &str, num_positions: u64) -> MmResult<u64> {
        // Response: "<pos>\rA" or "<pos>A" — take first token
        let pos_1based = resp
            .split(|c: char| c.is_whitespace() || c == '\r' || c == '\n' || c == 'A')
            .find(|s| !s.is_empty())
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or(MmError::SerialInvalidResponse)?;
        if pos_1based == 0 || pos_1based > num_positions {
            return Err(MmError::UnknownPosition);
        }
        Ok(pos_1based - 1)
    }

    fn require_ack(resp: &str, context: &str) -> MmResult<()> {
        if resp.ends_with('N') {
            return Err(MmError::LocallyDefined(format!(
                "{} NAK: {}",
                context, resp
            )));
        }
        if !resp.ends_with('A') {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(())
    }

    fn cmd_with_ack_retry(
        &mut self,
        command: &str,
        context: &str,
        max_attempts: usize,
        retry_delay: Duration,
    ) -> MmResult<()> {
        let mut last_err = MmError::SerialInvalidResponse;
        for attempt in 0..max_attempts {
            match self
                .cmd(command)
                .and_then(|resp| Self::require_ack(&resp, context))
            {
                Ok(()) => return Ok(()),
                Err(err) => {
                    last_err = err;
                    if attempt + 1 < max_attempts {
                        std::thread::sleep(retry_delay);
                    }
                }
            }
        }
        Err(last_err)
    }
}

impl Default for CsuFilterWheel {
    fn default() -> Self {
        Self::new(1, 10)
    }
}

impl Device for CsuFilterWheel {
    fn name(&self) -> &str {
        "CSUW1-Filter Wheel"
    }
    fn description(&self) -> &str {
        "CSUW1-Filter Wheel"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let q = format!("FW_POS, {}, ?", self.wheel);
        let resp = self.cmd(&q)?;
        self.position = Self::parse_pos_response(&resp, self.num_positions)?;
        if let Ok(resp) = self.cmd(&format!("FW_SPEED, {}, ?", self.wheel)) {
            self.speed = resp
                .split(|c: char| c.is_whitespace() || c == '\r' || c == '\n' || c == 'A')
                .find(|s| !s.is_empty())
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(self.speed);
        }
        self.props
            .set("State", PropertyValue::Integer(self.position as i64))?;
        self.props.set(
            "Label",
            PropertyValue::String(
                self.labels
                    .get(self.position as usize)
                    .cloned()
                    .unwrap_or_default(),
            ),
        )?;
        self.props
            .set("Speed", PropertyValue::Integer(self.speed))?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "State" => Ok(PropertyValue::Integer(self.position as i64)),
            "Label" => Ok(PropertyValue::String(
                self.labels
                    .get(self.position as usize)
                    .cloned()
                    .unwrap_or_default(),
            )),
            "WheelNumber" => Ok(PropertyValue::Integer(self.wheel as i64)),
            "Speed" => Ok(PropertyValue::Integer(self.speed)),
            "Delay" => Ok(PropertyValue::Float(self.delay_ms)),
            _ => self.props.get(name).cloned(),
        }
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "State" => {
                let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if pos < 0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.set_position(pos as u64)
            }
            "Label" => {
                let label = val.as_str().to_string();
                self.set_position_by_label(&label)
            }
            "WheelNumber" => {
                let wheel = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if !(1..=2).contains(&wheel) {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.wheel = wheel as u8;
                self.props
                    .set("WheelNumber", PropertyValue::Integer(self.wheel as i64))
            }
            "Speed" => {
                let speed = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if !(0..=3).contains(&speed) {
                    return Err(MmError::InvalidPropertyValue);
                }
                if self.initialized {
                    let resp = self.cmd(&format!("FW_SPEED, {}, {}", self.wheel, speed))?;
                    Self::require_ack(&resp, "CSU FW")?;
                }
                self.speed = speed;
                self.props.set("Speed", PropertyValue::Integer(speed))
            }
            "Delay" => {
                let delay = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if delay < 0.0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.delay_ms = delay;
                self.props.set("Delay", PropertyValue::Float(delay))
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
        let ms_per_position = match self.speed {
            3 => 40.0,
            2 => 66.0,
            1 => 100.0,
            _ => 400.0,
        };
        let wait_ms = self.positions_moved as f64 * ms_per_position + self.delay_ms;
        self.last_move_time.elapsed().as_secs_f64() * 1000.0 < wait_ms
    }
}

impl StateDevice for CsuFilterWheel {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::LocallyDefined(format!(
                "Position {} out of range",
                pos
            )));
        }
        let cmd = format!("FW_POS,{},{}", self.wheel, pos + 1); // 1-based
        self.cmd_with_ack_retry(&cmd, "CSU FW", 20, Duration::from_millis(50))?;
        let direct = self.position.abs_diff(pos);
        self.positions_moved = direct.min(self.num_positions.saturating_sub(direct));
        self.last_move_time = Instant::now();
        self.position = pos;
        self.props
            .set("State", PropertyValue::Integer(self.position as i64))?;
        self.props.set(
            "Label",
            PropertyValue::String(
                self.labels
                    .get(self.position as usize)
                    .cloned()
                    .unwrap_or_default(),
            ),
        )?;
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
            .ok_or_else(|| MmError::LocallyDefined(format!("Position {} out of range", pos)))
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
            return Err(MmError::LocallyDefined(format!(
                "Position {} out of range",
                pos
            )));
        }
        self.labels[pos as usize] = label.to_string();
        if pos == self.position {
            self.props
                .set("Label", PropertyValue::String(label.to_string()))?;
        }
        Ok(())
    }

    fn set_gate_open(&mut self, _open: bool) -> MmResult<()> {
        Ok(())
    }
    fn get_gate_open(&self) -> MmResult<bool> {
        Ok(true)
    }
}

/// CSU-W1 dichroic mirror selector.
pub struct CsuDichroic {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    num_positions: u64,
    position: u64,
    labels: Vec<String>,
}

impl CsuDichroic {
    pub fn new(num_positions: u64) -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("State", PropertyValue::Integer(0), false)
            .unwrap();
        props.set_allowed_values("State", &["0", "1", "2"]).unwrap();
        props
            .define_property("Label", PropertyValue::String("Dichroic-1".into()), false)
            .unwrap();
        let labels = (0..num_positions)
            .map(|i| format!("Dichroic-{}", i + 1))
            .collect();
        Self {
            props,
            transport: None,
            initialized: false,
            num_positions,
            position: 0,
            labels,
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t);
        self
    }

    fn call_transport<R, F>(&mut self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        match self.transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    fn cmd(&mut self, command: &str) -> MmResult<String> {
        let full = format!("{}\r", command);
        self.call_transport(|t| {
            let r = t.send_recv(&full)?;
            Ok(r.trim().to_string())
        })
    }

    fn cmd_with_ack_retry(&mut self, command: &str, context: &str) -> MmResult<()> {
        let mut last_err = MmError::SerialInvalidResponse;
        for attempt in 0..10 {
            match self
                .cmd(command)
                .and_then(|resp| CsuFilterWheel::require_ack(&resp, context))
            {
                Ok(()) => return Ok(()),
                Err(err) => {
                    last_err = err;
                    if attempt < 9 {
                        std::thread::sleep(Duration::from_millis(200));
                    }
                }
            }
        }
        Err(last_err)
    }
}

impl Default for CsuDichroic {
    fn default() -> Self {
        Self::new(3)
    }
}

impl Device for CsuDichroic {
    fn name(&self) -> &str {
        "CSUW1-Dichroic Mirror"
    }
    fn description(&self) -> &str {
        "CSUW1 Dichroics"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let resp = self.cmd("DMM_POS,1, ?")?;
        let pos_1based = resp
            .split(|c: char| c.is_whitespace() || c == '\r' || c == '\n' || c == 'A')
            .find(|s| !s.is_empty())
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or(MmError::SerialInvalidResponse)?;
        if pos_1based == 0 || pos_1based > self.num_positions {
            return Err(MmError::UnknownPosition);
        }
        self.position = pos_1based - 1;
        self.props
            .set("State", PropertyValue::Integer(self.position as i64))?;
        self.props.set(
            "Label",
            PropertyValue::String(
                self.labels
                    .get(self.position as usize)
                    .cloned()
                    .unwrap_or_default(),
            ),
        )?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
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
            "State" => {
                let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                if pos < 0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.set_position(pos as u64)
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
        false
    }
}

impl StateDevice for CsuDichroic {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::LocallyDefined(format!(
                "Position {} out of range",
                pos
            )));
        }
        let cmd = format!("DMM_POS,1, {}", pos + 1);
        self.cmd_with_ack_retry(&cmd, "CSU dichroic")?;
        self.position = pos;
        self.props
            .set("State", PropertyValue::Integer(self.position as i64))?;
        self.props.set(
            "Label",
            PropertyValue::String(
                self.labels
                    .get(self.position as usize)
                    .cloned()
                    .unwrap_or_default(),
            ),
        )?;
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
            .ok_or_else(|| MmError::LocallyDefined(format!("Position {} out of range", pos)))
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
            return Err(MmError::LocallyDefined(format!(
                "Position {} out of range",
                pos
            )));
        }
        self.labels[pos as usize] = label.to_string();
        if pos == self.position {
            self.props
                .set("Label", PropertyValue::String(label.to_string()))?;
        }
        Ok(())
    }

    fn set_gate_open(&mut self, _open: bool) -> MmResult<()> {
        Ok(())
    }
    fn get_gate_open(&self) -> MmResult<bool> {
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn filter_wheel_init() {
        let t = MockTransport::new()
            .expect("FW_POS, 1, ?\r", "2\rA")
            .expect("FW_SPEED, 1, ?\r", "2\rA");
        let mut w = CsuFilterWheel::new(1, 6).with_transport(Box::new(t));
        w.initialize().unwrap();
        assert_eq!(w.get_position().unwrap(), 1); // '2' → 0-based 1
    }

    #[test]
    fn filter_wheel_set() {
        let t = MockTransport::new()
            .expect("FW_POS, 1, ?\r", "1\rA")
            .expect("FW_SPEED, 1, ?\r", "2\rA")
            .expect("FW_POS,1,3\r", "A");
        let mut w = CsuFilterWheel::new(1, 6).with_transport(Box::new(t));
        w.initialize().unwrap();
        w.set_position(2).unwrap(); // 0-based 2 → sends 3
        assert_eq!(w.get_position().unwrap(), 2);
    }

    #[test]
    fn filter_wheel_busy_uses_move_distance_speed_and_delay() {
        let t = MockTransport::new()
            .expect("FW_POS, 1, ?\r", "1\rA")
            .expect("FW_SPEED, 1, ?\r", "2\rA")
            .expect("FW_POS,1,4\r", "A");
        let mut w = CsuFilterWheel::new(1, 10).with_transport(Box::new(t));
        w.initialize().unwrap();
        assert!(!w.busy());

        w.set_property("Delay", PropertyValue::Float(1000.0))
            .unwrap();
        w.set_position(3).unwrap();

        assert!(w.busy());
    }

    #[test]
    fn wheel_number_can_change_after_initialize_like_upstream() {
        let t = MockTransport::new()
            .expect("FW_POS, 1, ?\r", "1\rA")
            .expect("FW_SPEED, 1, ?\r", "2\rA");
        let mut w = CsuFilterWheel::new(1, 6).with_transport(Box::new(t));
        w.initialize().unwrap();

        w.set_property("WheelNumber", PropertyValue::Integer(2))
            .unwrap();

        assert_eq!(w.get_property("WheelNumber").unwrap().as_i64(), Some(2));
    }

    #[test]
    fn dichroic_init_and_set() {
        let t = MockTransport::new()
            .expect("DMM_POS,1, ?\r", "1\rA")
            .expect("DMM_POS,1, 2\r", "A");
        let mut d = CsuDichroic::new(3).with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(d.get_position().unwrap(), 0);
        d.set_position(1).unwrap();
        assert_eq!(d.get_position().unwrap(), 1);
    }

    #[test]
    fn no_transport_error() {
        assert!(CsuFilterWheel::new(1, 6).initialize().is_err());
        assert!(CsuDichroic::new(3).initialize().is_err());
    }

    #[test]
    fn default_filter_wheel_has_ten_positions_like_upstream() {
        assert_eq!(CsuFilterWheel::default().get_number_of_positions(), 10);
    }

    #[test]
    fn filter_wheel_rejects_invalid_query_position() {
        let t = MockTransport::new().expect("FW_POS, 1, ?\r", "0\rA");
        let mut w = CsuFilterWheel::new(1, 6).with_transport(Box::new(t));
        assert_eq!(w.initialize(), Err(MmError::UnknownPosition));
    }

    #[test]
    fn dichroic_rejects_invalid_query_position() {
        let t = MockTransport::new().expect("DMM_POS,1, ?\r", "4\rA");
        let mut d = CsuDichroic::new(3).with_transport(Box::new(t));
        assert_eq!(d.initialize(), Err(MmError::UnknownPosition));
    }

    #[test]
    fn setters_require_acknowledgment_like_upstream() {
        let t = MockTransport::new()
            .expect("FW_POS, 1, ?\r", "1\rA")
            .expect("FW_SPEED, 1, ?\r", "2\rA")
            .expect("FW_POS,1,2\r", "OK")
            .expect("FW_POS,1,2\r", "OK")
            .expect("FW_POS,1,2\r", "OK")
            .expect("FW_POS,1,2\r", "OK")
            .expect("FW_POS,1,2\r", "OK")
            .expect("FW_POS,1,2\r", "OK")
            .expect("FW_POS,1,2\r", "OK")
            .expect("FW_POS,1,2\r", "OK")
            .expect("FW_POS,1,2\r", "OK")
            .expect("FW_POS,1,2\r", "OK")
            .expect("FW_POS,1,2\r", "OK")
            .expect("FW_POS,1,2\r", "OK")
            .expect("FW_POS,1,2\r", "OK")
            .expect("FW_POS,1,2\r", "OK")
            .expect("FW_POS,1,2\r", "OK")
            .expect("FW_POS,1,2\r", "OK")
            .expect("FW_POS,1,2\r", "OK")
            .expect("FW_POS,1,2\r", "OK")
            .expect("FW_POS,1,2\r", "OK")
            .expect("FW_POS,1,2\r", "OK");
        let mut w = CsuFilterWheel::new(1, 6).with_transport(Box::new(t));
        w.initialize().unwrap();
        assert_eq!(w.set_position(1), Err(MmError::SerialInvalidResponse));

        let t = MockTransport::new()
            .expect("FW_POS, 1, ?\r", "1\rA")
            .expect("FW_SPEED, 1, ?\r", "2\rA")
            .expect("FW_SPEED, 1, 3\r", "OK");
        let mut w = CsuFilterWheel::new(1, 6).with_transport(Box::new(t));
        w.initialize().unwrap();
        assert_eq!(
            w.set_property("Speed", PropertyValue::Integer(3)),
            Err(MmError::SerialInvalidResponse)
        );

        let t = MockTransport::new()
            .expect("DMM_POS,1, ?\r", "1\rA")
            .expect("DMM_POS,1, 2\r", "OK")
            .expect("DMM_POS,1, 2\r", "OK")
            .expect("DMM_POS,1, 2\r", "OK")
            .expect("DMM_POS,1, 2\r", "OK")
            .expect("DMM_POS,1, 2\r", "OK")
            .expect("DMM_POS,1, 2\r", "OK")
            .expect("DMM_POS,1, 2\r", "OK")
            .expect("DMM_POS,1, 2\r", "OK")
            .expect("DMM_POS,1, 2\r", "OK")
            .expect("DMM_POS,1, 2\r", "OK");
        let mut d = CsuDichroic::new(3).with_transport(Box::new(t));
        d.initialize().unwrap();
        assert_eq!(d.set_position(1), Err(MmError::SerialInvalidResponse));
    }

    #[test]
    fn position_setters_retry_transient_nak_like_upstream() {
        let t = MockTransport::new()
            .expect("FW_POS, 1, ?\r", "1\rA")
            .expect("FW_SPEED, 1, ?\r", "2\rA")
            .expect("FW_POS,1,2\r", "N")
            .expect("FW_POS,1,2\r", "A");
        let mut w = CsuFilterWheel::new(1, 6).with_transport(Box::new(t));
        w.initialize().unwrap();
        w.set_position(1).unwrap();
        assert_eq!(w.get_position().unwrap(), 1);

        let t = MockTransport::new()
            .expect("DMM_POS,1, ?\r", "1\rA")
            .expect("DMM_POS,1, 2\r", "N")
            .expect("DMM_POS,1, 2\r", "A");
        let mut d = CsuDichroic::new(3).with_transport(Box::new(t));
        d.initialize().unwrap();
        d.set_position(1).unwrap();
        assert_eq!(d.get_position().unwrap(), 1);
    }
}
