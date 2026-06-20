//! MicroFPGA Hub — binary serial protocol, read/write 32-bit registers.

use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, Hub};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};
use parking_lot::Mutex;

use super::{
    read_register, write_register, ADDR_ID, ADDR_VERSION, FIRMWARE_VERSION, ID_AU, ID_AUP, ID_CU,
    ID_MOJO, OFFSET_CAM_SYNC_MODE,
};

pub struct MicroFpgaHub {
    props: PropertyMap,
    transport: Mutex<Option<Box<dyn Transport>>>,
    initialized: bool,
    pub version: u32,
    pub id: u32,
}

impl MicroFpgaHub {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("MicroFPGA version", PropertyValue::Integer(0), true)
            .unwrap();
        props
            .define_property(
                "MicroFPGA ID",
                PropertyValue::String("Unknown".into()),
                true,
            )
            .unwrap();
        props
            .define_property(
                "Camera trigger",
                PropertyValue::String("Passive".into()),
                true,
            )
            .unwrap();

        Self {
            props,
            transport: Mutex::new(None),
            initialized: false,
            version: 0,
            id: 0,
        }
    }

    pub fn with_transport(self, t: Box<dyn Transport>) -> Self {
        *self.transport.lock() = Some(t);
        self
    }

    fn call_transport<R, F>(&self, f: F) -> MmResult<R>
    where
        F: FnOnce(&mut dyn Transport) -> MmResult<R>,
    {
        let mut transport = self.transport.lock();
        match transport.as_mut() {
            Some(t) => f(t.as_mut()),
            None => Err(MmError::NotConnected),
        }
    }

    /// Send a 5-byte read request [0x00, addr_le_u32].
    pub fn send_read_request(&mut self, addr: u32) -> MmResult<()> {
        let bytes = [
            0x00u8,
            (addr & 0xFF) as u8,
            ((addr >> 8) & 0xFF) as u8,
            ((addr >> 16) & 0xFF) as u8,
            ((addr >> 24) & 0xFF) as u8,
        ];
        self.call_transport(|t| t.send_bytes(&bytes))
    }

    /// Read a 4-byte response and decode little-endian u32.
    pub fn read_answer(&mut self) -> MmResult<u32> {
        let raw = self.call_transport(|t| t.receive_bytes(4))?;
        if raw.len() < 4 {
            return Err(MmError::SerialInvalidResponse);
        }
        Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
    }

    /// Send a 9-byte write request [0x80, addr_le_u32, value_le_u32].
    pub fn send_write_request(&mut self, addr: u32, value: u32) -> MmResult<()> {
        let bytes = [
            0x80u8,
            (addr & 0xFF) as u8,
            ((addr >> 8) & 0xFF) as u8,
            ((addr >> 16) & 0xFF) as u8,
            ((addr >> 24) & 0xFF) as u8,
            (value & 0xFF) as u8,
            ((value >> 8) & 0xFF) as u8,
            ((value >> 16) & 0xFF) as u8,
            ((value >> 24) & 0xFF) as u8,
        ];
        self.call_transport(|t| t.send_bytes(&bytes))
    }

    /// Convenience: read a register.
    pub fn read_register(&mut self, addr: u32) -> MmResult<u32> {
        self.call_transport(|t| read_register(t, addr))
    }

    /// Convenience: write a register (no read-back).
    pub fn write_register(&mut self, addr: u32, value: u32) -> MmResult<()> {
        self.call_transport(|t| write_register(t, addr, value))
    }
}

