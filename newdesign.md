# Future MiniCore, Device Graph, and Hub Design

We should keep the translated `CMMCore`/`core.rs` as a compatibility layer for
existing Micro-Manager-style devices and configs. It should remain useful, but
we should avoid making it responsible for every future orchestration problem.

For the future design, add a smaller **MiniCore**: a device substrate that
tracks devices, capabilities, dependencies, and runtime lanes, while leaving
experiment orchestration to a higher-level framework.

The main issues to solve are:

- Devices that depend on hubs, shared controllers, or joint physical hardware.
- Blocking vendor SDK or serial calls that may take hundreds of milliseconds or
  seconds.
- High-speed acquisition and hardware timing without spawning excessive OS
  processes.
- Hardware triggers, TTL/DAC sequencing, and device-side timed programs.
- Autofocus devices that may depend on a camera, focus stage, objective turret,
  hub, or another autofocus device.
- Laser scanning/confocal systems, which may expose cameras, scanners, galvos,
  shutters, lasers, filter wheels, waveform generators, and timing controllers
  rather than one monolithic device.

Current Rust `Parent` config lines are only metadata. They are parsed, stored,
and written back, but they do not bind a child device to a runtime hub or shared
transport. That is not enough for hub-dependent devices.

## Recommendation

Keep the current `CMMCore` mostly as-is for backwards compatibility. Add a new
MiniCore with an explicit device DAG, hub-owned/shared contexts, capability
discovery, and a bounded execution runtime.

Hubs should own controller resources such as serial ports, SDK sessions, bus
state, device discovery results, and shared locks. Child devices should be
ordinary devices, but they must be bound to a parent hub before `initialize()`.

Not all dependencies are parent-child relationships. Some devices are composed
from other independently loaded devices. Autofocus is the clearest example: a
software autofocus routine may depend on any camera and any focus stage, while a
hardware autofocus offset stage may depend on an autofocus device. The runtime
therefore needs a DAG, not only hub parent labels.

This preserves Micro-Manager's device split while avoiding raw pointers and
global singleton controller state.

## Layer Split

| Layer | Responsibility | Non-goals |
|---|---|---|
| `CMMCore` compatibility | Preserve translated MMCore behavior, config parsing, legacy roles, and existing adapter expectations | Do not grow into the universal trigger/autofocus/confocal orchestrator |
| `MiniCore` | Track devices, labels, types, DAG edges, capabilities, properties, runtime lanes, and primitive operations | Do not decide experiment strategy |
| Orchestration framework | Build acquisition plans, timing graphs, autofocus workflows, confocal/light-sheet scans, and recovery policies | Do not own low-level driver translation details |
| UI / external API | Present workflows and user-facing controls | Do not encode hardware-specific driver logic |

MiniCore should be small enough that it can also be used outside this crate by a
higher-level microscope framework. Its job is to make orchestration possible,
not to be the orchestration policy.

## Execution Model

Device drivers should expose a mostly synchronous, faithful hardware API at the
low level, but MiniCore should not let slow calls block unrelated devices or the
control UI.

Use a bounded in-process runtime:

- One worker lane per exclusive hardware resource, not one process per device.
- Blocking SDK calls run on the lane that owns that SDK handle.
- Serial buses run on a bus lane that serializes commands and protects response
  ordering.
- Cameras may have acquisition lanes for frame callbacks or blocking waits.
- Fast metadata/property reads can be cached when the hardware protocol supports
  it, but explicit live reads should remain available.
- Long commands return an operation handle when useful, with `busy()`,
  cancellation where possible, timeout, and completion/error reporting.

This keeps the process count low while still isolating blocking calls. Separate
processes should be reserved for crash-prone vendor SDKs, incompatible runtime
dependencies, or hard real-time helper programs that cannot safely share the
main process.

## Blocking Calls

Some SDKs and serial protocols only provide blocking calls. The driver should
not pretend those are nonblocking. Instead:

- Keep the driver operation faithful: if the SDK blocks, the worker lane blocks.
- Bound blocking with configured timeouts where the API allows it.
- Represent long-running commands as operations at the core layer.
- Use `busy()` and operation state for move/acquire/search progress.
- Keep other devices usable by scheduling them on other independent lanes.

The key separation is between **driver semantics** and **runtime scheduling**.
Drivers can remain simple and faithful; the runtime decides where blocking work
runs.

## Options

