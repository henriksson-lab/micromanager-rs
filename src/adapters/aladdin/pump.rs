/// WPI (World Precision Instruments) Aladdin syringe pump.
///
/// Protocol (TX `\r`, RX `\n`):
///   `PHN1\r`           → echo line   (select phase 1)
///   `PHN2\r`           → echo line   (select phase 2)
///   `RAT\r`            → response     (query current rate)
///   `FUN RAT<rate>UM\r`→ echo line    (set phase rate in µL/min)
///   `FUN STP\r`        → echo line   (set phase to stop)
///   `VOL<mL>\r`        → echo line   (set volume in mL; send µL ÷ 1000)
///   `VOL\r`            → response ending in "UL" or "ML"
///   `DIA<mm>\r`        → echo line   (set syringe diameter in mm)
///   `DIA\r`            → response
///   `RAT<rate>UM\r`    → echo line   (set rate in µL/min)
///   `RAT\r`            → response
///   `DIR INF\r`        → echo line   (direction: infuse)
///   `DIR WDR\r`        → echo line   (direction: withdraw)
///   `DIR\r`            → response
///   `RUN\r`            → echo line   (start pump)
///   `STP\r`            → echo line   (stop pump)
///
/// Default syringe: 4.699 mm diameter (1 mL BD syringe).
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, VolumetricPump};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::{Cell, RefCell};
use std::time::Instant;

pub struct AladdinPump {
    props: PropertyMap,
    transport: Option<RefCell<Box<dyn Transport>>>,
    initialized: bool,
    diameter_mm: Cell<f64>,
    volume_ul: Cell<f64>,
    rate_ul_per_min: Cell<f64>,
    infuse: Cell<bool>, // true = infuse, false = withdraw
    running: Cell<bool>,
    delay_ms: Cell<f64>,
    last_command_time: Cell<Option<Instant>>,
}

