/// CARVII FRAP and intensity iris generic devices.
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Generic};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use std::cell::Cell;
use std::time::Duration;

use super::hub::SharedCarviiTransport;

const DEVICE_WAIT_MS: u64 = 20;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CarviiIrisKind {
    Frap,
    Intensity,
}

pub struct CarviiIris {
    props: PropertyMap,
    transport: Option<SharedCarviiTransport>,
    initialized: bool,
    kind: CarviiIrisKind,
    position: Cell<i64>,
}

impl CarviiIris {
    pub fn new(kind: CarviiIrisKind) -> Self {
        let mut props = PropertyMap::new();
        let (name, description) = identity(kind);
        props
            .define_property("Name", PropertyValue::String(name.into()), true)
            .unwrap();
        props
            .define_property(
                "Description",
                PropertyValue::String(description.into()),
                true,
            )
            .unwrap();
        props
            .define_property("Position", PropertyValue::Integer(1050), false)
            .unwrap();
        props
            .set_property_limits("Position", 450.0, 1050.0)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            kind,
            position: Cell::new(1050),
        }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(std::sync::Arc::new(std::sync::Mutex::new(t)));
        self
    }

    pub fn with_shared_transport(mut self, transport: SharedCarviiTransport) -> Self {
        self.transport = Some(transport);
        self
    }

    fn command_char(&self) -> char {
        match self.kind {
            CarviiIrisKind::Frap => 'I',
            CarviiIrisKind::Intensity => 'V',
        }
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        let transport = self.transport.as_ref().ok_or(MmError::NotConnected)?;
        let mut transport = transport
            .lock()
            .map_err(|_| MmError::LocallyDefined("CARVII transport lock poisoned".into()))?;
        f(transport.as_mut())
    }

    fn send_cmd_once(&self, command: &str) -> MmResult<()> {
        let full = format!("{command}\r");
        self.call_transport(|t| {
            t.purge()?;
            t.send(&full)
        })
    }

    fn send_cmd(&self, command: &str) -> MmResult<()> {
        let mut last_err = MmError::SerialCommandFailed;
        for _ in 0..10 {
            match self.send_cmd_once(command) {
                Ok(()) => {
                    std::thread::sleep(Duration::from_millis(DEVICE_WAIT_MS));
                    return Ok(());
                }
                Err(err) => last_err = err,
            }
            std::thread::sleep(Duration::from_millis(DEVICE_WAIT_MS));
        }
        Err(last_err)
    }

    fn query_position(&self) -> MmResult<i64> {
        if self.transport.is_none() {
            return Ok(self.position.get());
        }
        let cmd = format!("r{}\r", self.command_char());
        let resp = self.call_transport(|t| {
            t.purge()?;
            Ok(t.send_recv(&cmd)?.trim().to_string())
        })?;
        let pos = parse_iris_position(&resp, self.command_char())?;
        self.position.set(pos);
        Ok(pos)
    }

    fn set_position(&self, pos: i64) -> MmResult<()> {
        if !(450..=1050).contains(&pos) {
            return Err(MmError::InvalidPropertyValue);
        }
        let cmd = format!("{}{}", self.command_char(), pos);
        self.send_cmd(&cmd)?;
        self.position.set(pos);
        Ok(())
    }
}

impl Device for CarviiIris {
    fn name(&self) -> &str {
        identity(self.kind).0
    }

    fn description(&self) -> &str {
        identity(self.kind).1
    }

    fn initialize(&mut self) -> MmResult<()> {
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
            "Position" => Ok(PropertyValue::Integer(self.query_position()?)),
            _ => self.props.get(name).cloned(),
        }
    }

    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        match name {
            "Position" => {
                let pos = val.as_i64().ok_or(MmError::InvalidPropertyValue)?;
                self.set_position(pos)?;
                self.props.set(name, PropertyValue::Integer(pos))
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

impl Generic for CarviiIris {}

fn identity(kind: CarviiIrisKind) -> (&'static str, &'static str) {
    match kind {
        CarviiIrisKind::Frap => ("CARVII FRAP iris", "CARVII FRAP (field) iris"),
        CarviiIrisKind::Intensity => ("CARVII Intensity iris", "CARVII Intensity iris"),
    }
}

fn parse_iris_position(resp: &str, cmd_char: char) -> MmResult<i64> {
    let bytes = resp.as_bytes();
    if bytes.first() != Some(&b'r') || bytes.get(1) != Some(&(cmd_char as u8)) {
        return Err(MmError::SerialInvalidResponse);
    }
    let digits = resp.get(2..).ok_or(MmError::SerialInvalidResponse)?.trim();
    if digits.len() < 3 || !digits.as_bytes().iter().all(u8::is_ascii_digit) {
        return Err(MmError::SerialInvalidResponse);
    }
    digits
        .parse::<i64>()
        .map_err(|_| MmError::SerialInvalidResponse)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{MockTransport, Transport};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct RecordingTransport {
        sent: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingTransport {
        fn new() -> (Self, Arc<Mutex<Vec<String>>>) {
            let sent = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    sent: Arc::clone(&sent),
                },
                sent,
            )
        }
    }

    impl Transport for RecordingTransport {
        fn send(&mut self, cmd: &str) -> MmResult<()> {
            self.sent.lock().unwrap().push(cmd.to_string());
            Ok(())
        }

        fn receive_line(&mut self) -> MmResult<String> {
            Err(MmError::SerialTimeout)
        }

        fn purge(&mut self) -> MmResult<()> {
            Ok(())
        }
    }

    #[test]
    fn identity_matches_upstream() {
        let frap = CarviiIris::new(CarviiIrisKind::Frap);
        assert_eq!(frap.name(), "CARVII FRAP iris");
        assert_eq!(frap.description(), "CARVII FRAP (field) iris");
        assert_eq!(
            frap.get_property("Description").unwrap(),
            PropertyValue::String("CARVII FRAP (field) iris".into())
        );
        let intensity = CarviiIris::new(CarviiIrisKind::Intensity);
        assert_eq!(intensity.name(), "CARVII Intensity iris");
    }

    #[test]
    fn position_commands_match_upstream() {
        let (transport, sent) = RecordingTransport::new();
        let mut frap = CarviiIris::new(CarviiIrisKind::Frap).with_transport(Box::new(transport));
        frap.set_property("Position", PropertyValue::Integer(700))
            .unwrap();
        assert_eq!(*sent.lock().unwrap(), vec!["I700\r".to_string()]);
    }

    #[test]
    fn intensity_live_position_uses_rv() {
        let t = MockTransport::new()
            .expect("rV\r", "rV0450")
            .expect("rV\r", "rV1050");
        let mut intensity = CarviiIris::new(CarviiIrisKind::Intensity).with_transport(Box::new(t));
        intensity.initialize().unwrap();
        assert_eq!(
            intensity.get_property("Position").unwrap(),
            PropertyValue::Integer(1050)
        );
    }

    #[test]
    fn rejects_bad_position_and_bad_echo() {
        let mut frap = CarviiIris::new(CarviiIrisKind::Frap);
        assert_eq!(
            frap.set_property("Position", PropertyValue::Integer(449)),
            Err(MmError::InvalidPropertyValue)
        );

        let t = MockTransport::new().expect("rI\r", "rV1050");
        let mut with_transport = CarviiIris::new(CarviiIrisKind::Frap).with_transport(Box::new(t));
        assert_eq!(
            with_transport.initialize(),
            Err(MmError::SerialInvalidResponse)
        );
    }
}
