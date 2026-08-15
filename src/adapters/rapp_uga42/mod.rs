pub mod scanner;

pub use scanner::RappUga42Scanner;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub const DEVICE_NAME: &str = "RappUGA42Scanner";

static DEVICE_LIST: &[DeviceInfo] = &[DeviceInfo {
    name: DEVICE_NAME,
    description: "Rapp UGA-42 Scanner",
    device_type: DeviceType::Galvo,
}];

pub struct RappUga42Adapter;

impl AdapterModule for RappUga42Adapter {
    fn module_name(&self) -> &'static str {
        "Rapp_UGA42"
    }

    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }

    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME => Some(AnyDevice::Galvo(Box::new(RappUga42Scanner::new()))),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::minicore::{GalvoPoint, MiniCore, MiniOrchestrator, TriggerEdge};
    use crate::traits::Device;
    use crate::types::PropertyValue;

    #[test]
    fn minicore_builtin_registry_can_load_rapp_uga42_galvo() {
        let mut core = MiniCore::with_builtin_adapters();
        assert_eq!(
            core.get_available_devices("Rapp_UGA42").unwrap(),
            vec![DEVICE_NAME.to_string()]
        );

        core.load_device("scanner", "Rapp_UGA42", DEVICE_NAME)
            .unwrap();
        assert_eq!(core.device_type("scanner").unwrap(), DeviceType::Galvo);
    }

    #[test]
    fn scanner_exposes_upstream_identity_and_preinit_properties() {
        let mut scanner =
            RappUga42Scanner::with_backend(Box::new(scanner::MockRappBackend::default()));

        assert_eq!(scanner.name(), DEVICE_NAME);
        assert_eq!(scanner.description(), "Rapp UGA-42 Scanner");
        assert_eq!(
            scanner.property_names(),
            vec!["DebugMode".to_string(), "VirtualComPort".to_string()]
        );
        assert_eq!(
            scanner.get_property("DebugMode").unwrap(),
            PropertyValue::String("False".into())
        );
        scanner
            .set_property("DebugMode", PropertyValue::String("True".into()))
            .unwrap();
        assert_eq!(
            scanner.get_property("DebugMode").unwrap(),
            PropertyValue::String("True".into())
        );

        scanner.initialize().unwrap();
        assert_eq!(
            scanner.set_property("DebugMode", PropertyValue::String("False".into())),
            Err(crate::error::MmError::CanNotSetProperty)
        );
        assert!(scanner.has_property("ScanMode"));
        assert!(scanner.has_property("LaserIntensity"));
        assert!(scanner.has_property("PolygonRepetitions"));
        assert_eq!(
            scanner.get_property("TTLTriggerMode").unwrap(),
            PropertyValue::String("None".into())
        );
        scanner
            .set_property("TTLTriggerMode", PropertyValue::String("Port2_Once".into()))
            .unwrap();
        assert_eq!(
            scanner.get_property("TTLTriggerMode").unwrap(),
            PropertyValue::String("Port2_Once".into())
        );
        scanner
            .set_property("LaserPort", PropertyValue::String("RMIPort4".into()))
            .unwrap();
        assert_eq!(
            scanner.get_property("LaserPort").unwrap(),
            PropertyValue::String("RMIPort4".into())
        );
        assert_eq!(
            scanner.set_property("SpotSize", PropertyValue::Integer(0)),
            Err(crate::error::MmError::InvalidPropertyValue)
        );
        assert_eq!(
            scanner.set_property("LaserFrequency", PropertyValue::Integer(100001)),
            Err(crate::error::MmError::InvalidPropertyValue)
        );
        scanner
            .set_property("LaserFrequency", PropertyValue::Integer(1000))
            .unwrap();
        assert_eq!(
            scanner.get_property("LaserType").unwrap(),
            PropertyValue::String("Pulsed".into())
        );
        scanner
            .set_property("LaserType", PropertyValue::String("Continuous".into()))
            .unwrap();
        assert_eq!(
            scanner.get_property("LaserFrequency").unwrap(),
            PropertyValue::Integer(0)
        );
    }

    #[test]
    fn mini_orchestrator_drives_rapp_uga42_galvo_operations() {
        let backend = scanner::MockRappBackend::default();
        let shared = backend.shared();
        let mut core = MiniCore::new();
        core.add_device(
            "scanner",
            "Rapp_UGA42",
            DEVICE_NAME,
            AnyDevice::Galvo(Box::new(RappUga42Scanner::with_backend(Box::new(backend)))),
        )
        .unwrap();
        let mut orchestrator = MiniOrchestrator::new(&mut core);

        orchestrator.initialize_device("scanner").unwrap();
        orchestrator
            .set_galvo_position("scanner", 12.5, 34.0)
            .unwrap();
        assert_eq!(
            orchestrator.get_galvo_position("scanner").unwrap(),
            (12.5, 34.0)
        );
        orchestrator
            .set_galvo_illumination_state("scanner", true)
            .unwrap();
        assert_eq!(
            orchestrator.arm_galvo_sequence("scanner", TriggerEdge::Rising),
            Err(crate::error::MmError::InvalidPropertyValue)
        );

        let op = orchestrator
            .start_set_galvo_position("scanner", 40.0, 50.0)
            .unwrap();
        orchestrator
            .wait_operation_or_cancel(op.id(), std::time::Duration::from_secs(1))
            .unwrap();
        assert_eq!(
            orchestrator.get_galvo_position("scanner").unwrap(),
            (40.0, 50.0)
        );

        orchestrator
            .load_galvo_sequence(
                "scanner",
                vec![
                    GalvoPoint {
                        x: 1.0,
                        y: 2.0,
                        dwell_ms: 10.0,
                    },
                    GalvoPoint {
                        x: 3.0,
                        y: 4.0,
                        dwell_ms: 20.0,
                    },
                ],
            )
            .unwrap();
        orchestrator
            .arm_galvo_sequence("scanner", TriggerEdge::Rising)
            .unwrap();
        orchestrator.stop_galvo_sequence("scanner").unwrap();

        let state = shared.lock().unwrap();
        assert_eq!(state.moves, vec![(12.5, 34.0), (40.0, 50.0)]);
        assert_eq!(state.illumination, vec![true]);
        assert_eq!(state.loaded_points.len(), 2);
        assert_eq!(
            state.armed_edges,
            vec![crate::traits::DeviceTriggerEdge::Rising]
        );
        assert!(state.stopped);
    }
}