impl Default for MicroFpgaHub {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for MicroFpgaHub {
    fn name(&self) -> &str {
        "MicroFPGA-Hub"
    }
    fn description(&self) -> &str {
        "MicroFPGA Hub (required)"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.lock().is_none() {
            return Err(MmError::NotConnected);
        }

        // Read version register
        self.version = self.read_register(ADDR_VERSION)?;
        if self.version != FIRMWARE_VERSION {
            return Err(MmError::LocallyDefined(format!(
                "Firmware version mismatch: expected {}, got {}",
                FIRMWARE_VERSION, self.version
            )));
        }

        // Read ID register
        self.id = self.read_register(ADDR_ID)?;
        let id_str = match self.id {
            ID_AU => "Au",
            ID_AUP => "Au+",
            ID_CU => "Mojo",
            ID_MOJO => "Cu",
            _ => {
                return Err(MmError::LocallyDefined(format!(
                    "Unknown board ID: {}",
                    self.id
                )));
            }
        };

        self.props
            .entry_mut("MicroFPGA version")
            .map(|e| e.value = PropertyValue::Integer(self.version as i64));
        self.props
            .entry_mut("MicroFPGA ID")
            .map(|e| e.value = PropertyValue::String(id_str.into()));

        // Set passive sync mode (camera trigger in passive / listen mode)
        self.write_register(OFFSET_CAM_SYNC_MODE, 0)?;
        self.props
            .entry_mut("Camera trigger")
            .map(|e| e.value = PropertyValue::String("Passive".into()));

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> {
        self.initialized = false;
        Ok(())
    }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> {
        if self.initialized && name == "Camera trigger" {
            let mode = self.call_transport(|t| read_register(t, OFFSET_CAM_SYNC_MODE))?;
            return Ok(PropertyValue::String(
                if mode == 1 { "Active" } else { "Passive" }.into(),
            ));
        }
        self.props.get(name).cloned()
    }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "Camera trigger" {
            let mode = match &val {
                PropertyValue::String(s) if s == "Active" => 1,
                PropertyValue::String(s) if s == "Passive" => 0,
                _ => return Err(MmError::InvalidPropertyValue),
            };
            if self.initialized {
                self.write_register(OFFSET_CAM_SYNC_MODE, mode)?;
            }
        }
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
        DeviceType::Hub
    }
    fn busy(&self) -> bool {
        false
    }
}

impl Hub for MicroFpgaHub {
    fn detect_installed_devices(&mut self) -> MmResult<Vec<String>> {
        let mut devices = vec!["Laser Trigger".to_string(), "Camera Trigger".to_string()];
        // Only Au, Au+, Mojo have ADC
        if matches!(self.id, ID_AU | ID_AUP | ID_MOJO) {
            devices.push("Analog Input".to_string());
        }
        devices.push("PWM".to_string());
        devices.push("TTL".to_string());
        devices.push("Servos".to_string());
        Ok(devices)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn le4(v: u32) -> Vec<u8> {
        v.to_le_bytes().to_vec()
    }

    fn make_hub() -> MicroFpgaHub {
        let t = MockTransport::new()
            // read version → 3
            .expect_binary(&le4(3))
            // read id → 79 (Au)
            .expect_binary(&le4(79));
        // write_register for passive sync sends 9 bytes, no response expected
        MicroFpgaHub::new().with_transport(Box::new(t))
    }

    #[test]
    fn initialize_ok() {
        let mut hub = make_hub();
        hub.initialize().unwrap();
        assert_eq!(hub.version, 3);
        assert_eq!(hub.id, 79);
        assert_eq!(
            hub.get_property("MicroFPGA ID").unwrap(),
            PropertyValue::String("Au".into())
        );
    }

    #[test]
    fn initialize_labels_cu_and_mojo_ids_match_cpp() {
        let t = MockTransport::new()
            .expect_binary(&le4(3))
            .expect_binary(&le4(ID_CU));
        let mut hub = MicroFpgaHub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();
        assert_eq!(
            hub.get_property("MicroFPGA ID").unwrap(),
            PropertyValue::String("Mojo".into())
        );

        let t = MockTransport::new()
            .expect_binary(&le4(3))
            .expect_binary(&le4(ID_MOJO));
        let mut hub = MicroFpgaHub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();
        assert_eq!(
            hub.get_property("MicroFPGA ID").unwrap(),
            PropertyValue::String("Cu".into())
        );
    }

    #[test]
    fn wrong_version_rejected() {
        let t = MockTransport::new()
            .expect_binary(&le4(2)) // wrong version
            .expect_binary(&le4(79));
        let mut hub = MicroFpgaHub::new().with_transport(Box::new(t));
        assert!(hub.initialize().is_err());
    }

    #[test]
    fn id_mapping_matches_cpp() {
        let t = MockTransport::new()
            .expect_binary(&le4(3))
            .expect_binary(&le4(ID_CU));
        let mut hub = MicroFpgaHub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();
        assert_eq!(
            hub.get_property("MicroFPGA ID").unwrap(),
            PropertyValue::String("Mojo".into())
        );
    }

    #[test]
    fn camera_trigger_property_reads_live_sync_mode() {
        let t = MockTransport::new()
            .expect_binary(&le4(3))
            .expect_binary(&le4(ID_AU))
            .expect_binary(&le4(1));
        let mut hub = MicroFpgaHub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();

        assert_eq!(
            hub.get_property("Camera trigger").unwrap(),
            PropertyValue::String("Active".into())
        );
    }

    #[test]
    fn camera_trigger_unknown_live_sync_mode_defaults_passive() {
        let t = MockTransport::new()
            .expect_binary(&le4(3))
            .expect_binary(&le4(ID_AU))
            .expect_binary(&le4(2));
        let mut hub = MicroFpgaHub::new().with_transport(Box::new(t));
        hub.initialize().unwrap();

        assert_eq!(
            hub.get_property("Camera trigger").unwrap(),
            PropertyValue::String("Passive".into())
        );
    }
}
