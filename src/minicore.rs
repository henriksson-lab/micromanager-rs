use crate::error::MmResult;
use crate::types::{DeviceType, PropertyValue};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceLabel(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OperationId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceHandle {
    pub label: DeviceLabel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceDescriptor {
    pub name: &'static str,
    pub description: &'static str,
    pub device_type: DeviceType,
    pub capabilities: Vec<Capability>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Capability {
    Camera(CameraCapability),
    Stage(StageCapability),
    Property(PropertyCapability),
    Trigger(TriggerCapability),
    Detector(DetectorCapability),
    Scan(ScanCapability),
    Storage(StorageCapability),
    Analysis(AnalysisCapability),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraCapability {
    pub can_snap: bool,
    pub can_sequence: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StageCapability {
    pub axis: StageAxis,
    pub lower_um: Option<f64>,
    pub upper_um: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyCapability {
    pub name: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerCapability {
    pub line: String,
    pub direction: TriggerDirection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectorCapability {
    pub stream: DataStreamKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanCapability {
    pub axes: Vec<ScanAxis>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageCapability {
    pub parallel_writes: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisCapability {
    pub kind: AnalysisKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageAxis {
    Z,
    XY,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerDirection {
    Input,
    Output,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataStreamKind {
    Image,
    Waveform,
    PhotonEvents,
    Metadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanAxis {
    X,
    Y,
    Z,
    Wavelength,
    Time,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisKind {
    Segmentation,
    FocusMetric,
    Reconstruction,
    Feedback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub role: DependencyRole,
    pub target: DeviceLabel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyRole {
    ParentHub,
    UsesCamera,
    UsesStage,
    TriggerSource,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeviceCommand {
    Initialize,
    Shutdown,
    GetProperty { name: String },
    SetProperty { name: String, value: PropertyValue },
    Snap,
    StartSequence { count: u64, interval: Duration },
    StopSequence,
    MoveStageToUm { position: f64 },
    MoveStageByUm { delta: f64 },
    Arm(AcquisitionPlan),
    Start,
    Stop,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CommandOutcome {
    Done,
    Property(PropertyValue),
    Frame(Frame),
    Waveform(Waveform),
    PhotonEvents(PhotonEvents),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Waveform {
    pub samples: Vec<f32>,
    pub sample_rate_hz: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhotonEvents {
    pub timestamps_ps: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceSnapshot {
    pub label: DeviceLabel,
    pub descriptor: DeviceDescriptor,
    pub initialized: bool,
    pub busy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationSnapshot {
    pub id: OperationId,
    pub device: DeviceLabel,
    pub status: OperationStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationStatus {
    Queued,
    Running,
    Done,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MiniCoreEvent {
    DeviceAdded(DeviceLabel),
    DeviceRemoved(DeviceLabel),
    OperationChanged(OperationSnapshot),
    FrameReady(DeviceLabel),
}

pub struct MiniCore;

pub struct DeviceContext;

pub struct EventStream;

pub struct Operation<T = CommandOutcome> {
    pub id: OperationId,
    _result: std::marker::PhantomData<T>,
}

pub struct PendingOperation<T = CommandOutcome> {
    _result: std::marker::PhantomData<T>,
}

pub struct CameraClient<'a> {
    _core: &'a MiniCore,
    _label: DeviceLabel,
}

pub struct StageClient<'a> {
    _core: &'a MiniCore,
    _label: DeviceLabel,
}

pub struct DeviceClient<'a> {
    _core: &'a MiniCore,
    _label: DeviceLabel,
}

pub struct TriggerClient<'a> {
    _core: &'a MiniCore,
    _label: DeviceLabel,
}

pub struct RecorderClient<'a> {
    _core: &'a MiniCore,
    _label: DeviceLabel,
}

pub struct AnalysisClient<'a> {
    _core: &'a MiniCore,
    _label: DeviceLabel,
}

pub struct Experiment<'a> {
    _core: &'a MiniCore,
    _name: String,
}

pub struct Action;

pub struct Workflow;

pub struct ImageRecorder;

pub struct ImageAnalysisService;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roi {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Position {
    pub x_um: i64,
    pub y_um: i64,
    pub z_um: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellFinding {
    pub roi: Roi,
    pub score: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcquisitionPlan {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingPolicy {
    pub dataset: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    pub entries: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScanPath {
    pub points: Vec<Position>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stimulus {
    pub target: Roi,
    pub power_fraction: f64,
    pub dwell: Duration,
}

pub trait MiniDevice: Send + 'static {
    fn descriptor(&self) -> DeviceDescriptor;
    fn initialize(&mut self, ctx: &mut DeviceContext) -> MmResult<()>;
    fn shutdown(&mut self, ctx: &mut DeviceContext) -> MmResult<()>;
    fn submit(
        &mut self,
        command: DeviceCommand,
        ctx: &mut DeviceContext,
    ) -> MmResult<CommandOutcome>;
}

impl MiniCore {
    pub fn new() -> Self {
        todo!()
    }

    pub fn add_device<D>(&mut self, label: impl Into<String>, device: D) -> MmResult<DeviceHandle>
    where
        D: MiniDevice,
    {
        let _ = (label.into(), device);
        todo!()
    }

    pub fn add_dependency(
        &mut self,
        device: impl Into<String>,
        dependency: Dependency,
    ) -> MmResult<()> {
        let _ = (device.into(), dependency);
        todo!()
    }

    pub fn initialize(&mut self, label: impl Into<String>) -> MmResult<Operation<()>> {
        let _ = label.into();
        todo!()
    }

    pub fn shutdown(&mut self, label: impl Into<String>) -> MmResult<Operation<()>> {
        let _ = label.into();
        todo!()
    }

    pub fn device(&self, label: impl Into<String>) -> MmResult<DeviceSnapshot> {
        let _ = label.into();
        todo!()
    }

    pub fn control(&self, label: impl Into<String>) -> MmResult<DeviceClient<'_>> {
        let _ = label.into();
        todo!()
    }

    pub fn camera(&self, label: impl Into<String>) -> MmResult<CameraClient<'_>> {
        let _ = label.into();
        todo!()
    }

    pub fn stage(&self, label: impl Into<String>) -> MmResult<StageClient<'_>> {
        let _ = label.into();
        todo!()
    }

    pub fn trigger(&self, label: impl Into<String>) -> MmResult<TriggerClient<'_>> {
        let _ = label.into();
        todo!()
    }

    pub fn recorder(&self, label: impl Into<String>) -> MmResult<RecorderClient<'_>> {
        let _ = label.into();
        todo!()
    }

    pub fn analysis(&self, label: impl Into<String>) -> MmResult<AnalysisClient<'_>> {
        let _ = label.into();
        todo!()
    }

    pub fn experiment(&self, name: impl Into<String>) -> Experiment<'_> {
        let _ = name.into();
        todo!()
    }

    pub fn events(&self) -> EventStream {
        todo!()
    }
}

impl Dependency {
    pub fn parent_hub(label: impl Into<String>) -> Self {
        let _ = label.into();
        todo!()
    }

    pub fn uses_camera(label: impl Into<String>) -> Self {
        let _ = label.into();
        todo!()
    }

    pub fn uses_stage(label: impl Into<String>) -> Self {
        let _ = label.into();
        todo!()
    }

    pub fn trigger_source(label: impl Into<String>) -> Self {
        let _ = label.into();
        todo!()
    }
}

impl ImageRecorder {
    pub fn new() -> Self {
        todo!()
    }
}

impl ImageAnalysisService {
    pub fn new() -> Self {
        todo!()
    }
}

impl<T> PendingOperation<T> {
    pub fn submit(self) -> MmResult<Operation<T>> {
        let _ = self;
        todo!()
    }
}

impl<T> Operation<T> {
    pub fn id(&self) -> OperationId {
        todo!()
    }

    pub fn snapshot(&self) -> MmResult<OperationSnapshot> {
        todo!()
    }

    pub fn cancel(&self) -> MmResult<()> {
        todo!()
    }
}

impl<T> Operation<T> {
    pub fn wait(self, timeout: Duration) -> MmResult<T> {
        let _ = timeout;
        todo!()
    }
}

impl<'a> CameraClient<'a> {
    pub fn snap(&self) -> PendingOperation<Frame> {
        todo!()
    }

    pub fn snap_with(&self, plan: AcquisitionPlan) -> PendingOperation<Frame> {
        let _ = plan;
        todo!()
    }

    pub fn start_sequence(&self, count: u64, interval: Duration) -> PendingOperation<()> {
        let _ = (count, interval);
        todo!()
    }

    pub fn stop_sequence(&self) -> PendingOperation<()> {
        todo!()
    }

    pub fn set_exposure(&self, exposure: Duration) -> PendingOperation<()> {
        let _ = exposure;
        todo!()
    }

    pub fn record_to(&self, recorder: &RecorderClient<'_>) -> MmResult<()> {
        let _ = recorder;
        todo!()
    }
}

impl<'a> StageClient<'a> {
    pub fn move_to_um(&self, position: f64) -> PendingOperation<()> {
        let _ = position;
        todo!()
    }

    pub fn move_by_um(&self, delta: f64) -> PendingOperation<()> {
        let _ = delta;
        todo!()
    }
}

impl<'a> DeviceClient<'a> {
    pub fn set_property(
        &self,
        name: impl Into<String>,
        value: PropertyValue,
    ) -> PendingOperation<()> {
        let _ = (name.into(), value);
        todo!()
    }

    pub fn arm(&self) -> PendingOperation<()> {
        todo!()
    }

    pub fn arm_with(&self, plan: AcquisitionPlan) -> PendingOperation<()> {
        let _ = plan;
        todo!()
    }

    pub fn start(&self) -> PendingOperation<()> {
        todo!()
    }

    pub fn stop(&self) -> PendingOperation<()> {
        todo!()
    }
}

impl<'a> TriggerClient<'a> {
    pub fn on_rising_edge(&self, action: Action) -> Workflow {
        let _ = action;
        todo!()
    }
}

impl<'a> RecorderClient<'a> {
    pub fn policy(&self, policy: RecordingPolicy) -> MmResult<()> {
        let _ = policy;
        todo!()
    }

    pub fn attach(&self, camera: &CameraClient<'_>) -> MmResult<()> {
        let _ = camera;
        todo!()
    }
}

impl<'a> AnalysisClient<'a> {
    pub fn find_dividing_cells(&self, frame: &Frame) -> MmResult<Vec<CellFinding>> {
        let _ = frame;
        todo!()
    }
}

impl<'a> Experiment<'a> {
    pub fn run(self) -> MmResult<()> {
        todo!()
    }
}

impl Action {
    pub fn snap(camera: &CameraClient<'_>) -> Self {
        let _ = camera;
        todo!()
    }

    pub fn record_to(self, recorder: &RecorderClient<'_>) -> Self {
        let _ = recorder;
        todo!()
    }
}

impl Workflow {
    pub fn arm(self) -> MmResult<Operation<()>> {
        todo!()
    }
}

impl RecordingPolicy {
    pub fn new(dataset: impl Into<String>) -> Self {
        let _ = dataset.into();
        todo!()
    }
}

impl AcquisitionPlan {
    pub fn new(name: impl Into<String>) -> Self {
        let _ = name.into();
        todo!()
    }
}

impl ScanPath {
    pub fn raster(width: u32, height: u32) -> Self {
        let _ = (width, height);
        todo!()
    }

    pub fn sparse(points: Vec<Position>) -> Self {
        let _ = points;
        todo!()
    }

    pub fn line(points: Vec<Position>) -> Self {
        let _ = points;
        todo!()
    }
}

impl Stimulus {
    pub fn new(target: Roi, power_fraction: f64, dwell: Duration) -> Self {
        let _ = (target, power_fraction, dwell);
        todo!()
    }
}

impl MiniDevice for ImageRecorder {
    fn descriptor(&self) -> DeviceDescriptor {
        todo!()
    }

    fn initialize(&mut self, ctx: &mut DeviceContext) -> MmResult<()> {
        let _ = ctx;
        todo!()
    }

    fn shutdown(&mut self, ctx: &mut DeviceContext) -> MmResult<()> {
        let _ = ctx;
        todo!()
    }

    fn submit(
        &mut self,
        command: DeviceCommand,
        ctx: &mut DeviceContext,
    ) -> MmResult<CommandOutcome> {
        let _ = (command, ctx);
        todo!()
    }
}

impl MiniDevice for ImageAnalysisService {
    fn descriptor(&self) -> DeviceDescriptor {
        todo!()
    }

    fn initialize(&mut self, ctx: &mut DeviceContext) -> MmResult<()> {
        let _ = ctx;
        todo!()
    }

    fn shutdown(&mut self, ctx: &mut DeviceContext) -> MmResult<()> {
        let _ = ctx;
        todo!()
    }

    fn submit(
        &mut self,
        command: DeviceCommand,
        ctx: &mut DeviceContext,
    ) -> MmResult<CommandOutcome> {
        let _ = (command, ctx);
        todo!()
    }
}

impl Iterator for EventStream {
    type Item = MiniCoreEvent;

    fn next(&mut self) -> Option<Self::Item> {
        todo!()
    }
}
