#![allow(dead_code)]

use micromanager::adapters::demo_async::{AsyncDemoCamera, AsyncDemoStage};
use micromanager::{
    AcquisitionPlan, Action, Dependency, ImageAnalysisService, ImageRecorder, MiniCore, MmResult,
    Position, PropertyValue, RecordingPolicy, Roi, ScanPath, Stimulus,
};
use std::time::Duration;

// This example file is a catalog of API sketches rather than a runnable demo.
// Each function captures an advanced workflow and the API pressure it creates:
// deterministic timing, feedback loops, non-image streams, metadata, storage,
// multimodal alignment, or safety-critical device coordination.
fn main() -> MmResult<()> {
    Ok(())
}

// Demonstrates hardware-triggered acquisition. The advanced part is that the
// user does not poll or call snap at the moment of exposure; MiniCore must arm a
// trigger graph where a TTL edge causes the camera action and routes the frame
// to storage with deterministic trigger metadata.
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

// Demonstrates closed-loop adaptive microscopy. The advanced part is that image
// analysis changes the future acquisition: survey at low resolution, detect a
// biological event, switch objective, move/focus, run a high-resolution
// timelapse, then return to the survey state.
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

// Demonstrates laser-scanning confocal orchestration. The advanced part is that
// a scan engine, pulsed laser, and TCSPC detector must share timing, and the
// acquisition plan can mix imaging, FLIM photon timing, and a FRAP bleach phase.
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

// Demonstrates optoacoustic/photoacoustic acquisition. The advanced part is
// that the primary detector is a DAQ waveform stream, not a camera; each laser
// pulse produces an A-line that must be tied to scanner position, wavelength,
// pulse energy, and reconstruction metadata.
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

// Demonstrates light-sheet volume acquisition. The advanced part is that camera
// exposure, sheet scan, laser power, and Z motion are one timed graph; a slow
// sequential API would introduce blur, missed planes, or excessive phototoxicity.
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

// Demonstrates adaptive optics feedback. The advanced part is the tight
// read-analyze-write loop: image quality metrics update a deformable mirror or
// SLM while the acquisition remains live and latency-sensitive.
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

// Demonstrates localization super-resolution. The advanced part is sustained
// high-rate streaming with synchronized activation/excitation control, sparse
// emitter density management, precise frame timestamps, and storage throughput.
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

// Demonstrates electrophysiology-coupled imaging. The advanced part is
// cross-modal timing: camera frames, analog DAQ traces, and stimulus or
// patch-clamp protocols need shared clocks, markers, and recoverable metadata.
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

// Demonstrates perturbation imaging with microfluidics. The advanced part is
// long-running device-state orchestration: valves, pumps, sensors, safety limits,
// and image metadata must remain aligned over hours.
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

// Demonstrates patterned optogenetic/photoactivation stimulation. The advanced
// part is independent stimulation and imaging paths: a DMD/SLM/galvo timeline
// targets ROIs while imaging continues with event annotations and laser safety.
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

// Demonstrates cyclic spatial-omics acquisition. The advanced part is repeated
// reagent cycles with stage revisits, autofocus, channel metadata, and durable
// bookkeeping so downstream decoding can align many imaging rounds.
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

// Demonstrates non-rectangular acquisition paths. The advanced part is that not
// all scans are dense raster images; adaptive microscopy, optoacoustics, and
// sparse clinical scans need arbitrary trajectories and skipped positions.
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

// Demonstrates intravital multiphoton imaging. The advanced part is combining
// resonant scanning, tunable femtosecond excitation, physiological telemetry,
// and behavior/stimulus markers while preserving timing for live-animal data.
fn intravital_multiphoton_behavior() -> MmResult<()> {
    let scope = MiniCore::new();
    let scanner = scope.control("resonant_multiphoton_scanner")?;
    let laser = scope.control("tunable_femtosecond_laser")?;
    let physiology = scope.control("physiology_logger")?;
    let behavior = scope.control("behavior_trigger_box")?;
    let recorder = scope.recorder("recorder")?;

    recorder.policy(RecordingPolicy::new("intravital_multiphoton"))?;
    laser
        .set_property("wavelength_nm", PropertyValue::from(920_i64))
        .submit()?;
    physiology
        .arm_with(AcquisitionPlan::new("heart_respiration_temperature"))
        .submit()?;
    behavior
        .arm_with(AcquisitionPlan::new("behavior_markers"))
        .submit()?;
    scanner
        .arm_with(AcquisitionPlan::new("fast_resonant_z_stack"))
        .submit()?;
    scanner.start().submit()?.wait(Duration::from_secs(1))
}

