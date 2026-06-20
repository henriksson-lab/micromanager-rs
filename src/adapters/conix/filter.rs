/// Conix Research QuadFluor (4-position) and HexaFluor (6-position) filter changers
/// for Nikon TE200/300 microscopes.
///
/// Protocol (TX `\r`, RX `\r`):
///   QuadFluor:
///     `Quad \r`    → `:A <N>\r`  query current position (1-indexed)
///     `Quad N\r`   → `:A\r`      set position N (1-indexed)
///   HexaFluor:
///     `Cube \r`    → `:A <N>\r`  query current position (1-indexed)
///     `Cube N\r`   → `:A\r`      set position N (1-indexed)
///   Error response: `:N<code>\r`
use crate::error::{MmError, MmResult};
use crate::property::PropertyMap;
use crate::traits::{Device, StateDevice};
use crate::transport::Transport;
use crate::types::{DeviceType, PropertyValue};

/// Helper: parse `:A` prefix, returning remainder; error on `:N`.
fn check_a(resp: &str) -> MmResult<&str> {
    let s = resp.trim();
    if let Some(rest) = s.strip_prefix(":A") {
        Ok(rest.trim())
    } else {
        Err(MmError::LocallyDefined(format!("Conix error: {}", s)))
    }
}

// ── QuadFluor ────────────────────────────────────────────────────────────────

pub struct ConixQuadFilter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    position: u64,
    labels: Vec<String>,
    gate_open: bool,
}

impl ConixQuadFilter {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("State", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .set_allowed_values("State", &["0", "1", "2", "3"])
            .unwrap();
        props
            .define_property("Label", PropertyValue::String(String::new()), false)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            position: 0,
            labels: (0..4).map(|i| format!("Position: {}", i)).collect(),
            gate_open: true,
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
        let c = format!("{}\r", command);
        self.call_transport(|t| {
            let r = t.send_recv(&c)?;
            Ok(r.trim().to_string())
        })
    }
}

impl Default for ConixQuadFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ConixQuadFilter {
    fn name(&self) -> &str {
        "ConixQuadFilter"
    }
    fn description(&self) -> &str {
        "Conix Motorized Qud-Filter changer for Nikon TE200/300"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        // Query with trailing space: "Quad "
        let r = self.cmd("Quad ")?;
        let body = check_a(&r)?;
        let pos1: u64 = body
            .split_whitespace()
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        self.position = pos1.saturating_sub(1);
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

impl StateDevice for ConixQuadFilter {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= 4 {
            return Err(MmError::LocallyDefined(format!(
                "Position {} out of range (0-3)",
                pos
            )));
        }
        let r = self.cmd(&format!("Quad {}", pos + 1))?;
        check_a(&r)?;
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
        4
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
        if pos >= 4 {
            return Err(MmError::UnknownPosition);
        }
        self.labels[pos as usize] = label.to_string();
        if pos == self.position {
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

// ── HexaFluor ─────────────────────────────────────────────────────────────────

pub struct ConixHexFilter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    position: u64,
    num_positions: u64,
    labels: Vec<String>,
    gate_open: bool,
}

impl ConixHexFilter {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props
            .define_property("Port", PropertyValue::String("Undefined".into()), false)
            .unwrap();
        props
            .define_property("State", PropertyValue::Integer(0), false)
            .unwrap();
        props
            .set_allowed_values("State", &["0", "1", "2", "3", "4", "5"])
            .unwrap();
        props
            .define_property("Label", PropertyValue::String(String::new()), false)
            .unwrap();
        Self {
            props,
            transport: None,
            initialized: false,
            position: 0,
            num_positions: 6,
            labels: (0..6).map(|i| format!("Position: {}", i)).collect(),
            gate_open: true,
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
        let c = format!("{}\r", command);
        self.call_transport(|t| {
            let r = t.send_recv(&c)?;
            Ok(r.trim().to_string())
        })
    }
}

impl Default for ConixHexFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for ConixHexFilter {
    fn name(&self) -> &str {
        "ConixHexFilter"
    }
    fn description(&self) -> &str {
        "Conix Motorized 6-Filter changer for Nikon TE200/300"
    }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() {
            return Err(MmError::NotConnected);
        }
        let r = self.cmd("Cube ")?;
        let body = check_a(&r)?;
        let pos1: u64 = body
            .split_whitespace()
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        self.position = pos1.saturating_sub(1);
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

impl StateDevice for ConixHexFilter {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::LocallyDefined(format!(
                "Position {} out of range",
                pos
            )));
        }
        let r = self.cmd(&format!("Cube {}", pos + 1))?;
        check_a(&r)?;
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
        if pos == self.position {
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

    // ── QuadFluor tests ─────────────────────────────────────────────────────

    #[test]
    fn quad_initialize() {
        let t = MockTransport::new().any(":A 2"); // position 2 (1-indexed)
        let mut f = ConixQuadFilter::new().with_transport(Box::new(t));
        f.initialize().unwrap();
        assert_eq!(f.get_position().unwrap(), 1); // 0-indexed
    }

    #[test]
    fn quad_set_position() {
        let t = MockTransport::new().any(":A 1").any(":A");
        let mut f = ConixQuadFilter::new().with_transport(Box::new(t));
        f.initialize().unwrap();
        f.set_position(3).unwrap();
        assert_eq!(f.get_position().unwrap(), 3);
    }

    #[test]
    fn quad_out_of_range() {
        let t = MockTransport::new().any(":A 1");
        let mut f = ConixQuadFilter::new().with_transport(Box::new(t));
        f.initialize().unwrap();
        assert!(f.set_position(4).is_err());
    }

    #[test]
    fn quad_error_response_fails() {
        let t = MockTransport::new().any(":A 1").any(":N-21");
        let mut f = ConixQuadFilter::new().with_transport(Box::new(t));
        f.initialize().unwrap();
        assert!(f.set_position(2).is_err());
    }

    // ── HexaFluor tests ──────────────────────────────────────────────────────

    #[test]
    fn hex_initialize() {
        let t = MockTransport::new().any(":A 4"); // position 4 (1-indexed)
        let mut f = ConixHexFilter::new().with_transport(Box::new(t));
        f.initialize().unwrap();
        assert_eq!(f.get_position().unwrap(), 3); // 0-indexed
    }

    #[test]
    fn hex_set_position() {
        let t = MockTransport::new().any(":A 1").any(":A");
        let mut f = ConixHexFilter::new().with_transport(Box::new(t));
        f.initialize().unwrap();
        f.set_position(5).unwrap();
        assert_eq!(f.get_position().unwrap(), 5);
    }

    #[test]
    fn hex_out_of_range() {
        let t = MockTransport::new().any(":A 1");
        let mut f = ConixHexFilter::new().with_transport(Box::new(t));
        f.initialize().unwrap();
        assert!(f.set_position(6).is_err());
    }

    #[test]
    fn no_transport_error() {
        assert!(ConixQuadFilter::new().initialize().is_err());
        assert!(ConixHexFilter::new().initialize().is_err());
    }
}
