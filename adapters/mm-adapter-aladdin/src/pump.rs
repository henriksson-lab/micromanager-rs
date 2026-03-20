/// WPI (World Precision Instruments) Aladdin syringe pump.
///
/// Protocol (TX `\r`, RX `\n`):
///   `PHN1\r`           → echo line   (select phase 1)
///   `PHN2\r`           → echo line   (select phase 2)
///   `FUN RAT<rate>UM\r`→ echo line   (set phase rate in µL/min)
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
use mm_device::error::{MmError, MmResult};
use mm_device::property::PropertyMap;
use mm_device::traits::{Device, VolumetricPump};
use mm_device::transport::Transport;
use mm_device::types::{DeviceType, PropertyValue};

pub struct AladdinPump {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    diameter_mm: f64,
    volume_ul: f64,
    rate_ul_per_min: f64,
    infuse: bool,  // true = infuse, false = withdraw
    running: bool,
}

impl AladdinPump {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("SyringeDiameter_mm", PropertyValue::Float(4.699), false).unwrap();
        props.define_property("Direction", PropertyValue::String("Infuse".into()), false).unwrap();
        props.set_allowed_values("Direction", &["Infuse", "Withdraw"]).unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            diameter_mm: 4.699,
            volume_ul: 0.0,
            rate_ul_per_min: 1.0,
            infuse: true,
            running: false,
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

    /// Send command and read back one echo line (ignore content).
    fn send_cmd(&mut self, command: &str) -> MmResult<()> {
        let c = format!("{}\r", command);
        self.call_transport(|t| { t.send_recv(&c).map(|_| ()) })
    }

    /// Send command and return the response line.
    fn query(&mut self, command: &str) -> MmResult<String> {
        let c = format!("{}\r", command);
        self.call_transport(|t| { let r = t.send_recv(&c)?; Ok(r.trim().to_string()) })
    }

    fn setup_program(&mut self) -> MmResult<()> {
        self.send_cmd("PHN1")?;
        self.send_cmd(&format!("FUN RAT{:.4}UM", self.rate_ul_per_min))?;
        self.send_cmd("PHN2")?;
        self.send_cmd("FUN STP")?;
        self.send_cmd("PHN1")?;
        Ok(())
    }
}

impl Default for AladdinPump { fn default() -> Self { Self::new() } }

impl Device for AladdinPump {
    fn name(&self) -> &str { "AladdinPump" }
    fn description(&self) -> &str { "WPI Aladdin syringe pump" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        // Set diameter
        self.send_cmd(&format!("DIA{:.4}", self.diameter_mm))?;
        // Set direction
        let dir_cmd = if self.infuse { "DIR INF" } else { "DIR WDR" };
        self.send_cmd(dir_cmd)?;
        // Setup default 2-phase program
        self.setup_program()?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        if self.initialized {
            let _ = self.send_cmd("STP");
            self.running = false;
            self.initialized = false;
        }
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "SyringeDiameter_mm" => Ok(PropertyValue::Float(self.diameter_mm)),
            "Direction" => Ok(PropertyValue::String(if self.infuse { "Infuse" } else { "Withdraw" }.into())),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "SyringeDiameter_mm" => {
                let d = val.as_f64().ok_or(MmError::InvalidPropertyValue)?;
                if self.initialized { self.send_cmd(&format!("DIA{:.4}", d))?; }
                self.diameter_mm = d;
                self.props.entry_mut("SyringeDiameter_mm").map(|e| e.value = PropertyValue::Float(d));
                Ok(())
            }
            "Direction" => {
                let s = val.as_str().to_string();
                self.infuse = s == "Infuse";
                if self.initialized {
                    let cmd = if self.infuse { "DIR INF" } else { "DIR WDR" };
                    self.send_cmd(cmd)?;
                }
                self.props.entry_mut("Direction").map(|e| e.value = PropertyValue::String(s));
                Ok(())
            }
            _ => self.props.set(name, val),
        }
    }

    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::Generic }
    fn busy(&self) -> bool { self.running }
}

impl VolumetricPump for AladdinPump {
    fn set_volume_ul(&mut self, volume: f64) -> MmResult<()> {
        let vol_ml = volume / 1000.0;
        if self.initialized { self.send_cmd(&format!("VOL{:.6}", vol_ml))?; }
        self.volume_ul = volume;
        Ok(())
    }
    fn get_volume_ul(&self) -> MmResult<f64> { Ok(self.volume_ul) }

    fn set_flow_rate(&mut self, rate_ul_per_s: f64) -> MmResult<()> {
        let rate_ul_per_min = rate_ul_per_s * 60.0;
        if self.initialized {
            self.send_cmd(&format!("RAT{:.4}UM", rate_ul_per_min))?;
        }
        self.rate_ul_per_min = rate_ul_per_min;
        Ok(())
    }
    fn get_flow_rate(&self) -> MmResult<f64> { Ok(self.rate_ul_per_min / 60.0) }

    fn start(&mut self) -> MmResult<()> {
        self.send_cmd("RUN")?;
        self.running = true;
        Ok(())
    }
    fn stop(&mut self) -> MmResult<()> {
        self.send_cmd("STP")?;
        self.running = false;
        Ok(())
    }
    fn is_running(&self) -> bool { self.running }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_device::transport::MockTransport;

    fn make_init_transport() -> MockTransport {
        // DIA, DIR INF, PHN1, FUN RAT, PHN2, FUN STP, PHN1
        MockTransport::new()
            .any("00 W") // DIA
            .any("00 W") // DIR INF
            .any("00 W") // PHN1
            .any("00 W") // FUN RAT
            .any("00 W") // PHN2
            .any("00 W") // FUN STP
            .any("00 W") // PHN1
    }

    #[test]
    fn initialize() {
        let mut p = AladdinPump::new().with_transport(Box::new(make_init_transport()));
        p.initialize().unwrap();
        assert!(!p.is_running());
    }

    #[test]
    fn start_stop() {
        let t = make_init_transport().any("00 W").any("00 W"); // RUN, STP
        let mut p = AladdinPump::new().with_transport(Box::new(t));
        p.initialize().unwrap();
        p.start().unwrap();
        assert!(p.is_running());
        p.stop().unwrap();
        assert!(!p.is_running());
    }

    #[test]
    fn set_volume() {
        let t = make_init_transport().any("00 W"); // VOL command
        let mut p = AladdinPump::new().with_transport(Box::new(t));
        p.initialize().unwrap();
        p.set_volume_ul(500.0).unwrap();
        assert_eq!(p.get_volume_ul().unwrap(), 500.0);
    }

    #[test]
    fn set_flow_rate() {
        let t = make_init_transport().any("00 W"); // RAT command
        let mut p = AladdinPump::new().with_transport(Box::new(t));
        p.initialize().unwrap();
        p.set_flow_rate(2.0).unwrap(); // 2 µL/s = 120 µL/min
        assert!((p.get_flow_rate().unwrap() - 2.0).abs() < 1e-9);
    }

    #[test]
    fn set_direction() {
        let t = make_init_transport().any("00 W"); // DIR WDR
        let mut p = AladdinPump::new().with_transport(Box::new(t));
        p.initialize().unwrap();
        p.set_property("Direction", PropertyValue::String("Withdraw".into())).unwrap();
        assert!(!p.infuse);
    }

    #[test]
    fn no_transport_error() { assert!(AladdinPump::new().initialize().is_err()); }
}
