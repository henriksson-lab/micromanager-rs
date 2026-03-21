# claude-micromanager

A pure-Rust port of [MicroManager](https://micro-manager.org/) (`mmCoreAndDevices`). No C FFI, no Java bindings — Rust API only.

The port is based on https://github.com/micro-manager/mmCoreAndDevices, hash 67fe60267bc8d95554369d7fa42912775588e538

The license follows from the original code. To simplify Rust integration, the core code will be replaced, and also made
to be less monolithic while at it.

## Structure

```
claude-micromanager/
├── mm-device/          # Trait definitions (replaces MMDevice/)
├── mm-core/            # Engine: device manager, config, circular buffer (replaces MMCore/)
└── adapters/           # Hardware adapters, one crate per device family
```

### `mm-device`

Defines the core abstractions:

- **Traits** — `Device`, `Camera`, `Stage`, `XYStage`, `Shutter`, `StateDevice`, `VolumetricPump`, `Hub`, and more
- **`PropertyMap`** — typed property storage with allowed-value constraints
- **`Transport`** — serial communication abstraction (`send_recv`, `send_bytes`, `receive_bytes`) + `MockTransport` for unit tests
- **Error types**, **`PropertyValue`**, **`DeviceType`**, **`FocusDirection`**

### `mm-core`

The `CMMCore` engine:

- **`DeviceManager`** — load/unload/dispatch to typed device handles
- **`AdapterRegistry`** — static registration via the `inventory` crate
- **`CircularBuffer`** — fixed-size ring buffer for image sequence acquisition
- **`Config`** / config-file load/save

### Adapters

113 adapter crates — all pure serial, no vendor SDKs required (except the feature-gated SDK wrappers noted below). See [`ADAPTERS.md`](ADAPTERS.md) for the full list with status and notes.

Selected adapters:

| Crate | Device(s) | Protocol |
|---|---|---|
| `mm-adapter-demo` | DemoCamera, DemoStage, DemoShutter | Simulated |
| `mm-adapter-arduino` | Arduino shutter/state | ASCII `\r` |
| `mm-adapter-asi-stage` | ASI XY + Z stage | `:A`/`:N` ASCII |
| `mm-adapter-asi-tiger` | ASI Tiger XY + Z stage | `:A`/`:N` ASCII, 115200 baud |
| `mm-adapter-cobolt` | Cobolt diode laser | ASCII `\r` |
| `mm-adapter-coherent-obis` | Coherent OBIS laser | ASCII `\r` |
| `mm-adapter-conix` | Conix filter cubes, XY + Z stage | `:A`/`:N` ASCII |
| `mm-adapter-corvus` | Corvus XY + Z stage | ASCII space-terminated |
| `mm-adapter-csuw1` | Yokogawa CSU-W1 spinning disk | CSV ASCII `\r` |
| `mm-adapter-elliptec` | Thorlabs Elliptec linear stage + slider | Hex-position `\r` |
| `mm-adapter-hamilton-mvp` | Hamilton MVP valve positioner | `0x06` ACK binary |
| `mm-adapter-leica-dmi` | Leica DMI inverted microscope | ASCII `\r` |
| `mm-adapter-leica-dmr` | Leica DMR upright microscope | ASCII `\r` |
| `mm-adapter-ludl` | Ludl BioPrecision XY + Z, filter wheel, shutter | `:A` ASCII |
| `mm-adapter-marzhauser` | Märzhäuser TANGO XY + Z stage | ASCII `\r` |
| `mm-adapter-nikon` | Nikon ZStage, TIRFShutter, Ti-TIRFShutter, IntensiLight | ASCII `\r` / `\n` |
| `mm-adapter-omicron` | Omicron PhoxX/LuxX/BrixX laser | `?CMD`/`!CMD` hex `\r` |
| `mm-adapter-pi-gcs` | PI GCS Z-stage (CONEX, C-863, etc.) | `SVO`/`MOV`/`POS?` ASCII `\n` |
| `mm-adapter-prior` | Prior ProScan XY + Z, filter wheel, shutter | ASCII `\r` |
| `mm-adapter-scientifica` | Scientifica XY + Z stage | ASCII `\r` |
| `mm-adapter-sutter-lambda` | Sutter Lambda filter wheel | Binary |
| `mm-adapter-sutter-stage` | Sutter MP-285 XY + Z stage | `:A` ASCII |
| `mm-adapter-thorlabs-fw` | Thorlabs filter wheel | ASCII `\r` |
| `mm-adapter-zaber` | Zaber linear + XY stage | ASCII `\n` (Zaber ASCII v2) |
| `mm-adapter-zeiss-can` | Zeiss CAN-bus: FocusStage, MCU28 XY, turrets, shutter | Hex-encoded `\r`, 9600 baud |
| `mm-adapter-basler` | Basler cameras (feature-gated) | Pylon SDK; `--features basler` |
| `mm-adapter-andor-sdk3` | Andor sCMOS cameras (feature-gated) | SDK3 atcore; `--features andor-sdk3` |
| `mm-adapter-iidc` | FireWire IIDC cameras (feature-gated) | libdc1394; `--features iidc` |

## Building

```sh
cargo build --workspace
```

## Testing

```sh
cargo test --workspace
```

All adapters have unit tests that run against a `MockTransport` — no hardware required.

## Adding an Adapter

1. Create `adapters/mm-adapter-<name>/` with a `Cargo.toml` depending on `mm-device`.
2. Implement `Device` (and the appropriate device-type trait) for your struct.
3. Embed a `PropertyMap` and `Option<Box<dyn Transport>>`.
4. Add the crate to the workspace `Cargo.toml`.
5. Write tests using `MockTransport`.

Minimal example (`Cargo.toml`):

```toml
[package]
name = "mm-adapter-mydevice"
version = "0.1.0"
edition = "2021"

[dependencies]
mm-device = { path = "../../mm-device" }
```

Minimal struct pattern:

```rust
use mm_device::{error::MmResult, property::PropertyMap, traits::Device,
                transport::Transport, types::{DeviceType, PropertyValue}};

pub struct MyDevice {
    props: PropertyMap,
    transport: Option<Box<dyn Transport>>,
}

impl MyDevice {
    pub fn new() -> Self { /* define properties */ todo!() }
    pub fn with_transport(mut self, t: Box<dyn Transport>) -> Self {
        self.transport = Some(t); self
    }
}

impl Device for MyDevice {
    fn name(&self) -> &str { "MyDevice" }
    fn description(&self) -> &str { "My serial device" }
    fn initialize(&mut self) -> MmResult<()> { todo!() }
    fn shutdown(&mut self) -> MmResult<()> { Ok(()) }
    fn get_property(&self, name: &str) -> MmResult<PropertyValue> { self.props.get(name).cloned() }
    fn set_property(&mut self, name: &str, val: PropertyValue) -> MmResult<()> { self.props.set(name, val) }
    fn property_names(&self) -> Vec<String> { self.props.property_names().to_vec() }
    fn has_property(&self, name: &str) -> bool { self.props.has_property(name) }
    fn is_property_read_only(&self, name: &str) -> bool { false }
    fn device_type(&self) -> DeviceType { DeviceType::Generic }
    fn busy(&self) -> bool { false }
}
```
