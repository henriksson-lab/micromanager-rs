/// ASI W-PTR (Wellplate Transfer Robot) device.
///
/// Protocol (ASCII, `\r\n` terminator):
///   `ORG\r\n`              → echoes `ORG`  (home robot arm)
///   `GET <stage>,<slot>\r\n` → echoes `GET`  (retrieve plate)
///   `PUT <stage>,<slot>\r\n` → echoes `PUT`  (place plate)
///   `AES\r\n`              → echoes `AES`  (emergency stop)
///   `DRT\r\n`              → echoes `DRT`  (drive reset)
///
/// The robot does not reply until the operation is complete (blocking).
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::Device;
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

pub struct AsiWPTR {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    stage: i64,
    slot: i64,
    command: String,
}

impl AsiWPTR {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Name", PropertyValue::String("WPTRobot".into()), true)
            .unwrap();
        props
            .define_property(
                "Description",
                PropertyValue::String("Wellplate Transfer Robot".into()),
                true,
            )
            .unwrap();
        props
            .define_pre_init_property("Port", PropertyValue::String("Undefined".into()))
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            stage: 1,
            slot: 1,
            command: "Undefined".into(),
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

    /// Send a command (with `\r\n` terminator) and read 3-char echo response.
    fn send_cmd(&mut self, command: &str) -> MmResult<String> {
        let full = format!("{}\r\n", command);
        self.call_transport(|t| t.send_recv(&full))
    }

    fn execute_command(&mut self, command: &str) -> MmResult<()> {
        let Some(prefix) = command.get(0..3) else {
            return Ok(());
        };
        let raw_cmd = match prefix {
            "ORG" => "ORG".to_string(),
            "GET" => format!("GET {},{}", self.stage, self.slot),
            "PUT" => format!("PUT {},{}", self.stage, self.slot),
            "AES" => "AES".to_string(),
            "DRT" => "DRT".to_string(),
            _ => return Ok(()),
        };

        let expected_echo = &raw_cmd[..3];
        let resp = self.send_cmd(&raw_cmd)?;
        if resp.len() != 3 || &resp[..3] != expected_echo {
            return Err(MmError::LocallyDefined(format!(
                "W-PTR unexpected response '{}' for command '{}'",
                resp, raw_cmd
            )));
        }
        Ok(())
    }
}

impl Default for AsiWPTR {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for AsiWPTR {
    fn name(&self) -> &str {
        "WPTRobot"
    }
    fn description(&self) -> &str {
        "Wellplate Transfer Robot"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        self.call_transport(|t| t.purge())?;
        if !self.props.has_property("Stage") {
            self.props
                .define_property("Stage", PropertyValue::Integer(1), false)
                .unwrap();
            self.props
                .define_property("Slot", PropertyValue::Integer(1), false)
                .unwrap();
            self.props
                .define_property("Command", PropertyValue::String("Undefined".into()), false)
                .unwrap();
        }
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        match name {
            "Stage" => Ok(PropertyValue::Integer(self.stage)),
            "Slot" => Ok(PropertyValue::Integer(self.slot)),
            "Command" => Ok(PropertyValue::String(self.command.clone())),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Stage" => {
                self.stage = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                Ok(())
            }
            "Slot" => {
                self.slot = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                Ok(())
            }
            "Command" => {
                let s = val.as_str().to_string();
                self.command = s.clone();
                self.props
                    .set("Command", PropertyValue::String(s.clone()))?;
                if self.initialized {
                    self.execute_command(&s)?;
                }
                Ok(())
            }
            "Port" => {
                if self.initialized {
                    return Err(MmError::InvalidProperty);
                }
                self.props.set(name, val)
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
        DeviceType::Generic
    }
    fn busy(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn initialize_no_transport() {
        assert!(AsiWPTR::new().initialize().is_err());
    }

    #[test]
    fn initialize_ok() {
        let t = MockTransport::new();
        let mut robot = AsiWPTR::new().with_transport(Box::new(t));
        robot.initialize().unwrap();
        assert_eq!(
            robot.get_property("Stage").unwrap(),
            PropertyValue::Integer(1)
        );
        assert_eq!(
            robot.get_property("Slot").unwrap(),
            PropertyValue::Integer(1)
        );
    }

    #[test]
    fn org_command() {
        let t = MockTransport::new().expect("ORG\r\n", "ORG");
        let mut robot = AsiWPTR::new().with_transport(Box::new(t));
        robot.initialize().unwrap();
        robot
            .set_property("Command", PropertyValue::String("ORG".into()))
            .unwrap();
    }

    #[test]
    fn get_command() {
        let t = MockTransport::new().expect("GET 2,5\r\n", "GET");
        let mut robot = AsiWPTR::new().with_transport(Box::new(t));
        robot.initialize().unwrap();
        robot
            .set_property("Stage", PropertyValue::Integer(2))
            .unwrap();
        robot
            .set_property("Slot", PropertyValue::Integer(5))
            .unwrap();
        robot
            .set_property("Command", PropertyValue::String("GET".into()))
            .unwrap();
    }

    #[test]
    fn put_command() {
        let t = MockTransport::new().expect("PUT 1,3\r\n", "PUT");
        let mut robot = AsiWPTR::new().with_transport(Box::new(t));
        robot.initialize().unwrap();
        robot
            .set_property("Stage", PropertyValue::Integer(1))
            .unwrap();
        robot
            .set_property("Slot", PropertyValue::Integer(3))
            .unwrap();
        robot
            .set_property("Command", PropertyValue::String("PUT".into()))
            .unwrap();
    }

    #[test]
    fn aes_emergency_stop() {
        let t = MockTransport::new().expect("AES\r\n", "AES");
        let mut robot = AsiWPTR::new().with_transport(Box::new(t));
        robot.initialize().unwrap();
        robot
            .set_property("Command", PropertyValue::String("AES".into()))
            .unwrap();
    }

    #[test]
    fn wrong_echo_is_error() {
        let t = MockTransport::new().expect("ORG\r\n", "ERR");
        let mut robot = AsiWPTR::new().with_transport(Box::new(t));
        robot.initialize().unwrap();
        let result = robot.set_property("Command", PropertyValue::String("ORG".into()));
        assert!(result.is_err());
    }

    #[test]
    fn padded_echo_is_error() {
        let t = MockTransport::new().expect("ORG\r\n", "ORG ");
        let mut robot = AsiWPTR::new().with_transport(Box::new(t));
        robot.initialize().unwrap();
        let result = robot.set_property("Command", PropertyValue::String("ORG".into()));
        assert!(result.is_err());
    }
}
