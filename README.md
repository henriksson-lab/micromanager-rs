# micromanager-rs

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

## Demo

`mm-demo` is a live-preview app that uses `CMMCore` with the simulated `DemoCamera`. It opens a window and continuously snaps frames at ~30 fps.

```sh
cargo run -p mm-demo
```

The demo shows the minimal setup needed to drive a camera through the Rust API:

```rust
let mut core = CMMCore::new();
core.register_adapter(Box::new(DemoAdapter));
core.load_device("Camera", "demo", "DCamera").unwrap();
core.initialize_device("Camera").unwrap();
core.set_camera_device("Camera").unwrap();
core.set_exposure(25.0).unwrap();

core.snap_image().unwrap();
let frame = core.get_image().unwrap();
// frame.data is &[u8] (GRAY8), frame.width / frame.height are pixel dimensions
```

The window renders GRAY8 pixels as RGB and closes on Esc.

### `mm-core`

The `CMMCore` engine:

- **`DeviceManager`** — load/unload/dispatch to typed device handles
- **`AdapterRegistry`** — static registration via the `inventory` crate
- **`CircularBuffer`** — fixed-size ring buffer for image sequence acquisition
- **`Config`** / config-file load/save

### Adapters

W = Windows, M = macOS, L = Linux. SDK-wrapped adapters are feature-gated; all others are pure serial with no vendor dependencies.

#### Implemented (113 crates)

