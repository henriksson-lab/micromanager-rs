use micromanager::adapters::demo::DemoAdapter;
use micromanager::CMMCore;
use micromanager::MmError;
use micromanager::PropertyValue;

fn make_core() -> CMMCore {
    let mut core = CMMCore::new();
    core.register_adapter(Box::new(DemoAdapter));
    core
}

// ─── Phase 3 integration test — snap image ────────────────────────────────────

#[test]
fn snap_image_check_buffer_size() {
    let mut core = make_core();
    core.load_device("Camera", "demo", "DCam").unwrap();
    core.initialize_all_devices().unwrap();
    core.set_camera_device("Camera").unwrap();
    core.snap_image().unwrap();
    let frame = core.get_image().unwrap();
    assert_eq!(frame.data.len(), 512 * 512);
    assert_eq!(frame.width, 512);
    assert_eq!(frame.height, 512);
    assert_eq!(frame.bytes_per_pixel, 1);
}

// ─── Sequence acquisition ─────────────────────────────────────────────────────

#[test]
fn sequence_acquisition_start_stop() {
    let mut core = make_core();
    core.load_device("Camera", "demo", "DCam").unwrap();
    core.initialize_all_devices().unwrap();
    core.set_camera_device("Camera").unwrap();
    core.start_sequence_acquisition(10, 100.0).unwrap();
    assert!(core.is_sequence_running().unwrap());
    core.stop_sequence_acquisition().unwrap();
    assert!(!core.is_sequence_running().unwrap());
}

// ─── Stage movement ───────────────────────────────────────────────────────────

#[test]
fn stage_set_get_position() {
    let mut core = make_core();
    core.load_device("Focus", "demo", "DStage").unwrap();
    core.initialize_all_devices().unwrap();
    core.set_focus_device("Focus").unwrap();
    core.set_position(42.5).unwrap();
    assert!((core.get_position().unwrap() - 42.5).abs() < 1e-9);
    core.set_relative_position(-2.5).unwrap();
    assert!((core.get_position().unwrap() - 40.0).abs() < 1e-9);
}

// ─── XY stage ─────────────────────────────────────────────────────────────────

#[test]
fn xy_stage_set_get_position() {
    let mut core = make_core();
    core.load_device("XY", "demo", "DXYStage").unwrap();
    core.initialize_all_devices().unwrap();
    core.set_xy_stage_device("XY").unwrap();
    core.set_xy_position(100.0, 200.0).unwrap();
    let (x, y) = core.get_xy_position().unwrap();
    assert!((x - 100.0).abs() < 1e-9);
    assert!((y - 200.0).abs() < 1e-9);
}

#[test]
fn demo_stage_wheel_and_xy_upstream_properties() {
    let mut core = make_core();
    core.load_device("Focus", "demo", "DStage").unwrap();
    core.load_device("Wheel", "demo", "DWheel").unwrap();
    core.load_device("XY", "demo", "DXYStage").unwrap();
    core.initialize_all_devices().unwrap();

    core.set_property("Focus", "Position", PropertyValue::Float(42.0))
        .unwrap();
    assert_eq!(
        core.get_property("Focus", "Position").unwrap(),
        PropertyValue::Float(42.0)
    );
    core.set_property("Focus", "UseSequences", PropertyValue::String("Yes".into()))
        .unwrap();
    assert_eq!(
        core.get_property("Focus", "UseSequences").unwrap(),
        PropertyValue::String("Yes".into())
    );

    core.set_property("Wheel", "Closed_Position", PropertyValue::Integer(9))
        .unwrap();
    assert_eq!(
        core.get_property("Wheel", "Closed_Position").unwrap(),
        PropertyValue::Integer(9)
    );

    core.set_property("XY", "Velocity", PropertyValue::Float(-1.0))
        .unwrap();
    assert_eq!(
        core.get_property("XY", "Velocity").unwrap(),
        PropertyValue::Float(0.1)
    );
}

// ─── Shutter ──────────────────────────────────────────────────────────────────

#[test]
fn shutter_open_close() {
    let mut core = make_core();
    core.load_device("Shutter", "demo", "DShutter").unwrap();
    core.initialize_all_devices().unwrap();
    core.set_shutter_device("Shutter").unwrap();
    assert!(!core.get_shutter_open().unwrap());
    core.set_shutter_open(true).unwrap();
    assert!(core.get_shutter_open().unwrap());
}

// ─── Property access via core ─────────────────────────────────────────────────

#[test]
fn property_get_set_via_core() {
    let mut core = make_core();
    core.load_device("Camera", "demo", "DCam").unwrap();
    core.initialize_all_devices().unwrap();
    core.set_property("Camera", "Exposure", PropertyValue::Float(50.0))
        .unwrap();
    let val = core.get_property("Camera", "Exposure").unwrap();
    assert_eq!(val.as_f64().unwrap(), 50.0);
}

// ─── Config round-trip ────────────────────────────────────────────────────────

#[test]
fn config_save_and_reload() {
    let mut core = make_core();
    core.load_device("Camera", "demo", "DCam").unwrap();
    core.initialize_all_devices().unwrap();
    core.set_property("Camera", "Exposure", PropertyValue::Float(25.0))
        .unwrap();

    let cfg_text = core.save_system_configuration().unwrap();
    assert!(cfg_text.contains("Camera,demo,DCam"));
    assert!(cfg_text.contains("Exposure"));

    // Parse and verify round-trip
    let mut core2 = make_core();
    core2.load_system_configuration(&cfg_text).unwrap();
    let val = core2.get_property("Camera", "Exposure").unwrap();
    assert_eq!(val.as_f64().unwrap(), 25.0);
}

// ─── Unload device ────────────────────────────────────────────────────────────

#[test]
fn unload_device() {
    let mut core = make_core();
    core.load_device("Camera", "demo", "DCam").unwrap();
    core.initialize_all_devices().unwrap();
    assert!(core.device_labels().contains(&"Camera"));
    core.unload_device("Camera").unwrap();
    assert!(!core.device_labels().contains(&"Camera"));
}

#[test]
fn missing_core_devices_use_specific_errors() {
    let mut core = make_core();

    assert_eq!(
        core.snap_image().unwrap_err(),
        MmError::CoreCameraNotAvailable
    );
    assert_eq!(
        core.set_xy_position(1.0, 2.0).unwrap_err(),
        MmError::CoreInvalidXYStageDevice
    );
    assert_eq!(
        core.set_shutter_open(true).unwrap_err(),
        MmError::CoreInvalidShutterDevice
    );
}
