#![allow(dead_code)]

use micromanager::adapters::demo_async::{AsyncDemoCamera, AsyncDemoStage};
use micromanager::{
    AcquisitionPlan, Action, Dependency, ImageAnalysisService, ImageRecorder, MiniCore, MmResult,
    Position, PropertyValue, RecordingPolicy, Roi, ScanPath, Stimulus,
};
use std::time::Duration;

// This example file is a catalog of API sketches rather than a runnable demo.
// Each function below captures a workflow the future MiniCore should make
// natural to express.
fn main() -> MmResult<()> {
    Ok(())
}

// Hardware-triggered capture: arm a camera so a TTL rising edge causes one
// image to be captured and queued to a recorder without polling from user code.
fn hardware_triggered_picture() -> MmResult<()> {
    let mut scope = MiniCore::new();
    scope.add_device("camera", AsyncDemoCamera::new())?;
    scope.add_device("recorder", ImageRecorder::new())?;
    scope.add_dependency("camera", Dependency::trigger_source("ttl0"))?;

    let camera = scope.camera("camera")?;
    let recorder = scope.recorder("recorder")?;
    recorder.policy(RecordingPolicy::new("triggered_frames"))?;
    recorder.attach(&camera)?;

    let trigger = scope.trigger("ttl0")?;
    let armed = trigger
        .on_rising_edge(Action::snap(&camera).record_to(&recorder))
        .arm()?;

    armed.wait(Duration::from_secs(1))
}

// Closed-loop live-cell workflow: survey at low resolution, analyze frames for
// a cell about to divide, switch objective, move/focus, record a high-resolution
// timelapse, then return to the survey setup.
fn adaptive_cell_division_timelapse() -> MmResult<()> {
    let mut scope = MiniCore::new();
    scope.add_device("camera", AsyncDemoCamera::new())?;
    scope.add_device("focus", AsyncDemoStage::new())?;
    scope.add_device("analysis", ImageAnalysisService::new())?;
    scope.add_device("recorder", ImageRecorder::new())?;
    scope.add_dependency("camera", Dependency::uses_stage("focus"))?;

    let camera = scope.camera("camera")?;
    let focus = scope.stage("focus")?;
    let objective = scope.control("objective_turret")?;
    let autofocus = scope.control("hardware_autofocus")?;
    let analysis = scope.analysis("analysis")?;
    let recorder = scope.recorder("recorder")?;

    recorder.policy(RecordingPolicy::new("adaptive_cell_division"))?;
    objective
        .set_property("objective", PropertyValue::from("10x"))
        .submit()?
        .wait(Duration::from_secs(2))?;

    loop {
        let low_res_frame = camera
            .snap_with(AcquisitionPlan::new("low_res_survey"))
            .submit()?
            .wait(Duration::from_secs(1))?;

        if let Some(cell) = analysis.find_dividing_cells(&low_res_frame)?.first() {
            objective
                .set_property("objective", PropertyValue::from("40x"))
                .submit()?
                .wait(Duration::from_secs(2))?;

            focus
                .move_to_um(cell.roi.y as f64)
                .submit()?
                .wait(Duration::from_secs(2))?;

            autofocus.arm().submit()?.wait(Duration::from_secs(1))?;
            camera.record_to(&recorder)?;
            camera
                .start_sequence(240, Duration::from_secs(30))
                .submit()?
                .wait(Duration::from_secs(2))?;

            objective
                .set_property("objective", PropertyValue::from("10x"))
                .submit()?
                .wait(Duration::from_secs(2))?;
            break;
        }
    }

    Ok(())
}

