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

W = Windows, M = macOS, L = Linux. SDK-wrapped adapters are feature-gated; all others are pure serial with no vendor dependencies.

#### Implemented (113 crates)

| Crate | Devices | Protocol | W | M | L |
|---|---|---|:---:|:---:|:---:|
| `mm-adapter-aaaotf` | Crystal Technology AOTF | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-aladdin` | WPI Aladdin syringe pump | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-andor-sdk3` | Andor sCMOS cameras | SDK3 atcore; `--features andor-sdk3` | ✓ | ✗ | ✓ |
| `mm-adapter-aquinas` | Aquinas microfluidics controller | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-arduino` | Arduino shutter / state device | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-arduino-counter` | Arduino pulse counter | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-arduino32` | 32-bit Arduino boards | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-asi-fw` | ASI filter wheel | `:A`/`:N` ASCII | ✓ | ✓ | ✓ |
| `mm-adapter-asi-stage` | ASI XY + Z stage | `:A`/`:N` ASCII | ✓ | ✓ | ✓ |
| `mm-adapter-asi-tiger` | ASI Tiger controller (XY + Z) | `:A`/`:N` ASCII, 115200 baud | ✓ | ✓ | ✓ |
| `mm-adapter-asi-wptr` | ASI W-PTR serial device | ASCII | ✓ | ✓ | ✓ |
| `mm-adapter-asifw1000` | ASI FW-1000 filter wheel + shutter | Binary | ✓ | ✓ | ✓ |
| `mm-adapter-basler` | Basler cameras | Pylon SDK; `--features basler` | ✓ | ✓ | ✓ |
| `mm-adapter-carvii` | BD/CrEST CARVII confocal (shutters, filter wheels, sliders) | Single-char ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-chuoseiki` | ChuoSeiki MD-5000 XY stage | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-chuoseiki-qt` | ChuoSeiki QT-series stages | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-cobolt` | Cobolt diode laser | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-cobolt-official` | Cobolt vendor-independent variant | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-coherent-cube` | Coherent CUBE laser | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-coherent-obis` | Coherent OBIS laser | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-coherent-scientific-remote` | Coherent Scientific Remote | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-conix` | Conix filter cubes, XY + Z stage | `:A`/`:N` ASCII | ✓ | ✓ | ✓ |
| `mm-adapter-coolled` | CoolLED pE-300 LED | CSS format | ✓ | ✓ | ✓ |
| `mm-adapter-coolled-pe4000` | CoolLED pE-4000 LED (4-channel) | CSS format | ✓ | ✓ | ✓ |
| `mm-adapter-corvus` | Corvus XY + Z stage | Space-terminated ASCII | ✓ | ✓ | ✓ |
| `mm-adapter-csuw1` | Yokogawa CSU-W1 spinning disk | CSV ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-demo` | DemoCamera, DemoStage, DemoShutter | Simulated | ✓ | ✓ | ✓ |
| `mm-adapter-diskovery` | Intelligent Imaging Diskovery spinning disk | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-elliptec` | Thorlabs Elliptec linear stage + 2-position slider | Hex-position `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-esp32` | ESP32 Arduino controller | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-etl` | Electrically tunable lens | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-hamilton-mvp` | Hamilton MVP modular valve positioner | `0x06` ACK binary | ✓ | ✓ | ✓ |
| `mm-adapter-hydra-lmt200` | Hydra LMT-200 motion controller | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-iidc` | FireWire IIDC cameras | libdc1394; `--features iidc` | ✓ | ✗ | ✓ |
| `mm-adapter-illuminate-led` | Illuminate LED array | Serial + JSON | ✓ | ✓ | ✓ |
| `mm-adapter-ismatec` | Ismatec MCP peristaltic pump | Address-prefixed `*`-ACK | ✓ | ✓ | ✓ |
| `mm-adapter-jai` | JAI cameras | Pleora eBUS SDK; `--features jai` | ✓ | ✓ | ✓ |
| `mm-adapter-laser-quantum` | Laser Quantum Gem laser | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-ldi` | 89 North LDI LED illuminator | ASCII `\n`, dynamic wavelengths | ✓ | ✓ | ✓ |
| `mm-adapter-leica-dmi` | Leica DMI inverted microscope | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-leica-dmr` | Leica DMR upright microscope | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-ludl` | Ludl BioPrecision XY + Z, filter wheel, shutter | `:A` ASCII | ✓ | ✓ | ✓ |
| `mm-adapter-ludl-low` | Low-level Ludl variant | `:A` ASCII | ✓ | ✓ | ✓ |
| `mm-adapter-lumencor-cia` | Lumencor CIA LED | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-lumencor-spectra` | Lumencor Spectra/Aura/Sola LED (legacy) | Binary write-only | ✓ | ✓ | ✓ |
| `mm-adapter-marzhauser` | Märzhäuser TANGO XY + Z stage | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-marzhauser-lstep` | Märzhäuser LStep variant | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-marzhauser-lstep-old` | Märzhäuser LStep (older protocol) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-microfpga` | MicroFPGA FPGA controller | USB serial | ✓ | ✓ | ✓ |
| `mm-adapter-mpb-laser` | MPB Communications fiber laser | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-neopixel` | NeoPixel LED array | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-neos` | Neos Technologies AO shutter | No-response serial | ✓ | ✓ | ✓ |
| `mm-adapter-newport-stage` | Newport CONEX-CC / SMC100 Z stage | ASCII `\r\n` | ✓ | ✓ | ✓ |
| `mm-adapter-niji` | BlueboxOptics niji 7-channel LED | Binary sync + `\r\n` | ✓ | ✓ | ✓ |
| `mm-adapter-nikon` | Nikon ZStage, TIRFShutter, Ti-TIRFShutter, IntensiLight | ASCII `\r`/`\n` | ✓ | ✓ | ✓ |
| `mm-adapter-omicron` | Omicron PhoxX/LuxX/BrixX laser | `?CMD`/`!CMD` hex `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-opencv` | OpenCV video capture (camera) | OpenCV 4.x | ✓ | ✓ | ✓ |
| `mm-adapter-openflexure` | OpenFlexure microscope stage | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-openuc2` | UC2 Arduino controller | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-oxxius` | Oxxius L6Cc laser combiner | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-oxxius-laserboxx` | Oxxius LaserBoxx single laser | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-pecon` | Pecon TempControl 37-2 (temp + CO2) | Raw 3-byte BCD | ✓ | ✓ | ✓ |
| `mm-adapter-pgfocus` | pgFocus open-source autofocus | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-pi-gcs` | PI GCS Z-stage (C-863, CONEX, etc.) | `SVO`/`MOV`/`POS?` ASCII `\n` | ✓ | ✓ | ✓ |
| `mm-adapter-picam` | Princeton Instruments / Photometrics cameras | PVCAM SDK; `--features picam` | ✓ | ✓ | ✓ |
| `mm-adapter-piezosystem-30dv50` | Piezosystem Jena 30DV50 | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-piezosystem-ddrive` | Piezosystem Jena dDrive | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-piezosystem-nv120` | Piezosystem Jena NV-120/1 | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-piezosystem-nv40-1` | Piezosystem Jena NV-40/1 | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-piezosystem-nv40-3` | Piezosystem Jena NV-40/3 | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-precis-excite` | PrecisExcite LED illuminator | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-prior` | Prior ProScan XY + Z, filter wheel, shutter | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-prior-legacy` | Prior ProScan (legacy protocol) | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-prior-purefocus` | Prior PureFocus autofocus | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-prizmatix` | Prizmatix LED illuminator | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-sapphire` | Coherent Sapphire laser | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-scientifica` | Scientifica XY + Z stage | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-scientifica-motion8` | Scientifica Motion8 variant | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-scopeled` | ScopeLED illuminator | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-spectral-lmm5` | Spectral LMM5 laser combiner | Hex-encoded binary `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-spot` | Diagnostic Instruments SpotCam | SpotCam SDK; `--features spot` | ✓ | ✓ | ✗ |
| `mm-adapter-sutter-lambda` | Sutter Lambda 10-2/10-3 filter wheel | Binary | ✓ | ✓ | ✓ |
| `mm-adapter-sutter-lambda-arduino` | Sutter Lambda + Arduino parallel | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-sutter-lambda2` | Sutter Lambda 2 (newer protocol) | Binary | ✓ | ✓ | ✓ |
| `mm-adapter-sutter-stage` | Sutter MP-285 / MPC-200 XY + Z | `:A` ASCII | ✓ | ✓ | ✓ |
| `mm-adapter-teensy-pulse` | Teensy serial pulse generator | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-thorlabs-chrolis` | Thorlabs CHROLIS 6-channel LED | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-thorlabs-ell14` | Thorlabs ELL14 rotation stage | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-thorlabs-fw` | Thorlabs filter wheel | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-thorlabs-pm100x` | Thorlabs PM100x power meter | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-thorlabs-sc10` | Thorlabs SC10 shutter controller | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-thorlabs-tsp01` | Thorlabs TSP01 temp/humidity sensor | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-tofra` | TOFRA filter wheel, Z-drive, XY stage | IMS MDrive ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-toptica-ibeam` | Toptica iBeam Smart CW laser | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-triggerscope` | TriggerScope TTL/DAC controller | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-triggerscope-mm` | TriggerScope MM variant | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-tsi` | Thorlabs Scientific Imaging cameras | TSI SDK3; `--features tsi` | ✓ | ✓ | ✓ |
| `mm-adapter-twain` | TWAIN-compatible cameras | TWAIN DSM; `--features twain` | ✓ | ✗ | ✓ |
| `mm-adapter-universal-hub-serial` | Universal serial hub | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-varilc` | Cambridge Research VariLC liquid crystal | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-varispec` | CRI VariSpec LCTF | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-vincent` | Vincent Associates Uniblitz shutter | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-vortran` | Vortran Stradus / Versalase laser | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-wienecke-sinske` | Wienecke & Sinske stage | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-xcite` | Excelitas X-Cite arc lamp | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-xcite-led` | X-Cite LED illuminator | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-xcite-xt600` | X-Cite XT600 illuminator | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-xlight` | CrestOptics X-Light spinning disk | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-xlight-v3` | CrestOptics X-Light V3 | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-yodn-e600` | Yodn E600 LED | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-yokogawa` | Yokogawa spinning disk controller | ASCII `\r` | ✓ | ✓ | ✓ |
| `mm-adapter-zaber` | Zaber linear + XY stage | ASCII `\n` (Zaber ASCII v2) | ✓ | ✓ | ✓ |
| `mm-adapter-zeiss-can` | Zeiss CAN-bus: Z focus, MCU28 XY, turrets, shutter | 24-bit hex `\r`, 9600 baud | ✓ | ✓ | ✓ |

#### Pending — vendor SDK required

These adapters need proprietary SDKs or closed hardware interfaces not available as pure serial. Contributions welcome if you have access to the SDK.

| C++ adapter | Blocker | W | M | L |
|---|---|:---:|:---:|:---:|
| ABS | Demo/test DLL | ✓ | ✗ | ✓ |
| AMF | No serial interface found | ✓ | ✗ | ✗ |
| AOTF | `inpout.dll` LPT port I/O | ✓ | ✗ | ✓ |
| AgilentLaserCombiner | LaserCombinerSDK.h | ✓ | ✗ | ✗ |
| AlliedVisionCamera | Vimba SDK | ✓ | ✗ | ✓ |
| AmScope | AmScope camera SDK | ✓ | ✗ | ✗ |
| Andor | Andor SDK (CCD/EMCCD) | ✓ | ✗ | ✓ |
| AndorLaserCombiner | AB_ALC_REV64.dll | ✓ | ✗ | ✓ |
| AndorShamrock | Andor Shamrock spectrograph SDK | ✓ | ✗ | ✗ |
| Aravis | GLib/GObject/aravis (GigE Vision) | ✗ | ✗ | ✓ |
| Atik | Atik camera SDK | ✓ | ✗ | ✗ |
| BDPathway | BD Pathway imaging system | ✓ | ✗ | ✓ |
| BH_DCC_DCU | Becker-Hickl DCC/DCU DLL | ✓ | ✗ | ✗ |
| BaumerOptronic | Baumer camera SDK | ✓ | ✗ | ✗ |
| CNCMicroscope | Custom hardware | ✓ | ✗ | ✓ |
| CairnOptoSpinUCSF | Cairn/UCSF custom controller | ✓ | ✗ | ✓ |
| Cephla | Cephla controller | ✓ | ✗ | ✓ |
| DTOpenLayer | DAQ hardware I/O | ✓ | ✗ | ✓ |
| DahengGalaxy | Daheng Galaxy SDK | ✓ | ✗ | ✗ |
| DirectElectron | Direct Electron camera SDK | ✓ | ✗ | ✗ |
| Dragonfly | Andor Dragonfly SDK | ✓ | ✗ | ✗ |
| Elveflow | `ob1_mk4.h` proprietary SDK | ✓ | ✗ | ✗ |
| EvidentIX85 | Evident/Olympus IX85 SDK | ✓ | ✗ | ✓ |
| EvidentIX85Win | Evident/Olympus SDK (Windows) | ✓ | ✗ | ✗ |
| EvidentIX85XYStage | Evident/Olympus SDK | ✓ | ✗ | ✗ |
| FLICamera | FLI camera SDK (`libfli.h`) | ✓ | ✗ | ✗ |
| FakeCamera | Internal simulation utility | ✓ | ✗ | ✓ |
| Fli | FLI SDK | ✓ | ✗ | ✗ |
| Fluigent | `fgt_SDK.h` (GitHub) | ✓ | ✗ | ✗ |
| FocalPoint | Prior FocalPoint | ✗ | ✗ | ✓ |
| FreeSerialPort | Utility serial port device | ✓ | ✗ | ✓ |
| GenericSLM | Generic SLM utility | ✓ | ✗ | ✗ |
| GigECamera | GigE Vision SDK | ✓ | ✗ | ✗ |
| HIDManager | USB HID | ✓ | ✗ | ✓ |
| Hikrobot | Hikrobot MVSDK | ✓ | ✗ | ✗ |
| IDSPeak | IDS Peak SDK | ✓ | ✗ | ✗ |
| IDS_uEye | IDS uEye SDK | ✓ | ✗ | ✓ |
| ITC18 | Heka ITC-18 I/O hardware | ✓ | ✗ | ✓ |
| ImageProcessorChain | Utility/aggregator | ✓ | ✗ | ✓ |
| IntegratedLaserEngine | Andor ILE SDK | ✓ | ✗ | ✗ |
| K8055 | Velleman K8055 USB HID | ✓ | ✗ | ✓ |
| K8061 | Velleman K8061 USB HID | ✓ | ✗ | ✓ |
| KuriosLCTF | Thorlabs Windows DLLs only | ✓ | ✗ | ✗ |
| LeicaDMSTC | Leica DMSTC (check protocol) | ✓ | ✗ | ✓ |
| LightSheetManager | Utility/aggregator | ✓ | ✗ | ✓ |
| Lumencor | LightEngineAPI vendor SDK | ✓ | ✗ | ✗ |
| Lumenera | `lucamapi.h` SDK | ✓ | ✗ | ✗ |
| MCCDAQ | Measurement Computing NI-DAQ | ✓ | ✗ | ✗ |
| MCL_MicroDrive | Mad City Labs SDK | ✓ | ✗ | ✗ |
| MCL_NanoDrive | Mad City Labs SDK | ✓ | ✗ | ✗ |
| MT20 | Leica MT20 (check protocol) | ✓ | ✗ | ✗ |
| MaestroServo | Maestro servo controller | ✓ | ✗ | ✓ |
| MatrixVision | mvIMPACT Acquire SDK | ✓ | ✗ | ✗ |
| MeadowlarkLC | `usbdrvd.h` USB HID driver | ✓ | ✗ | ✗ |
| MicroPoint | Andor MicroPoint SDK | ✓ | ✗ | ✓ |
| Mightex | Mightex camera SDK | ✓ | ✗ | ✓ |
| Mightex_BLS | Mightex LED SDK | ✓ | ✗ | ✓ |
| Mightex_C_Cam | Mightex camera SDK | ✓ | ✗ | ✗ |
| Mightex_SB_Cam | Mightex camera SDK | ✓ | ✗ | ✗ |
| Modbus | libmodbus (LGPL, open-source) | ✓ | ✗ | ✓ |
| Motic | Motic camera SDK | ✓ | ✗ | ✗ |
| MoticMicroscope | Motic SDK | ✓ | ✗ | ✗ |
| Motic_mac | Motic SDK (macOS) | ✗ | ✓ | ✗ |
| NI100X | NI-DAQmx SDK | ✓ | ✗ | ✗ |
| NIDAQ | NI-DAQmx SDK | ✓ | ✗ | ✗ |
| NIMultiAnalog | NI-DAQmx SDK | ✓ | ✗ | ✗ |
| NKTSuperK | NKTPDLL.h Windows-only | ✓ | ✗ | ✗ |
| NikonKs | Nikon Ks SDK | ✓ | ✗ | ✗ |
| NikonTE2000 | Nikon TE2000 SDK | ✓ | ✗ | ✓ |
| NotificationTester | Internal test utility | ✓ | ✗ | ✓ |
| OVP_ECS2 | Check protocol | ✓ | ✗ | ✓ |
| ObjectiveImaging | Check protocol | ✓ | ✗ | ✗ |
| Okolab | `okolib.h` vendor SDK | ✓ | ✗ | ✗ |
| PCO_Generic | PCO camera SDK | ✓ | ✗ | ✗ |
| PI | PI SDK (non-GCS) | ✓ | ✗ | ✓ |
| PIEZOCONCEPT | Check protocol | ✓ | ✗ | ✓ |
| PVCAM | Photometrics PVCAM SDK | ✓ | ✗ | ✓ |
| ParallelPort | Windows LPT / Linux `/dev/parport` | ✓ | ✗ | ✓ |
| PicardStage | Check protocol | ✓ | ✗ | ✗ |
| Piper | Check protocol | ✓ | ✗ | ✗ |
| Pixelink | Pixelink camera SDK | ✓ | ✗ | ✗ |
| PlayerOne | Player One Astronomy SDK | ✓ | ✗ | ✗ |
| PointGrey | FLIR FlyCapture2 SDK | ✓ | ✗ | ✗ |
| PyDevice | Python binding | ✓ | ✗ | ✗ |
| QCam | QImaging SDK | ✓ | ✗ | ✓ |
| QSI | QSI camera SDK | ✓ | ✗ | ✗ |
| Rapp | obsROE_Device vendor class | ✓ | ✗ | ✗ |
| RappLasers | Rapp laser SDK | ✓ | ✗ | ✓ |
| Rapp_UGA42 | Rapp UGA-42 vendor class | ✓ | ✗ | ✗ |
| RaptorEPIX | Raptor EPIX SDK | ✓ | ✗ | ✗ |
| ReflectionFocus | Check protocol | ✓ | ✗ | ✓ |
| Revealer | Check protocol | ✓ | ✗ | ✗ |
| ScionCam | Scion camera SDK | ✓ | ✗ | ✓ |
| Sensicam | PCO Sensicam SDK | ✓ | ✗ | ✓ |
| SequenceTester | Internal test utility | ✓ | ✗ | ✓ |
| SerialManager | Utility serial port manager | ✓ | ✓ | ✓ |
| SigmaKoki | StCamD.h camera SDK | ✓ | ✗ | ✗ |
| SimpleCam | Camera simulation utility | ✓ | ✓ | ✓ |
| Skyra | Cobolt Skyra SDK | ✓ | ✗ | ✓ |
| SmarActHCU-3D | SmarAct SDK | ✓ | ✗ | ✓ |
| SouthPort | Check protocol | ✓ | ✗ | ✓ |
| Spinnaker | FLIR Spinnaker SDK | ✓ | ✗ | ✓ |
| SpinnakerC | FLIR Spinnaker C SDK | ✓ | ✗ | ✗ |
| Standa | Standa 8SMC SDK (`USMCDLL.h`) | ✓ | ✗ | ✗ |
| Standa8SMC4 | Standa 8SMC4 SDK | ✓ | ✗ | ✗ |
| StandaStage | Standa SDK | ✓ | ✗ | ✗ |
| StarlightXpress | Starlight Xpress camera SDK | ✓ | ✗ | ✓ |
| TCPIPPort | TCP/IP utility | ✓ | ✗ | ✓ |
| TISCam | The Imaging Source camera SDK | ✓ | ✗ | ✗ |
| TUCam | Tucsen camera SDK | ✓ | ✗ | ✗ |
| TeesnySLM | Teensy SLM (check protocol) | ✓ | ✗ | ✗ |
| ThorlabsAPTStage | Thorlabs APT SDK | ✓ | ✗ | ✗ |
| ThorlabsDC40 | `TLDC2200.h` vendor SDK | ✓ | ✗ | ✓ |
| ThorlabsDCxxxx | `TLDC2200.h` vendor SDK | ✓ | ✗ | ✓ |
| ThorlabsUSBCamera | Thorlabs camera SDK | ✓ | ✗ | ✗ |
| TwoPhoton | Custom two-photon hardware | ✓ | ✗ | ✗ |
| USBManager | USB utility | ✓ | ✗ | ✓ |
| USB_Viper_QPL | USB HID | ✓ | ✗ | ✗ |
| UniversalMMHubUsb | Universal USB hub | ✓ | ✗ | ✓ |
| UserDefinedSerial | *(todo — pure serial, not yet implemented)* | ✓ | ✓ | ✓ |
| Utilities | StateDeviceShutter, DAShutter, etc. | ✓ | ✗ | ✓ |
| VisiTech_iSIM | VisiTech iSIM SDK | ✓ | ✗ | ✗ |
| WOSM | Check protocol | ✓ | ✗ | ✗ |
| Ximea | Ximea xiAPI SDK | ✓ | ✗ | ✗ |
| ZWO | ZWO ASI camera SDK | ✓ | ✗ | ✗ |
| ZeissAxioZoom | Zeiss SDK | ✓ | ✗ | ✗ |
| ZeissCAN29 | Zeiss CAN29 bus SDK | ✓ | ✗ | ✓ |
| dc1394 | FireWire DC1394 library | ✓ | ✗ | ✓ |
| iSIMWaveforms | iSIM waveform utility | ✓ | ✗ | ✗ |
| kdv | Check protocol | ✓ | ✗ | ✓ |
| nPoint | nPoint piezo SDK | ✓ | ✗ | ✓ |

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