| Option | Shape | Pros | Cons | Fit |
|---|---|---|---|---|
| Keep `CMMCore` only | Put all future behavior into current `core.rs` | One API; maximal MM familiarity | Trigger/autofocus/confocal orchestration will make it large and rigid | Avoid for new design |
| Replace `CMMCore` entirely | Clean slate runtime | No legacy constraints | Breaks translated devices/configs and loses useful compatibility work | Avoid |
| `CMMCore` + MiniCore | Compatibility core stays stable; MiniCore becomes the device/capability substrate | Preserves old behavior while enabling better architecture | Two APIs to maintain | Recommended |
| MiniCore + external orchestration | MiniCore exposes primitives; higher layer builds timing/acquisition plans | Best fit for triggers and complex workflows | Requires clear capability model | Recommended |
| Label-only parent metadata | Keep `Parent,child,hub` as saved config only | Simple and MM config-compatible | Does not make hub-dependent devices work; children cannot share transport/state | Only for backward config parsing |
| Core-mediated binding | Core stores graph; before child init, calls `child.bind_parent(parent_handle)` | Closest to upstream `GetParentHub()`; preserves separately loaded devices; supports config replay | Needs typed/downcast-safe binding API; borrow/locking design must be careful | Good compatibility path |
| Hub factory/session | Load hub, then hub creates bound child instances from descriptors | Clean Rust ownership; hub owns shared transport/state; children are born connected | Less like old MM manual loading; config import needs mapping child labels to descriptors | Best new-core model |
| Shared bus service registry | Core has named services like serial buses, controller contexts, event streams; devices request capabilities | Generalizes beyond hubs; works for shared triggers, serial buses, CAN, SDK sessions | More abstraction; easy to overbuild | Good long-term substrate |
| Device DAG | Devices declare dependencies on hubs, cameras, stages, autofocus devices, trigger sources, or timing controllers | Handles autofocus/composite devices and non-hub dependencies | More complex config validation and initialization order | Required long-term |
| Flatten devices into one composite | One device exposes all axes/wheels/channels as properties | Very simple runtime; no graph needed | Loses MM device roles, labels, configs, per-child busy/state; bad for translation fidelity | Avoid except tiny hardware |
| Global module singleton | Adapter module owns global hub state, children access it | Mimics some old upstream code; quick port | Bad ownership, test isolation, multiple controllers, and thread safety | Avoid |

## Proposed Layers

| MiniCore Layer | Responsibility |
|---|---|
| `DeviceId` / `DeviceLabel` graph | Store dependency edges, roles, parent-child edges, and initialization order |
| `DetectedChild` descriptor | Store `device_name`, `label_hint`, `device_type`, `parent_label`, and address/channel/axis metadata |
| `HubContext` | Hub-owned shared transport/state, hidden behind typed APIs |
| `HubChild` hook | Bind a child to its parent context before `initialize()` |
| `RuntimeLane` | Own one exclusive SDK handle, serial bus, camera stream, or controller resource |
| `Operation` | Represent long-running move/acquire/focus/search commands with timeout and completion state |
| `TriggerEndpoint` / capabilities | Describe trigger inputs/outputs, camera trigger modes, sequence support, clocks, and constraints |
| Dependency initialization | Initialize hubs before children; reject children whose required parent is missing or uninitialized |
| Busy composition | Child busy is local delay/pending operation OR controller-reported busy; hub is not globally busy unless hardware requires it |

The higher-level orchestration layer owns the actual `TimingGraph`: which
devices participate, which trigger line is master, what gets armed first, and
what recovery policy applies if arming or acquisition fails.

## Concrete API Sketch

```rust
pub struct DetectedChild {
    pub device_name: String,
    pub label_hint: Option<String>,
    pub device_type: DeviceType,
    pub parent_label: String,
    pub metadata: ChildMetadata,
}

pub enum ChildMetadata {
    None,
    Address(String),
    Axis { card: Option<u8>, axis: String },
    Channel(u32),
    KeyValues(std::collections::BTreeMap<String, String>),
}

pub trait Hub: Device {
    fn detect_installed_devices(&mut self) -> MmResult<Vec<DetectedChild>>;
    fn create_child(&mut self, child: &DetectedChild) -> MmResult<AnyDevice>;
}

pub trait HubChild: Device {
    fn bind_parent(&mut self, parent: ParentHandle, metadata: ChildMetadata) -> MmResult<()>;
    fn parent_label(&self) -> Option<&str>;
}

pub struct DeviceDependency {
    pub role: DependencyRole,
    pub label: String,
    pub required: bool,
}

pub enum DependencyRole {
    ParentHub,
    Camera,
    FocusStage,
    XYStage,
    AutoFocus,
    ObjectiveTurret,
    TriggerSource,
    TimingController,
    SharedService(String),
}
```