// Demonstrates MINFLUX-style tracking. The advanced part is not frame imaging
// but active beam placement plus photon-event counting; the control loop must
// steer a patterned excitation minimum based on sparse localization events.
fn minflux_single_molecule_tracking() -> MmResult<()> {
    let scope = MiniCore::new();
    let beam = scope.control("doughnut_beam_steering")?;
    let detector = scope.control("photon_counter")?;
    let activation = scope.control("activation_laser")?;
    let recorder = scope.recorder("recorder")?;

    recorder.policy(RecordingPolicy::new("minflux_tracking"))?;
    activation
        .set_property("density_mode", PropertyValue::from("single_molecule"))
        .submit()?;
    detector
        .arm_with(AcquisitionPlan::new("photon_event_stream"))
        .submit()?;
    beam.arm_with(AcquisitionPlan::new("minflux_probe_pattern"))
        .submit()?;
    beam.start().submit()?.wait(Duration::from_secs(1))
}

// Demonstrates Raman hyperspectral mapping. The advanced part is that each
// spatial point yields a spectrum rather than an image pixel; the API must align
// stage position, laser settings, exposure, and spectral metadata.
fn raman_hyperspectral_mapping() -> MmResult<()> {
    let scope = MiniCore::new();
    let laser = scope.control("raman_excitation_laser")?;
    let spectrometer = scope.control("spectrometer")?;
    let stage = scope.stage("xy_stage")?;
    let recorder = scope.recorder("recorder")?;

    recorder.policy(RecordingPolicy::new("raman_hyperspectral_cube"))?;
    laser
        .set_property("wavelength_nm", PropertyValue::from(785_i64))
        .submit()?;
    spectrometer
        .arm_with(AcquisitionPlan::new("spectrum_per_point"))
        .submit()?;
    for x in [0.0, 10.0, 20.0] {
        stage.move_to_um(x).submit()?;
    }
    spectrometer.start().submit()?.wait(Duration::from_secs(1))
}

// Demonstrates cryo-CLEM targeting. The advanced part is correlative metadata:
// fluorescence targets on a cryo grid must be exported in coordinate systems
// suitable for later cryo-EM or cryo-ET acquisition.
fn cryo_clem_target_picking() -> MmResult<()> {
    let scope = MiniCore::new();
    let camera = scope.camera("cryo_fluorescence_camera")?;
    let cryo_stage = scope.stage("cryo_grid_stage")?;
    let analysis = scope.analysis("target_picker")?;
    let recorder = scope.recorder("recorder")?;

    recorder.policy(RecordingPolicy::new("cryo_clem_targets"))?;
    for grid_square in [0.0, 1_000.0, 2_000.0] {
        cryo_stage.move_to_um(grid_square).submit()?;
        let frame = camera.snap().submit()?.wait(Duration::from_secs(1))?;
        let _targets = analysis.find_dividing_cells(&frame)?;
    }
    Ok(())
}

// Demonstrates expansion microscopy tiling. The advanced part is very large
// fields of view and registration metadata: expanded samples need stitched
// tile/volume acquisition with expansion factors preserved in the dataset.
fn expansion_microscopy_tiled_volume() -> MmResult<()> {
    let scope = MiniCore::new();
    let camera = scope.camera("widefield_or_confocal_camera")?;
    let xy = scope.stage("tile_stage")?;
    let z = scope.stage("z_drive")?;
    let recorder = scope.recorder("recorder")?;

    recorder.policy(RecordingPolicy::new("expanded_sample_tiles"))?;
    camera.record_to(&recorder)?;
    for tile_um in [0.0, 500.0, 1_000.0] {
        xy.move_to_um(tile_um).submit()?;
        z.move_by_um(100.0).submit()?;
        camera
            .start_sequence(50, Duration::from_millis(20))
            .submit()?;
    }
    Ok(())
}