// Laser scanning confocal workflow: coordinate a scan engine, pulsed laser, and
// TCSPC detector so FLIM collection and a FRAP-style bleach segment can be part
// of one acquisition plan.
fn laser_scanning_confocal_flim_frap() -> MmResult<()> {
    let scope = MiniCore::new();
    let scan_engine = scope.control("scan_engine")?;
    let detector = scope.control("tcspc_detector")?;
    let laser = scope.control("pulsed_laser")?;
    let recorder = scope.recorder("recorder")?;

    recorder.policy(RecordingPolicy::new("confocal_flim_frap"))?;
    laser
        .set_property("sync_clock", PropertyValue::from("80MHz"))
        .submit()?;
    detector
        .arm_with(AcquisitionPlan::new("photon_lifetime_histograms"))
        .submit()?;
    scan_engine
        .arm_with(AcquisitionPlan::new("raster_then_bleach_roi"))
        .submit()?;
    scan_engine.start().submit()?.wait(Duration::from_secs(1))
}

// Optoacoustic workflow: drive a sparse scan path with a pulsed laser and DAQ,
// treating ultrasound A-lines as first-class waveform data rather than camera
// frames.
fn optoacoustic_sparse_raster() -> MmResult<()> {
    let scope = MiniCore::new();
    let laser = scope.control("nanosecond_pulse_laser")?;
    let scanner = scope.control("galvo_or_stage_scanner")?;
    let daq = scope.control("ultrasound_daq")?;

    laser
        .set_property("wavelength_nm", PropertyValue::from(532_i64))
        .submit()?;

    scanner
        .arm_with(AcquisitionPlan::new("sparse_photoacoustic_scan"))
        .submit()?;
    daq.arm_with(AcquisitionPlan::new("a_line_waveforms"))
        .submit()?;
    scanner.start().submit()?.wait(Duration::from_secs(1))
}

// Light-sheet workflow: coordinate sCMOS exposure, sheet galvo sweep, laser
// arming, and sample Z motion as one volume acquisition.
fn light_sheet_volume() -> MmResult<()> {
    let scope = MiniCore::new();
    let camera = scope.camera("sCMOS")?;
    let sheet_scanner = scope.control("sheet_galvo")?;
    let z = scope.stage("sample_z")?;
    let laser = scope.control("laser")?;
    let recorder = scope.recorder("recorder")?;

    recorder.policy(RecordingPolicy::new("light_sheet_volume"))?;
    camera.record_to(&recorder)?;
    laser.arm().submit()?;
    sheet_scanner
        .arm_with(AcquisitionPlan::new("sheet_sweep_per_frame"))
        .submit()?;
    z.move_by_um(50.0).submit()?;
    camera
        .start_sequence(200, Duration::from_millis(10))
        .submit()?
        .wait(Duration::from_secs(1))
}

// Adaptive optics workflow: capture an image, compute an image-quality metric,
// update a deformable mirror, and repeat as a low-latency feedback loop.
fn adaptive_optics_feedback() -> MmResult<()> {
    let scope = MiniCore::new();
    let camera = scope.camera("wavefront_or_image_camera")?;
    let analysis = scope.analysis("image_quality_metric")?;
    let mirror = scope.control("deformable_mirror")?;

    for _ in 0..5 {
        let frame = camera.snap().submit()?.wait(Duration::from_millis(200))?;
        let score = analysis.find_dividing_cells(&frame)?.len() as i64;
        mirror
            .set_property("correction_mode", PropertyValue::from(score))
            .submit()?;
    }

    Ok(())
}

// Super-resolution localization workflow: run long high-rate image sequences
// while controlling activation/excitation laser powers and streaming frames to
// storage.
fn super_resolution_localization() -> MmResult<()> {
    let scope = MiniCore::new();
    let camera = scope.camera("emccd_or_scmos")?;
    let activation = scope.control("activation_laser")?;
    let excitation = scope.control("excitation_laser")?;
    let recorder = scope.recorder("recorder")?;

    recorder.policy(RecordingPolicy::new("single_molecule_localization"))?;
    camera.record_to(&recorder)?;
    activation
        .set_property("power_fraction", PropertyValue::from(0.01))
        .submit()?;
    excitation
        .set_property("power_fraction", PropertyValue::from(0.25))
        .submit()?;
    camera
        .start_sequence(20_000, Duration::from_millis(5))
        .submit()?
        .wait(Duration::from_secs(1))
}