`ParentHandle` should not be a raw pointer. It should be a typed, cloneable
handle to hub-owned shared state, typically wrapping `Arc<Mutex<...>>` or a
message-passing actor handle. The concrete hub can expose only the operations
children need, rather than exposing the whole hub object.

## Triggers and Hardware Timing

Hardware triggering should not be modeled only as camera properties. In
Micro-Manager, much of this is exposed through property/device sequencing:
properties can be loaded with a sequence, sent to the device, armed, advanced by
TTL, and stopped.

The future interface should separate these concepts:

- Camera frame acquisition: start/stop capture and retrieve frames.
- Camera trigger configuration: internal, software, external edge, external
  exposure/duration, external start, trigger source, polarity, delay.
- Property/device sequencing: load values or positions, arm them, step by TTL
  or hardware clock, stop/clear.
- External timing devices: TTL pattern generators, DAC waveform generators,
  blanking outputs, trigger edges, polarity, event limits.
- Scanner/galvo sequences: polygons, point-and-fire, raster/vector programs,
  scan clocks, laser/camera timing outputs.

Use optional capabilities rather than bloating base traits:

```rust
pub trait TriggerConfig {
    fn trigger_capabilities(&self) -> TriggerCapabilities;
    fn set_trigger_config(&mut self, config: TriggerConfigSpec) -> MmResult<()>;
}

pub trait PropertySequence {
    fn is_sequenceable(&self, property: &str) -> bool;
    fn sequence_max_len(&self, property: &str) -> usize;
    fn load_property_sequence(&mut self, property: &str, values: &[PropertyValue]) -> MmResult<()>;
    fn start_property_sequence(&mut self, property: &str) -> MmResult<()>;
    fn stop_property_sequence(&mut self, property: &str) -> MmResult<()>;
}

pub trait SignalSequence {
    fn load_signal_sequence(&mut self, values: &[f64]) -> MmResult<()>;
    fn arm_signal_sequence(&mut self, trigger: TriggerEdge) -> MmResult<()>;
    fn stop_signal_sequence(&mut self) -> MmResult<()>;
}
```

Vendor-specific trigger properties should remain available as normal
properties. The typed trigger layer is for discovery, validation, and portable
high-level orchestration.

MiniCore should expose trigger/timing as data and primitive arm/start/stop
operations, not as a universal acquisition policy.

```rust
pub struct TriggerEndpoint {
    pub device: DeviceLabel,
    pub port: String,
    pub direction: TriggerDirection,
    pub signal: TriggerSignal,
    pub supported_edges: Vec<TriggerEdge>,
}

pub struct TimingCapability {
    pub external_trigger: bool,
    pub software_trigger: bool,
    pub property_sequence: bool,
    pub signal_sequence: bool,
    pub waveform_upload: bool,
    pub max_sequence_len: Option<usize>,
}
```

The orchestration layer can then build plans such as:

```rust
pub struct TimingPlan {
    pub master: TriggerEndpoint,
    pub routes: Vec<TriggerRoute>,
    pub armed_devices: Vec<DeviceLabel>,
    pub start_order: Vec<DeviceLabel>,
    pub stop_order: Vec<DeviceLabel>,
}
```

MiniCore validates the referenced devices and executes primitive operations.
The higher layer decides whether the acquisition is a triggered timelapse,
z-stack, FRAP experiment, light-sheet scan, confocal raster, autofocus loop, or
multi-camera acquisition.

## Autofocus

Autofocus needs first-class support, but it should not be collapsed into a focus
stage.

Hardware autofocus drivers should expose primitives:

- Continuous focusing on/off.
- Locked/in-focus state.
- Full/incremental focus search.
- Focus score.
- Offset get/set.
- Calibration/status/vendor properties.

Software or camera-based autofocus should be represented as a composed device or
higher-level routine with DAG dependencies:

- Any camera that can provide frames.
- Any focus stage or offset stage.
- Optional objective/turret metadata.
- Optional image analysis implementation.

The autofocus camera should still be a normal camera when possible, so users can
view it and debug image quality. The autofocus routine should consume frames
through the normal camera interface and report the derived focus error/score.

Add a core autofocus role next to camera, shutter, focus, and XY stage:

- `Core-AutoFocus` / `auto_focus_label` in `CMMCore` compatibility when needed.
- `AutoFocus` role/capability in MiniCore.
- Typed `AutoFocus` operations.
- Optional `AutoFocusStage` meta-device that maps stage position to autofocus
  offset, but do not treat this as the only autofocus representation.

## Laser Scanning and Confocal Systems

Confocal support is not one device class.

Spinning disk systems often look like ordinary serial devices:

