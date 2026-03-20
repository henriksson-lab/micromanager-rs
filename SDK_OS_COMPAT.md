# SDK Adapter OS Compatibility

Operating system support for the 143 SDK-dependent adapters, determined by examining
`mmCoreAndDevices/DeviceAdapters/` source: `.vcxproj` (Windows), `Makefile.am` (Unix),
`#ifdef _WIN32` / `#ifdef __linux__` / `#ifdef __APPLE__` guards, and SDK documentation.

W = Windows, M = macOS, L = Linux. ✓ supported, ✗ not supported, ? unknown.

| Adapter | W | M | L | Notes |
|---|---|---|---|---|
| ABS | ✓ | ✗ | ✓ | Windows DLL + Linux ifdef |
| AMF | ✓ | ✗ | ✗ | Windows-only |
| AOTF | ✓ | ✗ | ✓ | inpout.dll on Windows; Linux port exists |
| AgilentLaserCombiner | ✓ | ✗ | ✗ | Windows-only SDK |
| AlliedVisionCamera | ✓ | ✗ | ✓ | Vimba SDK (Win + Linux) |
| AmScope | ✓ | ✗ | ✗ | Windows-only SDK |
| Andor | ✓ | ✗ | ✓ | Andor SDK (Win + Linux) |
| AndorLaserCombiner | ✓ | ✗ | ✓ | Andor SDK (Win + Linux) |
| AndorSDK3 | ✓ | ✗ | ✓ | Andor SDK3 (Win + Linux) |
| AndorShamrock | ✓ | ✗ | ✗ | Windows-only Shamrock spectrograph SDK |
| Aravis | ✗ | ✗ | ✓ | Linux GigE Vision (GLib/GObject) |
| Atik | ✓ | ✗ | ✗ | Windows-only camera SDK |
| BDPathway | ✓ | ✗ | ✓ | Cross-platform |
| BH_DCC_DCU | ✓ | ✗ | ✗ | Windows-only Becker-Hickl SDK |
| Basler | ✓ | ✓ | ✓ | Pylon SDK — all platforms |
| BaumerOptronic | ✓ | ✗ | ✗ | Windows-only SDK |
| CNCMicroscope | ✓ | ✗ | ✓ | Cross-platform |
| CairnOptoSpinUCSF | ✓ | ✗ | ✓ | Cross-platform |
| Cephla | ✓ | ✗ | ✓ | Cross-platform |
| DTOpenLayer | ✓ | ✗ | ✓ | Cross-platform DAQ |
| DahengGalaxy | ✓ | ✗ | ✗ | Windows-only SDK |
| DirectElectron | ✓ | ✗ | ✗ | Windows-only camera SDK |
| Dragonfly | ✓ | ✗ | ✗ | Andor Dragonfly Windows SDK |
| Elveflow | ✓ | ✗ | ✗ | Windows-only SDK |
| EvidentIX85 | ✓ | ✗ | ✓ | Cross-platform |
| EvidentIX85Win | ✓ | ✗ | ✗ | Windows-only (explicit in name) |
| EvidentIX85XYStage | ✓ | ✗ | ✗ | Windows-only stage SDK |
| FLICamera | ✓ | ✗ | ✗ | Windows-only FLI SDK |
| FakeCamera | ✓ | ✗ | ✓ | Cross-platform simulation |
| Fli | ✓ | ✗ | ✗ | Windows-only FLI SDK |
| Fluigent | ✓ | ✗ | ✗ | Windows-only microfluidics SDK |
| FocalPoint | ✗ | ✗ | ✓ | Linux-only |
| FreeSerialPort | ✓ | ✗ | ✓ | Cross-platform serial port |
| GenericSLM | ✓ | ✗ | ✗ | Windows-only SLM control |
| GigECamera | ✓ | ✗ | ✗ | Windows GigE Vision SDK |
| HIDManager | ✓ | ✗ | ✓ | Cross-platform USB HID |
| Hikrobot | ✓ | ✗ | ✗ | Windows-only MVSDK |
| IDSPeak | ✓ | ✗ | ✗ | Windows-only IDS Peak SDK |
| IDS_uEye | ✓ | ✗ | ✓ | IDS uEye (Win + Linux) |
| IIDC | ✓ | ✓ | ✓ | FireWire IIDC — all platforms |
| ITC18 | ✓ | ✗ | ✓ | Heka ITC-18 cross-platform |
| ImageProcessorChain | ✓ | ✗ | ✓ | Cross-platform utility |
| IntegratedLaserEngine | ✓ | ✗ | ✗ | Andor ILE Windows-only SDK |
| JAI | ✓ | ✓ | ✓ | JAI SDK — all platforms |
| K8055 | ✓ | ✗ | ✓ | Velleman K8055 (Win + Linux) |
| K8061 | ✓ | ✗ | ✓ | Velleman K8061 (Win + Linux) |
| KuriosLCTF | ✓ | ✗ | ✗ | Thorlabs Windows DLL only |
| LeicaDMSTC | ✓ | ✗ | ✓ | Cross-platform |
| LightSheetManager | ✓ | ✗ | ✓ | Cross-platform utility |
| Lumencor | ✓ | ✗ | ✗ | LightEngineAPI Windows-only |
| Lumenera | ✓ | ✗ | ✗ | Windows-only camera SDK |
| MCCDAQ | ✓ | ✗ | ✗ | Measurement Computing Windows SDK |
| MCL_MicroDrive | ✓ | ✗ | ✗ | Mad City Labs Windows-only |
| MCL_NanoDrive | ✓ | ✗ | ✗ | Mad City Labs Windows-only |
| MT20 | ✓ | ✗ | ✗ | Windows-only |
| MaestroServo | ✓ | ✗ | ✓ | Cross-platform |
| MatrixVision | ✓ | ✗ | ✗ | Windows-only camera SDK |
| MeadowlarkLC | ✓ | ✗ | ✗ | usbdrvd.h Windows HID driver |
| MicroPoint | ✓ | ✗ | ✓ | Andor MicroPoint (Win + Linux) |
| Mightex | ✓ | ✗ | ✓ | Cross-platform |
| Mightex_BLS | ✓ | ✗ | ✓ | Cross-platform LED SDK |
| Mightex_C_Cam | ✓ | ✗ | ✗ | Windows-only camera SDK |
| Mightex_SB_Cam | ✓ | ✗ | ✗ | Windows-only camera SDK |
| Modbus | ✓ | ✗ | ✓ | Cross-platform serial protocol |
| Motic | ✓ | ✗ | ✗ | Windows-only Motic SDK |
| MoticMicroscope | ✓ | ✗ | ✗ | Windows-only |
| Motic_mac | ✗ | ✓ | ✗ | macOS-only (explicit in name) |
| NI100X | ✓ | ✗ | ✗ | Windows-only NI SDK |
| NIDAQ | ✓ | ✗ | ✗ | NI-DAQ Windows SDK |
| NIMultiAnalog | ✓ | ✗ | ✗ | NI-DAQ Windows SDK |
| NKTSuperK | ✓ | ✗ | ✗ | NKTPDLL.h Windows-only |
| Nikon | ✓ | ✗ | ✓ | Cross-platform |
| NikonKs | ✓ | ✗ | ✗ | Windows-only |
| NikonTE2000 | ✓ | ✗ | ✓ | Cross-platform |
| NotificationTester | ✓ | ✗ | ✓ | Cross-platform test utility |
| OVP_ECS2 | ✓ | ✗ | ✓ | Cross-platform |
| ObjectiveImaging | ✓ | ✗ | ✗ | Windows-only |
| Okolab | ✓ | ✗ | ✗ | okolib.h Windows-only SDK |
| PCO_Generic | ✓ | ✗ | ✗ | PCO camera Windows SDK |
| PI | ✓ | ✗ | ✓ | PI SDK (Win + Linux) |
| PICAM | ✓ | ✓ | ✓ | Princeton Instruments — all platforms |
| PIEZOCONCEPT | ✓ | ✗ | ✓ | Cross-platform |
| PI_GCS | ✓ | ✗ | ✓ | PI GCS SDK (Win + Linux) |
| PI_GCS_2 | ✓ | ✗ | ✓ | PI GCS2 SDK (Win + Linux) |
| PVCAM | ✓ | ✗ | ✓ | Photometrics PVCAM (Win + Linux) |
| ParallelPort | ✓ | ✗ | ✓ | Windows LPT + Linux /dev/parport |
| PicardStage | ✓ | ✗ | ✗ | Windows-only |
| Piper | ✓ | ✗ | ✗ | Windows-only |
| Pixelink | ✓ | ✗ | ✗ | Windows-only camera SDK |
| PlayerOne | ✓ | ✗ | ✗ | Windows-only astronomy SDK |
| PointGrey | ✓ | ✗ | ✗ | FlyCapture2 Windows (no Makefile.am) |
| PyDevice | ✓ | ✗ | ✗ | Windows-only Python binding |
| QCam | ✓ | ✗ | ✓ | QImaging (Win + Linux) |
| QSI | ✓ | ✗ | ✗ | Windows-only astronomy SDK |
| Rapp | ✓ | ✗ | ✗ | obsROE_Device Windows class |
| RappLasers | ✓ | ✗ | ✓ | Cross-platform laser control |
| Rapp_UGA42 | ✓ | ✗ | ✗ | Windows-only |
| RaptorEPIX | ✓ | ✗ | ✗ | Windows-only EPIX SDK |
| ReflectionFocus | ✓ | ✗ | ✓ | Cross-platform |
| Revealer | ✓ | ✗ | ✗ | Windows-only |
| ScionCam | ✓ | ✗ | ✓ | Scion camera (Win + Linux) |
| Sensicam | ✓ | ✗ | ✓ | PCO Sensicam (Win + Linux) |
| SequenceTester | ✓ | ✗ | ✓ | Cross-platform test utility |
| SerialManager | ✓ | ✓ | ✓ | Serial I/O — all platforms |
| SigmaKoki | ✓ | ✗ | ✗ | StCamD.h Windows-only SDK |
| SimpleCam | ✓ | ✓ | ✓ | Cross-platform demo camera |
| Skyra | ✓ | ✗ | ✓ | Cobolt Skyra (Win + Linux) |
| SmarActHCU-3D | ✓ | ✗ | ✓ | SmarAct SDK (Win + Linux) |
| SouthPort | ✓ | ✗ | ✓ | Cross-platform |
| Spinnaker | ✓ | ✗ | ✓ | FLIR Spinnaker SDK (Win + Linux) |
| SpinnakerC | ✓ | ✗ | ✗ | Spinnaker C SDK Windows-only |
| Spot | ✓ | ✓ | ✓ | Diagnostic Instruments — all platforms |
| Standa | ✓ | ✗ | ✗ | Windows-only 8SMC SDK |
| Standa8SMC4 | ✓ | ✗ | ✗ | Windows-only 8SMC4 SDK |
| StandaStage | ✓ | ✗ | ✗ | Windows-only Standa SDK |
| StarlightXpress | ✓ | ✗ | ✓ | SX astronomy camera (Win + Linux) |
| TCPIPPort | ✓ | ✗ | ✓ | Cross-platform TCP/IP utility |
| TISCam | ✓ | ✗ | ✗ | The Imaging Source Windows SDK |
| TSI | ✓ | ✓ | ✓ | Thorlabs Scientific Imaging — all platforms |
| TUCam | ✓ | ✗ | ✗ | Windows-only Tucsen SDK |
| TeesnySLM | ✓ | ✗ | ✗ | Windows-only SLM |
| ThorlabsAPTStage | ✓ | ✗ | ✗ | Thorlabs APT Windows SDK |
| ThorlabsDC40 | ✓ | ✗ | ✓ | Thorlabs LED controller (Win + Linux) |
| ThorlabsDCxxxx | ✓ | ✗ | ✓ | Thorlabs filter wheel (Win + Linux) |
| ThorlabsUSBCamera | ✓ | ✗ | ✗ | Windows-only Thorlabs camera SDK |
| TwainCamera | ✓ | ✗ | ✗ | TWAIN Windows/macOS — no Linux |
| TwoPhoton | ✓ | ✗ | ✗ | Windows-only custom hardware |
| USBManager | ✓ | ✗ | ✓ | Cross-platform USB serial |
| USB_Viper_QPL | ✓ | ✗ | ✗ | Windows-only USB HID |
| UniversalMMHubUsb | ✓ | ✗ | ✓ | Cross-platform |
| Utilities | ✓ | ✗ | ✓ | Cross-platform utility devices |
| VisiTech_iSIM | ✓ | ✗ | ✗ | Windows-only confocal SDK |
| WOSM | ✓ | ✗ | ✗ | Windows-only |
| Ximea | ✓ | ✗ | ✗ | Windows-only Ximea SDK |
| ZWO | ✓ | ✗ | ✗ | Windows-only ASI astronomy SDK |
| ZeissAxioZoom | ✓ | ✗ | ✗ | Zeiss Windows SDK |
| ZeissCAN | ✓ | ✗ | ✓ | Zeiss CAN bus (Win + Linux) |
| ZeissCAN29 | ✓ | ✗ | ✓ | Zeiss CAN29 bus (Win + Linux) |
| dc1394 | ✓ | ✗ | ✓ | FireWire IEEE1394 (Win + Linux) |
| iSIMWaveforms | ✓ | ✗ | ✗ | Windows-only waveform generator |
| kdv | ✓ | ✗ | ✓ | Cross-platform |
| nPoint | ✓ | ✗ | ✓ | nPoint piezo (Win + Linux) |

## Summary

| Platform | Count |
|---|---|
| Windows only | ~70 |
| Windows + Linux | ~59 |
| All platforms (Win+Mac+Linux) | 7 (Basler, IIDC, JAI, PICAM, SerialManager, SimpleCam, Spot, TSI) |
| Linux only | 2 (Aravis, FocalPoint) |
| macOS only | 1 (Motic_mac) |