// Electrophysiology-coupled imaging: align fast camera frames with DAQ analog
// traces and patch-clamp or stimulus waveforms using shared timing markers.
fn electrophysiology_coupled_imaging() -> MmResult<()> {
    let scope = MiniCore::new();
    let camera = scope.camera("camera")?;
    let daq = scope.control("daq")?;
    let stimulator = scope.control("patch_clamp_or_stimulus")?;
    let recorder = scope.recorder("recorder")?;

    recorder.policy(RecordingPolicy::new("ephys_aligned_imaging"))?;
    camera.record_to(&recorder)?;
    daq.arm_with(AcquisitionPlan::new("analog_trace_with_frame_markers"))
        .submit()?;
    stimulator
        .arm_with(AcquisitionPlan::new("voltage_or_current_protocol"))
        .submit()?;
    camera
        .start_sequence(1_000, Duration::from_millis(2))
        .submit()?
        .wait(Duration::from_secs(1))
}

// Microfluidic perturbation workflow: change valves and pump pressure while
// recording a long timelapse with metadata for each fluidic state transition.
fn microfluidic_perturbation_screen() -> MmResult<()> {
    let scope = MiniCore::new();
    let camera = scope.camera("camera")?;
    let pump = scope.control("pressure_pump")?;
    let valve = scope.control("valve_bank")?;
    let recorder = scope.recorder("recorder")?;

    recorder.policy(RecordingPolicy::new("microfluidic_perturbation"))?;
    camera.record_to(&recorder)?;
    valve
        .set_property("active_inlet", PropertyValue::from("drug_a"))
        .submit()?;
    pump.set_property("pressure_mbar", PropertyValue::from(35_i64))
        .submit()?;
    camera
        .start_sequence(360, Duration::from_secs(10))
        .submit()?
        .wait(Duration::from_secs(1))
}

// Patterned stimulation workflow: define an ROI stimulus for a DMD/SLM/galvo
// path while simultaneously acquiring camera frames.
fn patterned_opto_stimulation() -> MmResult<()> {
    let scope = MiniCore::new();
    let camera = scope.camera("camera")?;
    let stimulation = scope.control("dmd_or_slm_stimulator")?;
    let recorder = scope.recorder("recorder")?;

    recorder.policy(RecordingPolicy::new("patterned_opto_stimulation"))?;
    camera.record_to(&recorder)?;

    let target = Roi {
        x: 100,
        y: 80,
        width: 24,
        height: 24,
    };
    let _stimulus = Stimulus::new(target, 0.4, Duration::from_millis(20));

    stimulation
        .arm_with(AcquisitionPlan::new("patterned_stimulus_timeline"))
        .submit()?;
    camera
        .start_sequence(500, Duration::from_millis(20))
        .submit()?
        .wait(Duration::from_secs(1))
}

// Spatial omics workflow: repeat fluidic reagent cycles while revisiting stage
// positions and recording images for later decoding or registration.
fn spatial_omics_round_trip() -> MmResult<()> {
    let scope = MiniCore::new();
    let camera = scope.camera("camera")?;
    let stage = scope.stage("xy_stage")?;
    let fluidics = scope.control("fluidics")?;
    let recorder = scope.recorder("recorder")?;

    recorder.policy(RecordingPolicy::new("optical_pooled_screening"))?;
    camera.record_to(&recorder)?;

    for cycle in 0..4 {
        fluidics
            .set_property("reagent_cycle", PropertyValue::from(cycle))
            .submit()?;
        stage.move_to_um(0.0).submit()?;
        camera
            .start_sequence(100, Duration::from_millis(50))
            .submit()?;
    }

    Ok(())
}

// Sparse trajectory shape: represent non-rectangular scan plans such as sparse
// optoacoustic rasters or adaptive ROI revisits.
fn sparse_scan_path_shape() {
    let _path = ScanPath::sparse(vec![
        Position {
            x_um: 0,
            y_um: 0,
            z_um: 0,
        },
        Position {
            x_um: 10,
            y_um: 40,
            z_um: 0,
        },
    ]);
}
