//! UniversalGeneric — a passthrough generic device for the Universal Serial Hub.
use micromanager::error::{MmResult};
use micromanager::property::PropertyMap;
use micromanager::traits::{Device, Generic};
use micromanager::transport::Transport;
use micromanager::types::{DeviceType, PropertyValue};

pub struct UniversalGeneric {
    name: String,
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
}

impl UniversalGeneric {
    pub fn new(name: &str) -> Self {
        let mut props = PropertyMap::new();
        props.define_property("Port", PropertyValue::String("Undefined".into()), false).unwrap();
        Self { name: name.to_string(), props, transport: None }
    }

    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t); self
    }
}

impl Device for UniversalGeneric {
    fn name(&self) -> &str { &self.name }
    fn description(&self) -> &str { "Universal Serial Hub generic device" }
    fn initialize(&mut self) -> MmResult<()> { Ok(()) }
    fn shutdown(&mut self) -> MmResult<()> { Ok(()) }
    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> { self.props.set(name, val) }
    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, _name: &str) -> bool { false }
    fn device_type(&self) -> DeviceType { DeviceType::Generic }
    fn busy(&self) -> bool { false }
}
impl Generic for UniversalGeneric {}