// Demonstrates OCT angiography. The advanced part is repeated B-scan timing:
// flow contrast comes from decorrelation across repeated scans, so scan order
// and interferogram streams must be represented explicitly.
fn oct_angiography_repeated_b_scans() -> MmResult<()> {
    let scope = MiniCore::new();
    let oct_engine = scope.control("oct_engine")?;
    let scanner = scope.control("fast_axis_scanner")?;
    let recorder = scope.recorder("recorder")?;

    recorder.policy(RecordingPolicy::new("oct_angiography"))?;
    scanner
        .arm_with(AcquisitionPlan::new("repeated_b_scan_positions"))
        .submit()?;
    oct_engine
        .arm_with(AcquisitionPlan::new("spectral_interferograms"))
        .submit()?;
    oct_engine.start().submit()?.wait(Duration::from_secs(1))
}

// Demonstrates FRET biosensor imaging. The advanced part is ratio integrity:
// donor, acceptor, and FRET channels need matched timing, exposure, and metadata
// so downstream analysis can compute meaningful ratio maps.
fn fret_ratio_biosensor_timelapse() -> MmResult<()> {
    let scope = MiniCore::new();
    let camera = scope.camera("camera")?;
    let filter = scope.control("emission_filter_wheel")?;
    let excitation = scope.control("excitation_selector")?;
    let recorder = scope.recorder("recorder")?;

    recorder.policy(RecordingPolicy::new("fret_ratio_timelapse"))?;
    camera.record_to(&recorder)?;
    for channel in ["donor", "acceptor", "fret"] {
        excitation
            .set_property("channel", PropertyValue::from(channel))
            .submit()?;
        filter
            .set_property("channel", PropertyValue::from(channel))
            .submit()?;
        camera.snap().submit()?;
    }
    Ok(())
}

// Demonstrates MERFISH/seqFISH-style cyclic imaging. The advanced part is
// experiment state management across many chemistry rounds; imaging, autofocus,
// fluidics, and cycle metadata must survive interruptions and support decoding.
fn cyclic_spatial_transcriptomics() -> MmResult<()> {
    let scope = MiniCore::new();
    let camera = scope.camera("camera")?;
    let fluidics = scope.control("fluidics")?;
    let autofocus = scope.control("hardware_autofocus")?;
    let recorder = scope.recorder("recorder")?;

    recorder.policy(RecordingPolicy::new("cyclic_spatial_transcriptomics"))?;
    camera.record_to(&recorder)?;
    for round in 0..16 {
        fluidics
            .set_property("hybridization_round", PropertyValue::from(round))
            .submit()?;
        autofocus.arm().submit()?;
        camera
            .start_sequence(4, Duration::from_millis(100))
            .submit()?;
    }
    Ok(())
}

// Demonstrates cleared-tissue mesoscopy. The advanced part is scale: large
// multi-tile, multi-depth datasets stress stage scheduling, illumination
// stability, metadata volume, and parallel storage throughput.
fn cleared_tissue_mesoscope_volume() -> MmResult<()> {
    let scope = MiniCore::new();
    let camera = scope.camera("large_sensor_camera")?;
    let stage = scope.stage("sample_stage")?;
    let illumination = scope.control("mesoscope_illumination")?;
    let recorder = scope.recorder("recorder")?;

    recorder.policy(RecordingPolicy::new("cleared_tissue_volume"))?;
    camera.record_to(&recorder)?;
    illumination.arm().submit()?;
    for position in [0.0, 2_000.0, 4_000.0] {
        stage.move_to_um(position).submit()?;
        camera
            .start_sequence(300, Duration::from_millis(15))
            .submit()?;
    }
    Ok(())
}

// Demonstrates AFM-correlative microscopy. The advanced part is multimodal
// registration: optical frames and AFM force maps must share coordinates,
// timing, and metadata even though one stream is image-like and one is not.
fn afm_correlative_force_mapping() -> MmResult<()> {
    let scope = MiniCore::new();
    let camera = scope.camera("fluorescence_camera")?;
    let afm = scope.control("atomic_force_microscope")?;
    let stage = scope.stage("sample_stage")?;
    let recorder = scope.recorder("recorder")?;

    recorder.policy(RecordingPolicy::new("afm_correlative_force_maps"))?;
    camera.record_to(&recorder)?;
    for position in [0.0, 50.0, 100.0] {
        stage.move_to_um(position).submit()?;
        camera.snap().submit()?;
        afm.arm_with(AcquisitionPlan::new("force_volume"))
            .submit()?;
        afm.start().submit()?;
    }
    Ok(())
}