- Filter wheels.
- Dichroics.
- Disk sliders.
- Shutters.
- Illumination controls.

Laser scanning and light-sheet systems often look like timing/scanning graphs:

- Galvos or scanner axes.
- Laser shutters/modulation outputs.
- Camera trigger outputs.
- TTL/DAC waveform generation.
- Raster/vector/polygon scan programs.
- Pinhole/slit/light-sheet timing parameters.

The base design should therefore support:

- `Galvo` plus scanner sequence capabilities.
- Timing controllers and trigger edges in the DAG.
- Device-side waveform/program upload.
- Coordinated arm/start/stop across camera, scanner, laser, and stage.
- SDK-backed opaque controllers when vendors expose only a monolithic API.

Do not assume there will be accessible SDKs. Many confocal systems are
proprietary or only expose a narrow serial/control surface. The design should
allow partial support that still exposes the real devices faithfully.

## Design Rules

- Keep upstream's device split. Do not flatten hubs unless upstream did.
- Make parent-child binding explicit and required for devices that need it.
- Model non-hub dependencies explicitly; the graph is a DAG, not only a tree.
- Initialize dependencies before dependents.
- Store parent/dependency labels in config, but use them to build the runtime
  graph.
- Let hubs own transport and controller discovery state.
- Let children own their device-role behavior, labels, state positions, and
  property surface.
- Avoid global module singletons. They break multiple-controller setups and make
  tests unreliable.
- Avoid raw pointers. Use typed handles, shared contexts, or message-passing.
- Model discovered children with descriptors, not just names. Many controllers
  need address, card, axis, channel, or slot metadata.
- Compose busy state at the child level unless the hardware protocol truly makes
  the whole hub busy.
- Prefer in-process worker lanes over extra processes. Add processes only for
  SDK isolation, crash containment, dependency conflicts, or hard real-time
  needs.
- Keep low-level drivers faithful and small; put cross-device orchestration in
  MiniCore services or higher-level APIs.
- Keep trigger/timing support as optional capabilities that can be discovered at
  runtime.
- Do not make MiniCore a second MMCore. It should expose inventory,
  capabilities, dependencies, operation scheduling, and event/frame streams.
- Put experiment-specific timing plans and acquisition state machines in the
  higher-level orchestration framework.

## Device Families This Should Cover

- Arduino and Arduino32 hub/child devices.
- ASI Tiger-style addressed controller cards and axes.
- Zeiss CAN controllers with many logical microscope devices.
- OpenUC2 and TriggerScope shared-controller devices.
- Universal serial hubs that discover typed subdevices.
- State devices with labels/configs such as wheels, turrets, dichroics, and
  shutters.
- Hardware autofocus devices such as CRISP, Nikon PFS, PureFocus, pgFocus, and
  ZDC-style systems.
- Camera-based autofocus routines that depend on any camera and any focus
  stage.
- TriggerScope/WOSM/Arduino-style TTL and DAC sequencers.
- Galvo/scanner/light-sheet systems such as ASI scanner/SPIM controllers.
- Spinning disk confocal devices such as Yokogawa, CSU-W1, X-Light, Diskovery,
  and CARVII.

## Migration Path

1. Keep current `CMMCore`/`core.rs` as the compatibility API.
2. Keep parsing and writing `Parent` config lines for compatibility.
3. Add a separate MiniCore module rather than expanding `CMMCore`.
4. Add runtime DAG storage to MiniCore.
5. Add dependency declarations for hubs, cameras, stages, autofocus devices,
   trigger sources, and timing controllers.
6. Add a `DetectedChild` descriptor type richer than `Vec<String>`.
7. Add hub-owned shared context and child binding APIs.
8. Add bounded in-process worker lanes for blocking SDK/serial calls.
9. Add operation handles for long moves, focus searches, acquisition starts, and
   other slow commands.
10. Add typed autofocus role and basic autofocus operations.
11. Add generic property/device sequence capabilities.
12. Add typed trigger configuration for cameras and TTL/DAC devices.
13. Add a minimal external orchestration prototype that consumes MiniCore
    capabilities and builds a timing plan.
14. Port one simple hub family first, ideally Arduino or Prizmatix.
15. Port one trigger sequencer family, ideally TriggerScope or Arduino switch
    sequences.
16. Port one autofocus family, ideally pgFocus or Prior PureFocus.
17. Port one addressed multi-device family next, ideally ASI Tiger or a
    UniversalHub-style adapter.
18. Only then migrate complex microscope hubs and scanner/confocal systems such
    as Zeiss CAN, ASI scanner/SPIM, or vendor SDK confocals.