impl AladdinPump {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("Volume-uL", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property("SyringeDiameter", PropertyValue::Float(4.699), false)
            .unwrap();
        props
            .define_property("FlowRate-uL/min", PropertyValue::Float(0.0), false)
            .unwrap();
        props
            .define_property("Direction", PropertyValue::String("Infuse".into()), false)
            .unwrap();
        props
            .set_allowed_values("Direction", &["Infuse", "Withdraw"])
            .unwrap();
        props
            .define_property("Run", PropertyValue::Integer(0), false)
            .unwrap();
        props.set_allowed_values("Run", &["0", "1"]).unwrap();
        props
            .define_property("Delay_ms", PropertyValue::Float(0.0), false)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            diameter_mm: Cell::new(4.699),
            volume_ul: Cell::new(0.0),
            rate_ul_per_min: Cell::new(0.0),
            infuse: Cell::new(true),
            running: Cell::new(false),
            delay_ms: Cell::new(0.0),
            last_command_time: Cell::new(None),
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

    /// Send command and read back one echo line (ignore content).
    fn send_cmd(&self, command: &str) -> MmResult<()> {
        let c = format!("{}\r", command);
        self.call_transport(|t| {
            t.purge()?;
            t.send_recv(&c).map(|_| ())
        })?;
        self.last_command_time.set(Some(Instant::now()));
        Ok(())
    }

    /// Send command and return a fixed-width controller response.
    fn query_fixed(&self, command: &str, bytes: usize) -> MmResult<String> {
        let c = format!("{}\r", command);
        self.call_transport(|t| {
            t.purge()?;
            t.send(&c)?;
            let r = t.receive_bytes(bytes)?;
            Ok(String::from_utf8_lossy(&r).trim().to_string())
        })
    }

    fn parse_rate_ul_per_min(response: &str) -> MmResult<f64> {
        let value = response.get(4..9).ok_or(MmError::SerialInvalidResponse)?;
        let units = response.get(9..11).ok_or(MmError::SerialInvalidResponse)?;
        let rate = value
            .trim()
            .parse::<f64>()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        match units {
            "UM" => Ok(rate),
            "UH" => Ok(rate / 60.0),
            "MM" => Ok(rate * 1000.0),
            "MH" => Ok(rate * 1000.0 / 60.0),
            _ => Err(MmError::SerialInvalidResponse),
        }
    }

    fn parse_volume_ul(response: &str) -> MmResult<f64> {
        let value = response.get(4..9).ok_or(MmError::SerialInvalidResponse)?;
        let unit = response.get(9..10).ok_or(MmError::SerialInvalidResponse)?;
        let volume = value
            .trim()
            .parse::<f64>()
            .map_err(|_| MmError::SerialInvalidResponse)?;
        match unit {
            "U" => Ok(volume),
            "M" => Ok(volume * 1000.0),
            _ => Err(MmError::SerialInvalidResponse),
        }
    }

    fn parse_diameter_mm(response: &str) -> MmResult<f64> {
        response
            .get(4..9)
            .ok_or(MmError::SerialInvalidResponse)?
            .trim()
            .parse::<f64>()
            .map_err(|_| MmError::SerialInvalidResponse)
    }

    fn parse_direction(response: &str) -> MmResult<bool> {
        match response.get(4..7).ok_or(MmError::SerialInvalidResponse)? {
            "INF" => Ok(true),
            "WDR" => Ok(false),
            _ => Err(MmError::SerialInvalidResponse),
        }
    }

    fn parse_running(response: &str) -> MmResult<bool> {
        match response.get(3..4).ok_or(MmError::SerialInvalidResponse)? {
            "I" | "W" => Ok(true),
            _ => Ok(false),
        }
    }

    fn query_volume_ul(&self) -> MmResult<f64> {
        let volume = Self::parse_volume_ul(&self.query_fixed("VOL", 11)?)?;
        self.volume_ul.set(volume);
        Ok(volume)
    }

    fn query_diameter_mm(&self) -> MmResult<f64> {
        let diameter = Self::parse_diameter_mm(&self.query_fixed("DIA", 9)?)?;
        self.diameter_mm.set(diameter);
        Ok(diameter)
    }

    fn query_rate_ul_per_min(&self) -> MmResult<f64> {
        let rate = Self::parse_rate_ul_per_min(&self.query_fixed("RAT", 11)?)?;
        self.rate_ul_per_min.set(rate);
        Ok(rate)
    }

    fn query_direction_infuse(&self) -> MmResult<bool> {
        let infuse = Self::parse_direction(&self.query_fixed("DIR", 7)?)?;
        self.infuse.set(infuse);
        Ok(infuse)
    }

    fn query_running(&self) -> MmResult<bool> {
        let running = Self::parse_running(&self.query_fixed("", 4)?)?;
        self.running.set(running);
        Ok(running)
    }

    fn set_cached_rate_ul_per_min(&mut self, rate: f64) {
        self.rate_ul_per_min.set(rate);
        self.props
            .entry_mut("FlowRate-uL/min")
            .map(|e| e.value = PropertyValue::Float(rate));
    }

    fn set_cached_volume_ul(&mut self, volume: f64) {
        self.volume_ul.set(volume);
        self.props
            .entry_mut("Volume-uL")
            .map(|e| e.value = PropertyValue::Float(volume));
    }

    fn set_cached_diameter_mm(&mut self, diameter: f64) {
        self.diameter_mm.set(diameter);
        self.props
            .entry_mut("SyringeDiameter")
            .map(|e| e.value = PropertyValue::Float(diameter));
    }

    fn set_cached_running(&mut self, running: bool) {
        self.running.set(running);
        self.props
            .entry_mut("Run")
            .map(|e| e.value = PropertyValue::Integer(if running { 1 } else { 0 }));
    }

    fn setup_program(&mut self) -> MmResult<()> {
        let rate = self.query_rate_ul_per_min()?;
        self.set_cached_rate_ul_per_min(rate);
        self.send_cmd("PHN1")?;
        self.send_cmd(&format!("FUN RAT{:.4}UM", self.rate_ul_per_min.get()))?;
        self.send_cmd("PHN2")?;
        self.send_cmd("FUN STP")?;
        self.send_cmd("PHN1")?;
        Ok(())
    }
}

impl Default for AladdinPump {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for AladdinPump {
    fn name(&self) -> &str {
        "Aladdin"
    }
    fn description(&self) -> &str {
        "Aladdin Syringe Pump"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.setup_program()?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Volume-uL" if self.initialized => Ok(PropertyValue::Float(self.query_volume_ul()?)),
            "Volume-uL" => Ok(PropertyValue::Float(self.volume_ul.get())),
            "FlowRate-uL/min" if self.initialized => {
                Ok(PropertyValue::Float(self.query_rate_ul_per_min()?))
            }
            "FlowRate-uL/min" => Ok(PropertyValue::Float(self.rate_ul_per_min.get())),
            "Run" if self.initialized => Ok(PropertyValue::Integer(if self.query_running()? {
                1
            } else {
                0
            })),
            "Run" => Ok(PropertyValue::Integer(if self.running.get() {
                1
            } else {
                0
            })),
            "SyringeDiameter" if self.initialized => {
                Ok(PropertyValue::Float(self.query_diameter_mm()?))
            }
            "SyringeDiameter" => Ok(PropertyValue::Float(self.diameter_mm.get())),
            "Direction" if self.initialized => Ok(PropertyValue::String(
                if self.query_direction_infuse()? {
                    "Infuse"
                } else {
                    "Withdraw"
                }
                .into(),
            )),
            "Direction" => Ok(PropertyValue::String(
                if self.infuse.get() {
                    "Infuse"
                } else {
                    "Withdraw"
                }
                .into(),
            )),
            "Delay_ms" => Ok(PropertyValue::Float(self.delay_ms.get())),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Port" if self.initialized => Err(MmError::CanNotSetProperty),
            "Volume-uL" => {
                let volume = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_volume_ul(volume)
            }
            "FlowRate-uL/min" => {
                let rate = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    self.send_cmd(&format!("RAT{:.4}UM", rate))?;
                }
                self.set_cached_rate_ul_per_min(rate);
                Ok(())
            }
            "Run" => {
                let run = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                match run {
                    0 => self.stop(),
                    1 => self.start(),
                    _ => Err(MmError::InvalidPropertyValue),
                }
            }
            "SyringeDiameter" => {
                let d = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized {
                    self.send_cmd(&format!("DIA{:.4}", d))?;
                }
                self.set_cached_diameter_mm(d);
                Ok(())
            }
            "Direction" => {
                let s = val.as_str().to_string();
                if s != "Infuse" && s != "Withdraw" {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.infuse.set(s == "Infuse");
                if self.initialized {
                    let cmd = if self.infuse.get() {
                        "DIR INF"
                    } else {
                        "DIR WDR"
                    };
                    self.send_cmd(cmd)?;
                }
                self.props
                    .entry_mut("Direction")
                    .map(|e| e.value = PropertyValue::String(s));
                Ok(())
            }
            "Delay_ms" => {
                let delay = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if delay < 0.0 {
                    return Err(MmError::InvalidPropertyValue);
                }
                self.delay_ms.set(delay);
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
        if name == "Port" && self.initialized {
            return true;
        }
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType {
        DeviceType::Generic
    }
    fn busy(&self) -> bool {
        self.last_command_time
            .get()
            .map(|last| last.elapsed().as_secs_f64() * 1000.0 < self.delay_ms.get())
            .unwrap_or(false)
    }
}

impl VolumetricPump for AladdinPump {
    fn set_volume_ul(&mut self, volume: f64) -> MmResult<()> {
        let vol_ml = volume / 1000.0;
        if self.initialized {
            self.send_cmd(&format!("VOL{:.6}", vol_ml))?;
        }
        self.set_cached_volume_ul(volume);
        Ok(())
    }
    fn get_volume_ul(&self) -> MmResult<f64> {
        if self.initialized {
            self.query_volume_ul()
        } else {
            Ok(self.volume_ul.get())
        }
    }

    fn set_flow_rate(&mut self, rate_ul_per_s: f64) -> MmResult<()> {
        let rate_ul_per_min = rate_ul_per_s * 60.0;
        if self.initialized {
            self.send_cmd(&format!("RAT{:.4}UM", rate_ul_per_min))?;
        }
        self.set_cached_rate_ul_per_min(rate_ul_per_min);
        Ok(())
    }
    fn get_flow_rate(&self) -> MmResult<f64> {
        if self.initialized {
            Ok(self.query_rate_ul_per_min()? / 60.0)
        } else {
            Ok(self.rate_ul_per_min.get() / 60.0)
        }
    }

    fn start(&mut self) -> MmResult<()> {
        self.send_cmd("RUN")?;
        self.set_cached_running(true);
        Ok(())
    }
    fn stop(&mut self) -> MmResult<()> {
        self.send_cmd("STP")?;
        self.set_cached_running(false);
        Ok(())
    }
    fn is_running(&self) -> bool {
        self.running.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn make_init_transport() -> MockTransport {
        // RAT, PHN1, FUN RAT, PHN2, FUN STP, PHN1
        MockTransport::new()
            .expect_binary(b"00S 01.00UM")
            .expect("PHN1\r", "00 W")
            .expect("FUN RAT1.0000UM\r", "00 W")
            .expect("PHN2\r", "00 W")
            .expect("FUN STP\r", "00 W")
            .expect("PHN1\r", "00 W")
    }

    #[test]
    fn initialize() {
        let mut p = AladdinPump::new().with_transport(Box::new(make_init_transport()));
        p.initialize().unwrap();
        assert!(!p.is_running());
    }

    #[test]
    fn start_stop() {
        let t = make_init_transport()
            .expect("RUN\r", "00 W")
            .expect("STP\r", "00 W");
        let mut p = AladdinPump::new().with_transport(Box::new(t));
        p.initialize().unwrap();
        p.start().unwrap();
        assert!(p.is_running());
        p.stop().unwrap();
        assert!(!p.is_running());
    }

    #[test]
    fn set_volume() {
        let t = make_init_transport()
            .expect("VOL0.500000\r", "00 W")
            .expect_binary(b"00S 00.50ML");
        let mut p = AladdinPump::new().with_transport(Box::new(t));
        p.initialize().unwrap();
        p.set_volume_ul(500.0).unwrap();
        assert_eq!(p.get_volume_ul().unwrap(), 500.0);
    }

    #[test]
    fn set_flow_rate() {
        let t = make_init_transport()
            .expect("RAT120.0000UM\r", "00 W")
            .expect_binary(b"00S 120.0UM");
        let mut p = AladdinPump::new().with_transport(Box::new(t));
        p.initialize().unwrap();
        p.set_flow_rate(2.0).unwrap(); // 2 µL/s = 120 µL/min
        assert!((p.get_flow_rate().unwrap() - 2.0).abs() < 1e-9);
    }

    #[test]
    fn set_direction() {
        let t = make_init_transport().expect("DIR WDR\r", "00 W");
        let mut p = AladdinPump::new().with_transport(Box::new(t));
        p.initialize().unwrap();
        p.set_property("Direction", PropertyValue::String("Withdraw".into()))
            .unwrap();
        assert!(!p.infuse.get());
    }

    #[test]
    fn upstream_property_aliases() {
        let t = make_init_transport()
            .expect("VOL0.250000\r", "00 W")
            .expect("RAT30.0000UM\r", "00 W")
            .expect("RUN\r", "00 W")
            .expect_binary(b"00SI")
            .expect("STP\r", "00 W");
        let mut p = AladdinPump::new().with_transport(Box::new(t));
        p.initialize().unwrap();
        p.set_property("Volume-uL", PropertyValue::Float(250.0))
            .unwrap();
        p.set_property("FlowRate-uL/min", PropertyValue::Float(30.0))
            .unwrap();
        p.set_property("Run", PropertyValue::Integer(1)).unwrap();
        assert_eq!(p.get_property("Run").unwrap(), PropertyValue::Integer(1));
        p.set_property("Run", PropertyValue::Integer(0)).unwrap();
        assert_eq!(p.is_running(), false);
    }

    #[test]
    fn property_getters_query_device_like_before_get() {
        let t = make_init_transport()
            .expect_binary(b"00S 25.00UL")
            .expect_binary(b"00S 04.70")
            .expect_binary(b"00S 12.50UM")
            .expect_binary(b"00S WDR")
            .expect_binary(b"00SW");
        let mut p = AladdinPump::new().with_transport(Box::new(t));
        p.initialize().unwrap();

        assert_eq!(
            p.get_property("Volume-uL").unwrap(),
            PropertyValue::Float(25.0)
        );
        assert_eq!(
            p.get_property("SyringeDiameter").unwrap(),
            PropertyValue::Float(4.70)
        );
        assert_eq!(
            p.get_property("FlowRate-uL/min").unwrap(),
            PropertyValue::Float(12.5)
        );
        assert_eq!(
            p.get_property("Direction").unwrap(),
            PropertyValue::String("Withdraw".into())
        );
        assert_eq!(p.get_property("Run").unwrap(), PropertyValue::Integer(1));
    }

    #[test]
    fn parse_rate_units() {
        assert_eq!(
            AladdinPump::parse_rate_ul_per_min("00S 60.00UM").unwrap(),
            60.0
        );
        assert_eq!(
            AladdinPump::parse_rate_ul_per_min("00S 60.00UH").unwrap(),
            1.0
        );
        assert_eq!(
            AladdinPump::parse_rate_ul_per_min("00S 01.00MM").unwrap(),
            1000.0
        );
        assert_eq!(
            AladdinPump::parse_rate_ul_per_min("00S 60.00MH").unwrap(),
            1000.0
        );
    }

    #[test]
    fn parse_volume_units() {
        assert_eq!(AladdinPump::parse_volume_ul("00S 25.00UL").unwrap(), 25.0);
        assert_eq!(AladdinPump::parse_volume_ul("00S 01.25ML").unwrap(), 1250.0);
    }

    #[test]
    fn port_is_locked_after_initialize() {
        let t = make_init_transport();
        let mut p = AladdinPump::new().with_transport(Box::new(t));
        assert!(!p.is_property_read_only("Port"));
        p.initialize().unwrap();
        assert!(p.is_property_read_only("Port"));
        assert_eq!(
            p.set_property("Port", PropertyValue::String("COM2".into())),
            Err(MmError::CanNotSetProperty)
        );
    }

    #[test]
    fn delay_property_drives_busy_after_command() {
        let t = make_init_transport().expect("RUN\r", "00 W");
        let mut p = AladdinPump::new().with_transport(Box::new(t));
        p.set_property("Delay_ms", PropertyValue::Float(1000.0))
            .unwrap();
        p.initialize().unwrap();
        assert!(p.busy());
        p.start().unwrap();
        assert!(p.busy());
    }

    #[test]
    fn no_transport_error() {
        assert!(AladdinPump::new().initialize().is_err());
    }

    #[test]
    fn shutdown_only_clears_initialized_like_upstream() {
        let t = make_init_transport();
        let mut p = AladdinPump::new().with_transport(Box::new(t));
        p.initialize().unwrap();
        p.set_cached_running(true);
        p.shutdown().unwrap();
        assert!(p.is_running());
    }
}