| Crate | Devices | Protocol | W | M | L |
|---|---|---|:---:|:---:|:---:|
| `mm-adapter-aaaotf` | [Crystal Technology AOTF](https://micro-manager.org/AA_AOTF) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-aladdin` | [WPI Aladdin syringe pump](https://micro-manager.org/Aladdin) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-andor-sdk3` | [Andor sCMOS cameras](https://micro-manager.org/Andor_SDK3) | SDK3 atcore; `--features andor-sdk3` | ✓ | ✗ | ✓ |
| `mm-adapter-aquinas` | [Aquinas microfluidics controller](https://micro-manager.org/Aquinas) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-arduino` | [Arduino shutter / state device](https://micro-manager.org/Arduino) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-arduino-counter` | [Arduino pulse counter](https://micro-manager.org/Arduino_Counter) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-arduino32` | [32-bit Arduino boards](https://micro-manager.org/Arduino32bitBoards) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-asi-fw` | [ASI filter wheel](https://micro-manager.org/ASIFW1000) | `:A`/`:N` ASCII | ✓ | ✓ | ✓ |
| `mm-adapter-asi-stage` | [ASI XY + Z stage](https://micro-manager.org/ASIStage) | `:A`/`:N` ASCII | ✓ | ✓ | ✓ |
| `mm-adapter-asi-tiger` | [ASI Tiger controller (XY + Z)](https://micro-manager.org/ASITiger) | `:A`/`:N` ASCII, 115200 baud | ✓ | ✓ | ✓ |
| `mm-adapter-asi-wptr` | [ASI W-PTR serial device](https://micro-manager.org/ASIwptr) | ASCII | ✓ | ✓ | ✓ |
| `mm-adapter-asifw1000` | [ASI FW-1000 filter wheel + shutter](https://micro-manager.org/ASIFW1000) | Binary | ✓ | ✓ | ✓ |
| `mm-adapter-basler` | [Basler cameras](https://micro-manager.org/BaslerPylon) | Pylon SDK; `--features basler` | ✓ | ✓ | ✓ |
| `mm-adapter-carvii` | [BD/CrEST CARVII confocal (shutters, filter wheels, sliders)](https://micro-manager.org/CARVII) | Single-char ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-chuoseiki` | [ChuoSeiki MD-5000 XY stage](https://micro-manager.org/ChuoSeiki_MD5000) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-chuoseiki-qt` | [ChuoSeiki QT-series stages](https://micro-manager.org/ChuoSeiki_QT) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-cobolt` | [Cobolt diode laser](https://micro-manager.org/Cobolt) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-cobolt-official` | [Cobolt vendor-independent variant](https://micro-manager.org/CoboltOfficial) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-coherent-cube` | [Coherent CUBE laser](https://micro-manager.org/Coherent_Cube) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-coherent-obis` | [Coherent OBIS laser](https://micro-manager.org/CoherentOBIS) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-coherent-scientific-remote` | [Coherent Scientific Remote](https://micro-manager.org/Coherent_Scientific_Remote) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-conix` | [Conix filter cubes, XY + Z stage](https://micro-manager.org/Conix) | `:A`/`:N` ASCII | ✓ | ✓ | ✓ |
| `mm-adapter-coolled` | [CoolLED pE-300 LED](https://micro-manager.org/CoolLED) | CSS format | ✓ | ✓ | ✓ |
| `mm-adapter-coolled-pe4000` | [CoolLED pE-4000 LED (4-channel)](https://micro-manager.org/CoolLED) | CSS format | ✓ | ✓ | ✓ |
| `mm-adapter-corvus` | [Corvus XY + Z stage](https://micro-manager.org/Corvus) | Space-terminated ASCII | ✓ | ✓ | ✓ |
| `mm-adapter-csuw1` | [Yokogawa CSU-W1 spinning disk](https://micro-manager.org/Yokogawa_CSUW1) | CSV ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-demo` | [DemoCamera, DemoStage, DemoShutter](https://micro-manager.org/DemoCamera) | Simulated | ✓ | ✓ | ✓ |
| `mm-adapter-diskovery` | [Intelligent Imaging Diskovery spinning disk](https://micro-manager.org/Diskovery) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-elliptec` | [Thorlabs Elliptec linear stage + 2-position slider](https://micro-manager.org/ThorlabsElliptecSlider) | Hex-position `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-esp32` | ESP32 Arduino controller | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-etl` | Electrically tunable lens | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-hamilton-mvp` | Hamilton MVP modular valve positioner | `0x06` ACK binary | ✓ | ✓ | ✓ |
| `mm-adapter-hydra-lmt200` | [Hydra LMT-200 motion controller](https://micro-manager.org/LMT200-V3) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-iidc` | [FireWire IIDC cameras](https://micro-manager.org/IIDC) | libdc1394; `--features iidc` | ✓ | ✗ | ✓ |
| `mm-adapter-illuminate-led` | Illuminate LED array | Serial + JSON | ✓ | ✓ | ✓ |
| `mm-adapter-ismatec` | Ismatec MCP peristaltic pump | Address-prefixed `*`-ACK | ✓ | ✓ | ✓ |
| `mm-adapter-jai` | [JAI cameras](https://micro-manager.org/JAI) | Pleora eBUS SDK; `--features jai` | ✓ | ✓ | ✓ |
| `mm-adapter-laser-quantum` | [Laser Quantum Gem laser](https://micro-manager.org/LaserQuantumLaser) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-ldi` | [89 North LDI LED illuminator](https://micro-manager.org/89NorthLDI) | ASCII `\n`, dynamic wavelengths | ✓ | ✓ | ✓ |
| `mm-adapter-leica-dmi` | [Leica DMI inverted microscope](https://micro-manager.org/LeicaDMI) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-leica-dmr` | [Leica DMR upright microscope](https://micro-manager.org/LeicaDMR) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-ludl` | [Ludl BioPrecision XY + Z, filter wheel, shutter](https://micro-manager.org/Ludl) | `:A` ASCII | ✓ | ✓ | ✓ |
| `mm-adapter-ludl-low` | [Low-level Ludl variant](https://micro-manager.org/LudlLow) | `:A` ASCII | ✓ | ✓ | ✓ |
| `mm-adapter-lumencor-cia` | [Lumencor CIA LED](https://micro-manager.org/LumencorCIA) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-lumencor-spectra` | [Lumencor Spectra/Aura/Sola LED (legacy)](https://micro-manager.org/LumencorSpectra) | Binary write-only | ✓ | ✓ | ✓ |
| `mm-adapter-marzhauser` | [Märzhäuser TANGO XY + Z stage](https://micro-manager.org/Marzhauser) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-marzhauser-lstep` | [Märzhäuser LStep variant](https://micro-manager.org/MarzhauserLStep) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-marzhauser-lstep-old` | [Märzhäuser LStep (older protocol)](https://micro-manager.org/MarzhauserLStepOld) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-microfpga` | [MicroFPGA FPGA controller](https://micro-manager.org/MicroFPGA) | USB serial | ✓ | ✓ | ✓ |
| `mm-adapter-mpb-laser` | MPB Communications fiber laser | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-neopixel` | [NeoPixel LED array](https://micro-manager.org/ArduinoNeoPixel) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-neos` | [Neos Technologies AO shutter](https://micro-manager.org/Neos) | No-response serial | ✓ | ✓ | ✓ |
| `mm-adapter-newport-stage` | [Newport CONEX-CC / SMC100 Z stage](https://micro-manager.org/NewportCONEX) | ASCII `\r\n` | ✓ | ✓ | ✓ |
| `mm-adapter-niji` | [BlueboxOptics niji 7-channel LED](https://micro-manager.org/BlueboxOptics_niji) | Binary sync + `\r\n` | ✓ | ✓ | ✓ |
| `mm-adapter-nikon` | [Nikon ZStage, TIRFShutter, Ti-TIRFShutter, IntensiLight](https://micro-manager.org/Nikon) | ASCII `\r`/`\n` | ✓ | ✓ | ✓ |
| `mm-adapter-omicron` | [Omicron PhoxX/LuxX/BrixX laser](https://micro-manager.org/Omicron) | `?CMD`/`!CMD` hex `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-opencv` | [OpenCV video capture (camera)](https://micro-manager.org/OpenCVgrabber) | OpenCV 4.x | ✓ | ✓ | ✓ |
| `mm-adapter-openflexure` | [OpenFlexure microscope stage](https://micro-manager.org/OpenFlexure) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-openuc2` | UC2 Arduino controller | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-oxxius` | [Oxxius L6Cc laser combiner](https://micro-manager.org/Oxxius_combiner) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-oxxius-laserboxx` | [Oxxius LaserBoxx single laser](https://micro-manager.org/Oxxius) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-pecon` | [Pecon TempControl 37-2 (temp + CO2)](https://micro-manager.org/Pecon) | Raw 3-byte BCD | ✓ | ✓ | ✓ |
| `mm-adapter-pgfocus` | [pgFocus open-source autofocus](https://micro-manager.org/pgFocus) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-pi-gcs` | [PI GCS Z-stage (C-863, CONEX, etc.)](https://micro-manager.org/PI_GCS) | `SVO`/`MOV`/`POS?` ASCII `\n` | ✓ | ✓ | ✓ |
| `mm-adapter-picam` | [Princeton Instruments / Photometrics cameras](https://micro-manager.org/PICAM) | PVCAM SDK; `--features picam` | ✓ | ✓ | ✓ |
| `mm-adapter-piezosystem-30dv50` | [Piezosystem Jena 30DV50](https://micro-manager.org/Piezosystem_30DV50) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-piezosystem-ddrive` | [Piezosystem Jena dDrive](https://micro-manager.org/Piezosystem_dDrive) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-piezosystem-nv120` | [Piezosystem Jena NV-120/1](https://micro-manager.org/Piezosystem_NV120_1) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-piezosystem-nv40-1` | [Piezosystem Jena NV-40/1](https://micro-manager.org/Piezosystem_NV40_1) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-piezosystem-nv40-3` | [Piezosystem Jena NV-40/3](https://micro-manager.org/Piezosystem_NV40_3) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-precis-excite` | [PrecisExcite LED illuminator](https://micro-manager.org/PrecisExcite) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-prior` | [Prior ProScan XY + Z, filter wheel, shutter](https://micro-manager.org/Prior) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-prior-legacy` | [Prior ProScan (legacy protocol)](https://micro-manager.org/Prior) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-prior-purefocus` | [Prior PureFocus autofocus](https://micro-manager.org/PriorPureFocus) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-prizmatix` | Prizmatix LED illuminator | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-sapphire` | [Coherent Sapphire laser](https://micro-manager.org/Sapphire) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-scientifica` | [Scientifica XY + Z stage](https://micro-manager.org/Scientifica) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-scientifica-motion8` | [Scientifica Motion8 variant](https://micro-manager.org/Scientifica) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-scopeled` | [ScopeLED illuminator](https://micro-manager.org/ScopeLED) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-spectral-lmm5` | [Spectral LMM5 laser combiner](https://micro-manager.org/SpectralLMM5) | Hex-encoded binary `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-spot` | [Diagnostic Instruments SpotCam](https://micro-manager.org/SpotCamera) | SpotCam SDK; `--features spot` | ✓ | ✓ | ✗ |
| `mm-adapter-sutter-lambda` | [Sutter Lambda 10-2/10-3 filter wheel](https://micro-manager.org/SutterLambda) | Binary | ✓ | ✓ | ✓ |
| `mm-adapter-sutter-lambda-arduino` | [Sutter Lambda + Arduino parallel](https://micro-manager.org/SutterLambda) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-sutter-lambda2` | [Sutter Lambda 2 (newer protocol)](https://micro-manager.org/SutterLambda2) | Binary | ✓ | ✓ | ✓ |
| `mm-adapter-sutter-stage` | [Sutter MP-285 / MPC-200 XY + Z](https://micro-manager.org/SutterStage) | `:A` ASCII | ✓ | ✓ | ✓ |
| `mm-adapter-teensy-pulse` | Teensy serial pulse generator | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-thorlabs-chrolis` | [Thorlabs CHROLIS 6-channel LED](https://micro-manager.org/ThorlabsCHROLIS) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-thorlabs-ell14` | [Thorlabs ELL14 rotation stage](https://micro-manager.org/ThorlabsElliptecSlider) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-thorlabs-fw` | [Thorlabs filter wheel](https://micro-manager.org/ThorlabsFilterWheel) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-thorlabs-pm100x` | [Thorlabs PM100x power meter](https://micro-manager.org/ThorlabsPM) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-thorlabs-sc10` | [Thorlabs SC10 shutter controller](https://micro-manager.org/ThorlabsSC10) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-thorlabs-tsp01` | Thorlabs TSP01 temp/humidity sensor | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-tofra` | [TOFRA filter wheel, Z-drive, XY stage](https://micro-manager.org/Tofra) | IMS MDrive ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-toptica-ibeam` | [Toptica iBeam Smart CW laser](https://micro-manager.org/Toptica_iBeamSmartCW) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-triggerscope` | [TriggerScope TTL/DAC controller](https://micro-manager.org/TriggerScope) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-triggerscope-mm` | [TriggerScope MM variant](https://micro-manager.org/TriggerScopeMM) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-tsi` | [Thorlabs Scientific Imaging cameras](https://micro-manager.org/TSI) | TSI SDK3; `--features tsi` | ✓ | ✓ | ✓ |
| `mm-adapter-twain` | [TWAIN-compatible cameras](https://micro-manager.org/TwainCamera) | TWAIN DSM; `--features twain` | ✓ | ✗ | ✓ |
| `mm-adapter-universal-hub-serial` | [Universal serial hub](https://micro-manager.org/UniversalMMHubSerial) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-varilc` | [Cambridge Research VariLC liquid crystal](https://micro-manager.org/VariLC) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-varispec` | [CRI VariSpec LCTF](https://micro-manager.org/VarispecLCTF) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-vincent` | [Vincent Associates Uniblitz shutter](https://micro-manager.org/Vincent) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-vortran` | [Vortran Stradus / Versalase laser](https://micro-manager.org/Stradus) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-wienecke-sinske` | [Wienecke & Sinske stage](https://micro-manager.org/WieneckeSinske) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-xcite` | [Excelitas X-Cite arc lamp](https://micro-manager.org/XCite120PC_Exacte) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-xcite-led` | [X-Cite LED illuminator](https://micro-manager.org/XCiteLed) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-xcite-xt600` | [X-Cite XT600 illuminator](https://micro-manager.org/XCiteXT600) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-xlight` | [CrestOptics X-Light spinning disk](https://micro-manager.org/XLight) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-xlight-v3` | CrestOptics X-Light V3 | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-yodn-e600` | [Yodn E600 LED](https://micro-manager.org/YodnLighting) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-yokogawa` | [Yokogawa spinning disk controller](https://micro-manager.org/Yokogawa) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-zaber` | [Zaber linear + XY stage](https://micro-manager.org/Zaber) | ASCII `\n` (Zaber ASCII v2) | ✓ | ✓ | ✓ |
| `mm-adapter-zeiss-can` | [Zeiss CAN-bus: Z focus, MCU28 XY, turrets, shutter](https://micro-manager.org/ZeissCAN) | 24-bit hex `\r`, 9600 baud | ✓ | ✓ | ✓ |

#### Pending — vendor SDK required

These adapters need proprietary SDKs or closed hardware interfaces not available as pure serial. Contributions welcome if you have access to the SDK.

| C++ adapter | Blocker | W | M | L |
|---|---|:---:|:---:|:---:|
| [ABS](https://micro-manager.org/ABSCamera) | Demo/test DLL | ✓ | ✗ | ✓ |
| AMF | No serial interface found | ✓ | ✗ | ✗ |
| [AOTF](https://micro-manager.org/AA_AOTF) | `inpout.dll` LPT port I/O | ✓ | ✗ | ✓ |
| [AgilentLaserCombiner](https://micro-manager.org/AgilentLaserCombiner) | LaserCombinerSDK.h | ✓ | ✗ | ✗ |
| [AlliedVisionCamera](https://micro-manager.org/AlliedVisionCamera) | Vimba SDK | ✓ | ✗ | ✓ |
| [AmScope](https://micro-manager.org/AmScope) | AmScope camera SDK | ✓ | ✗ | ✗ |
| [Andor](https://micro-manager.org/Andor) | Andor SDK (CCD/EMCCD) | ✓ | ✗ | ✓ |
| [AndorLaserCombiner](https://micro-manager.org/AndorLaserCombiner) | AB_ALC_REV64.dll | ✓ | ✗ | ✓ |
| [AndorShamrock](https://micro-manager.org/AndorShamrock) | Andor Shamrock spectrograph SDK | ✓ | ✗ | ✗ |
| [Aravis](https://micro-manager.org/Aravis) | GLib/GObject/aravis (GigE Vision) | ✗ | ✗ | ✓ |
| Atik | Atik camera SDK | ✓ | ✗ | ✗ |
| BDPathway | BD Pathway imaging system | ✓ | ✗ | ✓ |
| [BH_DCC_DCU](https://micro-manager.org/BH_DCC_DCU) | Becker-Hickl DCC/DCU DLL | ✓ | ✗ | ✗ |
| [BaumerOptronic](https://micro-manager.org/BaumerOptronic) | Baumer camera SDK | ✓ | ✗ | ✗ |
| CNCMicroscope | Custom hardware | ✓ | ✗ | ✓ |
| [CairnOptoSpinUCSF](https://micro-manager.org/CairnOptospinUCSF) | Cairn/UCSF custom controller | ✓ | ✗ | ✓ |
| Cephla | Cephla controller | ✓ | ✗ | ✓ |
| [DTOpenLayer](https://micro-manager.org/DTOpenLayer) | DAQ hardware I/O | ✓ | ✗ | ✓ |
| [DahengGalaxy](https://micro-manager.org/Daheng) | Daheng Galaxy SDK | ✓ | ✗ | ✗ |
| [DirectElectron](https://micro-manager.org/DECamera) | Direct Electron camera SDK | ✓ | ✗ | ✗ |
| Dragonfly | Andor Dragonfly SDK | ✓ | ✗ | ✗ |
| [Elveflow](https://micro-manager.org/Elveflow) | `ob1_mk4.h` proprietary SDK | ✓ | ✗ | ✗ |
| [EvidentIX85](https://micro-manager.org/EvidentIX85) | Evident/Olympus IX85 SDK | ✓ | ✗ | ✓ |
| [EvidentIX85Win](https://micro-manager.org/EvidentIX85Win) | Evident/Olympus SDK (Windows) | ✓ | ✗ | ✗ |
| [EvidentIX85XYStage](https://micro-manager.org/EvidentIX85XYStage) | Evident/Olympus SDK | ✓ | ✗ | ✗ |
| [FLICamera](https://micro-manager.org/FLICamera) | FLI camera SDK (`libfli.h`) | ✓ | ✗ | ✗ |
| [FakeCamera](https://micro-manager.org/FakeCamera) | Internal simulation utility | ✓ | ✗ | ✓ |
| [Fli](https://micro-manager.org/FLICamera) | FLI SDK | ✓ | ✗ | ✗ |
| [Fluigent](https://micro-manager.org/Fluigent) | `fgt_SDK.h` (GitHub) | ✓ | ✗ | ✗ |
| FocalPoint | Prior FocalPoint | ✗ | ✗ | ✓ |
| [FreeSerialPort](https://micro-manager.org/FreeSerialPort) | Utility serial port device | ✓ | ✗ | ✓ |
| [GenericSLM](https://micro-manager.org/GenericSLM) | Generic SLM utility | ✓ | ✗ | ✗ |
| [GigECamera](https://micro-manager.org/GigECamera) | GigE Vision SDK | ✓ | ✗ | ✗ |
| HIDManager | USB HID | ✓ | ✗ | ✓ |
| Hikrobot | Hikrobot MVSDK | ✓ | ✗ | ✗ |
| [IDSPeak](https://micro-manager.org/IDSPeak) | IDS Peak SDK | ✓ | ✗ | ✗ |
| [IDS_uEye](https://micro-manager.org/IDS_uEye) | IDS uEye SDK | ✓ | ✗ | ✓ |
| [ITC18](https://micro-manager.org/ITC18) | Heka ITC-18 I/O hardware | ✓ | ✗ | ✓ |
| [ImageProcessorChain](https://micro-manager.org/ImageProcessorChain) | Utility/aggregator | ✓ | ✗ | ✓ |
| IntegratedLaserEngine | Andor ILE SDK | ✓ | ✗ | ✗ |
| [K8055](https://micro-manager.org/Velleman_K8055) | Velleman K8055 USB HID | ✓ | ✗ | ✓ |
| [K8061](https://micro-manager.org/Velleman_K8061) | Velleman K8061 USB HID | ✓ | ✗ | ✓ |
| [KuriosLCTF](https://micro-manager.org/KuriosLCTF) | Thorlabs Windows DLLs only | ✓ | ✗ | ✗ |
| [LeicaDMSTC](https://micro-manager.org/LeicaDMSTC) | Leica DMSTC (check protocol) | ✓ | ✗ | ✓ |
| LightSheetManager | Utility/aggregator | ✓ | ✗ | ✓ |
| [Lumencor](https://micro-manager.org/Lumencor_Light_Engine) | LightEngineAPI vendor SDK | ✓ | ✗ | ✗ |
| [Lumenera](https://micro-manager.org/Lumenera) | `lucamapi.h` SDK | ✓ | ✗ | ✗ |
| [MCCDAQ](https://micro-manager.org/MCCDAQ) | Measurement Computing NI-DAQ | ✓ | ✗ | ✗ |
| [MCL_MicroDrive](https://micro-manager.org/MCL_MicroDrive) | Mad City Labs SDK | ✓ | ✗ | ✗ |
| [MCL_NanoDrive](https://micro-manager.org/MCL_NanoDrive) | Mad City Labs SDK | ✓ | ✗ | ✗ |
| [MT20](https://micro-manager.org/MT20) | Leica MT20 (check protocol) | ✓ | ✗ | ✗ |
| MaestroServo | Maestro servo controller | ✓ | ✗ | ✓ |
| [MatrixVision](https://micro-manager.org/MatrixVision) | mvIMPACT Acquire SDK | ✓ | ✗ | ✗ |
| [MeadowlarkLC](https://micro-manager.org/MeadowlarkLC) | `usbdrvd.h` USB HID driver | ✓ | ✗ | ✗ |
| [MicroPoint](https://micro-manager.org/MicroPoint) | Andor MicroPoint SDK | ✓ | ✗ | ✓ |
| [Mightex](https://micro-manager.org/Mightex) | Mightex camera SDK | ✓ | ✗ | ✓ |
| [Mightex_BLS](https://micro-manager.org/Mightex_BLS) | Mightex LED SDK | ✓ | ✗ | ✓ |
| [Mightex_C_Cam](https://micro-manager.org/Mightex_C_Cam) | Mightex camera SDK | ✓ | ✗ | ✗ |
| [Mightex_SB_Cam](https://micro-manager.org/Mightex_SB_Cam) | Mightex camera SDK | ✓ | ✗ | ✗ |
| Modbus | libmodbus (LGPL, open-source) | ✓ | ✗ | ✓ |
| [Motic](https://micro-manager.org/MoticCamera) | Motic camera SDK | ✓ | ✗ | ✗ |
| [MoticMicroscope](https://micro-manager.org/MoticMicroscope) | Motic SDK | ✓ | ✗ | ✗ |
| Motic_mac | Motic SDK (macOS) | ✗ | ✓ | ✗ |
| [NI100X](https://micro-manager.org/National_Instruments) | NI-DAQmx SDK | ✓ | ✗ | ✗ |
| [NIDAQ](https://micro-manager.org/NIDAQ) | NI-DAQmx SDK | ✓ | ✗ | ✗ |
| [NIMultiAnalog](https://micro-manager.org/National_Instruments) | NI-DAQmx SDK | ✓ | ✗ | ✗ |
| NKTSuperK | NKTPDLL.h Windows-only | ✓ | ✗ | ✗ |
| [NikonKs](https://micro-manager.org/NikonKS) | Nikon Ks SDK | ✓ | ✗ | ✗ |
| [NikonTE2000](https://micro-manager.org/NikonTE2000) | Nikon TE2000 SDK | ✓ | ✗ | ✓ |
| [NotificationTester](https://micro-manager.org/NotificationTester) | Internal test utility | ✓ | ✗ | ✓ |
| [OVP_ECS2](https://micro-manager.org/OVP_ECS2) | Check protocol | ✓ | ✗ | ✓ |
| [ObjectiveImaging](https://micro-manager.org/ObjectiveImaging) | Check protocol | ✓ | ✗ | ✗ |
| [Okolab](https://micro-manager.org/Okolab) | `okolib.h` vendor SDK | ✓ | ✗ | ✗ |
| [PCO_Generic](https://micro-manager.org/PCO_Camera) | PCO camera SDK | ✓ | ✗ | ✗ |
| [PI](https://micro-manager.org/PI) | PI SDK (non-GCS) | ✓ | ✗ | ✓ |
| [PIEZOCONCEPT](https://micro-manager.org/PIEZOCONCEPT) | Check protocol | ✓ | ✗ | ✓ |
| [PVCAM](https://micro-manager.org/PVCAM) | Photometrics PVCAM SDK | ✓ | ✗ | ✓ |
| [ParallelPort](https://micro-manager.org/ParallelPort) | Windows LPT / Linux `/dev/parport` | ✓ | ✗ | ✓ |
| [PicardStage](https://micro-manager.org/PicardStage) | Check protocol | ✓ | ✗ | ✗ |
| [Piper](https://micro-manager.org/Piper) | Check protocol | ✓ | ✗ | ✗ |
| [Pixelink](https://micro-manager.org/Pixelink) | Pixelink camera SDK | ✓ | ✗ | ✗ |
| [PlayerOne](https://micro-manager.org/PlayerOne) | Player One Astronomy SDK | ✓ | ✗ | ✗ |
| [PointGrey](https://micro-manager.org/Point_Grey_Research) | FLIR FlyCapture2 SDK | ✓ | ✗ | ✗ |
| [PyDevice](https://micro-manager.org/PyDevice) | Python binding | ✓ | ✗ | ✗ |
| [QCam](https://micro-manager.org/QCam) | QImaging SDK | ✓ | ✗ | ✓ |
| [QSI](https://micro-manager.org/QSICamera) | QSI camera SDK | ✓ | ✗ | ✗ |
| [Rapp](https://micro-manager.org/Rapp) | obsROE_Device vendor class | ✓ | ✗ | ✗ |
| [RappLasers](https://micro-manager.org/RappLasers) | Rapp laser SDK | ✓ | ✗ | ✓ |
| [Rapp_UGA42](https://micro-manager.org/Rapp_UGA42) | Rapp UGA-42 vendor class | ✓ | ✗ | ✗ |
| [RaptorEPIX](https://micro-manager.org/RaptorEPIX) | Raptor EPIX SDK | ✓ | ✗ | ✗ |
| [ReflectionFocus](https://micro-manager.org/ReflectorFocus) | Check protocol | ✓ | ✗ | ✓ |
| Revealer | Check protocol | ✓ | ✗ | ✗ |
| [ScionCam](https://micro-manager.org/ScionCam) | Scion camera SDK | ✓ | ✗ | ✓ |
| [Sensicam](https://micro-manager.org/Sensicam) | PCO Sensicam SDK | ✓ | ✗ | ✓ |
| [SequenceTester](https://micro-manager.org/SequenceTester) | Internal test utility | ✓ | ✗ | ✓ |
| [SerialManager](https://micro-manager.org/SerialManager) | Utility serial port manager | ✓ | ✓ | ✓ |
| [SigmaKoki](https://micro-manager.org/SigmaKoki) | StCamD.h camera SDK | ✓ | ✗ | ✗ |
| SimpleCam | Camera simulation utility | ✓ | ✓ | ✓ |
| [Skyra](https://micro-manager.org/Skyra) | Cobolt Skyra SDK | ✓ | ✗ | ✓ |
| [SmarActHCU-3D](https://micro-manager.org/SmarActHCU-3D) | SmarAct SDK | ✓ | ✗ | ✓ |
| [SouthPort](https://micro-manager.org/SouthPort_MicroZ) | Check protocol | ✓ | ✗ | ✓ |
| [Spinnaker](https://micro-manager.org/Spinnaker) | FLIR Spinnaker SDK | ✓ | ✗ | ✓ |
| [SpinnakerC](https://micro-manager.org/SpinnakerC) | FLIR Spinnaker C SDK | ✓ | ✗ | ✗ |
| [Standa](https://micro-manager.org/Standa) | Standa 8SMC SDK (`USMCDLL.h`) | ✓ | ✗ | ✗ |
| [Standa8SMC4](https://micro-manager.org/Standa8SMC4) | Standa 8SMC4 SDK | ✓ | ✗ | ✗ |
| [StandaStage](https://micro-manager.org/StandaStage) | Standa SDK | ✓ | ✗ | ✗ |
| [StarlightXpress](https://micro-manager.org/StarlightXpress) | Starlight Xpress camera SDK | ✓ | ✗ | ✓ |
| TCPIPPort | TCP/IP utility | ✓ | ✗ | ✓ |
| [TISCam](https://micro-manager.org/TIScam) | The Imaging Source camera SDK | ✓ | ✗ | ✗ |
| [TUCam](https://micro-manager.org/TUCam) | Tucsen camera SDK | ✓ | ✗ | ✗ |
| TeesnySLM | Teensy SLM (check protocol) | ✓ | ✗ | ✗ |
| [ThorlabsAPTStage](https://micro-manager.org/ThorlabsAPTStage) | Thorlabs APT SDK | ✓ | ✗ | ✗ |
| [ThorlabsDC40](https://micro-manager.org/ThorlabsDCStage) | `TLDC2200.h` vendor SDK | ✓ | ✗ | ✓ |
| [ThorlabsDCxxxx](https://micro-manager.org/ThorlabsDCxxxx) | `TLDC2200.h` vendor SDK | ✓ | ✗ | ✓ |
| [ThorlabsUSBCamera](https://micro-manager.org/ThorlabsUSBCamera) | Thorlabs camera SDK | ✓ | ✗ | ✗ |
| TwoPhoton | Custom two-photon hardware | ✓ | ✗ | ✗ |
| [USBManager](https://micro-manager.org/USBManager) | USB utility | ✓ | ✗ | ✓ |
| [USB_Viper_QPL](https://micro-manager.org/USB_Viper_QPL) | USB HID | ✓ | ✗ | ✗ |
| [UniversalMMHubUsb](https://micro-manager.org/UniversalMMHubUsb) | Universal USB hub | ✓ | ✗ | ✓ |
| [UserDefinedSerial](https://micro-manager.org/UserDefinedSerial) | *(todo — pure serial, not yet implemented)* | ✓ | ✓ | ✓ |
| [Utilities](https://micro-manager.org/Utilities) | StateDeviceShutter, DAShutter, etc. | ✓ | ✗ | ✓ |
| [VisiTech_iSIM](https://micro-manager.org/VisiTech_iSIM) | VisiTech iSIM SDK | ✓ | ✗ | ✗ |
| WOSM | Check protocol | ✓ | ✗ | ✗ |
| [Ximea](https://micro-manager.org/XIMEA) | Ximea xiAPI SDK | ✓ | ✗ | ✗ |
| [ZWO](https://micro-manager.org/ZWO) | ZWO ASI camera SDK | ✓ | ✗ | ✗ |
| [ZeissAxioZoom](https://micro-manager.org/ZeissAxioZoom) | Zeiss SDK | ✓ | ✗ | ✗ |
| [ZeissCAN29](https://micro-manager.org/ZeissCAN29) | Zeiss CAN29 bus SDK | ✓ | ✗ | ✓ |
| [dc1394](https://micro-manager.org/dc1394) | FireWire DC1394 library | ✓ | ✗ | ✓ |
| iSIMWaveforms | iSIM waveform utility | ✓ | ✗ | ✗ |
| [kdv](https://micro-manager.org/KDV) | Check protocol | ✓ | ✗ | ✓ |
| [nPoint](https://micro-manager.org/NPointC400) | nPoint piezo SDK | ✓ | ✗ | ✓ |

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
