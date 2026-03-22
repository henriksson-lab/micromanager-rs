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
use micromanager::error::{MmError, MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, StateDevice};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

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
    gate_open: bool,
}

impl ConixQuadFilter {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self { props, transport: None, initialized: false, position: 0, gate_open: true }
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
        let c = format!("{}\r", command);
        self.call_transport(|t| { let r = t.send_recv(&c)?; Ok(r.trim().to_string()) })
    }
}

impl Default for ConixQuadFilter { fn default() -> Self { Self::new() } }

impl Device for ConixQuadFilter {
    fn name(&self) -> &str { "ConixQuadFilter" }
    fn description(&self) -> &str { "Conix QuadFluor 4-position filter changer" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        // Query with trailing space: "Quad "
        let r = self.cmd("Quad ")?;
        let body = check_a(&r)?;
        let pos1: u64 = body.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(1);
        self.position = pos1.saturating_sub(1);
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
    fn device_type(&self) -> DeviceType { DeviceType::State }
    fn busy(&self) -> bool { false }
}

impl StateDevice for ConixQuadFilter {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= 4 {
            return Err(MmError::LocallyDefined(format!("Position {} out of range (0-3)", pos)));
        }
        let r = self.cmd(&format!("Quad {}", pos + 1))?;
        check_a(&r)?;
        self.position = pos;
        Ok(())
    }
    fn get_position(&self) -> MmResult<u64> { Ok(self.position) }
    fn get_number_of_positions(&self) -> u64 { 4 }
    fn get_position_label(&self, pos: u64) -> MmResult<String> { Ok(format!("Position-{}", pos + 1)) }
    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let pos: u64 = label.strip_prefix("Position-")
            .and_then(|s| s.parse::<u64>().ok())
            .map(|p| p.saturating_sub(1))
            .ok_or_else(|| MmError::UnknownLabel(label.to_string()))?;
        self.set_position(pos)
    }
    fn set_position_label(&mut self, _pos: u64, _label: &str) -> MmResult<()> { Ok(()) }
    fn set_gate_open(&mut self, open: bool) -> MmResult<()> { self.gate_open = open; Ok(()) }
    fn get_gate_open(&self) -> MmResult<bool> { Ok(self.gate_open) }
}

// ── HexaFluor ─────────────────────────────────────────────────────────────────

pub struct ConixHexFilter {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
    initialized: bool,
    position: u64,
    num_positions: u64,
    gate_open: bool,
}

impl ConixHexFilter {
    pub fn new() -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        props.define_property("NumPositions", PropertyValue::Integer(6), false).unwrap();
        Self { props, transport: None, initialized: false, position: 0, num_positions: 6, gate_open: true }
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
        let c = format!("{}\r", command);
        self.call_transport(|t| { let r = t.send_recv(&c)?; Ok(r.trim().to_string()) })
    }
}

impl Default for ConixHexFilter { fn default() -> Self { Self::new() } }

impl Device for ConixHexFilter {
    fn name(&self) -> &str { "ConixHexFilter" }
    fn description(&self) -> &str { "Conix HexaFluor 6-position filter changer" }

    fn initialize(&mut self) -> MmResult<()> {
        if self.transport.is_none() { return Err(MmError::NotConnected); }
        let r = self.cmd("Cube ")?;
        let body = check_a(&r)?;
        let pos1: u64 = body.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(1);
        self.position = pos1.saturating_sub(1);
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> MmResult<()> { self.initialized = false; Ok(()) }

    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> {
        if name == "NumPositions" {
            self.num_positions = val.as_i64().ok_or(MmError::InvalidPropertyValue)? as u64;
        }
        self.props.set(name, val)
    }
    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool {
        self.props.entry(name).map(|e| e.read_only).unwrap_or(false)
    }
    fn device_type(&self) -> DeviceType { DeviceType::State }
    fn busy(&self) -> bool { false }
}

impl StateDevice for ConixHexFilter {
    fn set_position(&mut self, pos: u64) -> MmResult<()> {
        if pos >= self.num_positions {
            return Err(MmError::LocallyDefined(format!("Position {} out of range", pos)));
        }
        let r = self.cmd(&format!("Cube {}", pos + 1))?;
        check_a(&r)?;
        self.position = pos;
        Ok(())
    }
    fn get_position(&self) -> MmResult<u64> { Ok(self.position) }
    fn get_number_of_positions(&self) -> u64 { self.num_positions }
    fn get_position_label(&self, pos: u64) -> MmResult<String> { Ok(format!("Position-{}", pos + 1)) }
    fn set_position_by_label(&mut self, label: &str) -> MmResult<()> {
        let pos: u64 = label.strip_prefix("Position-")
            .and_then(|s| s.parse::<u64>().ok())
            .map(|p| p.saturating_sub(1))
            .ok_or_else(|| MmError::UnknownLabel(label.to_string()))?;
        self.set_position(pos)
    }
    fn set_position_label(&mut self, _pos: u64, _label: &str) -> MmResult<()> { Ok(()) }
    fn set_gate_open(&mut self, open: bool) -> MmResult<()> { self.gate_open = open; Ok(()) }
    fn get_gate_open(&self) -> MmResult<bool> { Ok(self.gate_open) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micromanager::transport::MockTransport;

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
