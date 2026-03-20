# Adapter Status

Status values: `done` | `todo` (pure serial, portable) | `sdk` (requires vendor SDK/HID/DLL)

| C++ directory | Rust crate | Status | Notes |
|---|---|---|---|
| `89NorthLDI` | `mm-adapter-ldi` | done | ASCII `\n`, dynamic wavelengths |
| `AAAOTF` | `mm-adapter-aaaotf` | done | Crystal Technology AOTF, serial |
| `ABS` | — | sdk | Demo/test only |
| `AMF` | — | sdk | No serial interface found |
| `AOTF` | — | sdk | Uses inpout.dll (Windows LPT port I/O) |
| `ASIFW1000` | `mm-adapter-asifw1000` | done | Binary filter wheel |
| `ASIStage` | `mm-adapter-asi-stage` | done | `:A`/`:N` ASCII |
| `ASITiger` | `mm-adapter-asi-tiger` | done | `:A`/`:N` ASCII, 115200 baud |
| `ASIWPTR` | `mm-adapter-asi-wptr` | done | ASI W-PTR serial device |
| `AgilentLaserCombiner` | — | sdk | LaserCombinerSDK.h |
| `Aladdin` | `mm-adapter-aladdin` | done | ASCII `\r` |
| `AlliedVisionCamera` | — | sdk | Vimba SDK |
| `AmScope` | — | sdk | Camera SDK |
| `Andor` | — | sdk | Andor SDK |
| `AndorLaserCombiner` | — | sdk | AB_ALC_REV64.dll |
| `AndorSDK3` | — | sdk | Andor SDK3 |
| `AndorShamrock` | — | sdk | Andor SDK |
| `Aquinas` | `mm-adapter-aquinas` | done | Microfluidics controller, serial (LGPL) |
| `Aravis` | — | sdk | GLib/GObject/GigE |
| `Arduino` | `mm-adapter-arduino` | done | ASCII `\r` |
| `Arduino32bitBoards` | `mm-adapter-arduino32` | done | 32-bit Arduino variant, serial |
| `ArduinoCounter` | `mm-adapter-arduino-counter` | done | Arduino counter, serial |
| `Atik` | — | sdk | Atik camera SDK |
| `BDPathway` | — | sdk | BD Pathway imaging system |
| `BH_DCC_DCU` | — | sdk | Becker-Hickl photon counting |
| `Basler` | — | sdk | Pylon SDK |
| `BaumerOptronic` | — | sdk | Baumer SDK |
| `BlueboxOptics_niji` | `mm-adapter-niji` | done | Binary sync + `\r\n` |
| `CARVII` | `mm-adapter-carvii` | done | Single-char ASCII `\r` |
| `CNCMicroscope` | — | sdk | Custom hardware |
| `CSUW1` | `mm-adapter-csuw1` | done | CSV ASCII `\r` |
| `CairnOptoSpinUCSF` | — | sdk | Cairn/UCSF custom |
| `Cephla` | — | sdk | Cephla controller |
| `ChuoSeiki_MD5000` | `mm-adapter-chuoseiki` | done | ASCII `\r` |
| `ChuoSeiki_QT` | `mm-adapter-chuoseiki-qt` | done | ChuoSeiki QT-series stages, serial |
| `Cobolt` | `mm-adapter-cobolt` | done | ASCII `\r` |
| `CoboltOfficial` | `mm-adapter-cobolt-official` | done | Cobolt vendor-independent variant, serial |
| `CoherentCube` | `mm-adapter-coherent-cube` | done | ASCII `\r` |
| `CoherentOBIS` | `mm-adapter-coherent-obis` | done | ASCII `\r` |
| `CoherentScientificRemote` | `mm-adapter-coherent-scientific-remote` | done | Coherent Scientific Remote, serial |
| `Conix` | `mm-adapter-conix` | done | `:A`/`:N` ASCII |
| `CoolLEDpE300` | `mm-adapter-coolled` | done | CSS format |
| `CoolLEDpE4000` | `mm-adapter-coolled-pe4000` | done | CSS format, 4-channel |
| `Corvus` | `mm-adapter-corvus` | done | ASCII space-terminated |
| `DTOpenLayer` | — | sdk | DAQ hardware I/O |
| `DahengGalaxy` | — | sdk | Daheng SDK |
| `DemoCamera` | `mm-adapter-demo` | done | Simulated |
| `DirectElectron` | — | sdk | Direct Electron camera SDK |
| `Diskovery` | `mm-adapter-diskovery` | done | Intelligent Imaging spinning disk, serial |
| `Dragonfly` | — | sdk | Andor Dragonfly SDK |
| `ESP32` | `mm-adapter-esp32` | done | ESP32 Arduino controller, serial |
| `ETL` | `mm-adapter-etl` | done | Electrically Tunable Lens, serial |
| `Elveflow` | — | sdk | Elveflow microfluidics SDK |
| `EvidentIX85` | — | sdk | Evident/Olympus IX85 SDK |
| `EvidentIX85Win` | — | sdk | Evident/Olympus SDK (Windows) |
| `EvidentIX85XYStage` | — | sdk | Evident/Olympus SDK |
| `FLICamera` | — | sdk | FLI SDK |
| `FakeCamera` | — | sdk | Internal simulation utility |
| `Fli` | — | sdk | FLI SDK |
| `Fluigent` | — | sdk | Fluigent microfluidics SDK |
| `FocalPoint` | — | sdk | Prior FocalPoint |
| `FreeSerialPort` | — | sdk | Utility serial port device |
| `GenericSLM` | — | sdk | Generic SLM utility |
| `GigECamera` | — | sdk | GigE Vision SDK |
| `HIDManager` | — | sdk | USB HID |
| `HamiltonMVP` | `mm-adapter-hamilton-mvp` | done | `0x06` ACK binary |
| `Hikrobot` | — | sdk | MVSDK |
| `HydraLMT200` | `mm-adapter-hydra-lmt200` | done | Hydra LMT-200 motion controller, serial |
| `IDSPeak` | — | sdk | IDS Peak SDK |
| `IDS_uEye` | — | sdk | IDS uEye SDK |
| `IIDC` | `mm-adapter-iidc` | done | FireWire IIDC (libdc1394); `--features iidc`; no driver on macOS |
| `ITC18` | — | sdk | Heka ITC-18 I/O hardware |
| `IlluminateLEDArray` | `mm-adapter-illuminate-led` | done | LED array, serial + JSON |
| `ImageProcessorChain` | — | sdk | Utility/aggregator |
| `IntegratedLaserEngine` | — | sdk | Andor ILE SDK |
| `IsmatecMCP` | `mm-adapter-ismatec` | done | Address-prefixed `*`-ACK |
| `JAI` | — | sdk | JAI camera SDK |
| `K8055` | — | sdk | Velleman K8055 USB HID |
| `K8061` | — | sdk | Velleman K8061 USB HID |
| `KuriosLCTF` | — | sdk | Thorlabs Windows DLLs only |
| `LaserQuantumLaser` | `mm-adapter-laser-quantum` | done | ASCII `\r` |
| `LeicaDMI` | `mm-adapter-leica-dmi` | done | Leica DMI inverted microscope, serial |
| `LeicaDMR` | `mm-adapter-leica-dmr` | done | Leica DMR upright microscope, serial |
| `LeicaDMSTC` | — | sdk | Leica DMSTC (check protocol) |
| `LightSheetManager` | — | sdk | Utility/aggregator |
| `Ludl` | `mm-adapter-ludl` | done | `:A` ASCII |
| `LudlLow` | `mm-adapter-ludl-low` | done | Low-level Ludl variant, serial |
| `Lumencor` | — | sdk | LightEngineAPI vendor SDK |
| `LumencorCIA` | `mm-adapter-lumencor-cia` | done | Lumencor CIA LED, serial |
| `LumencorSpectra` | `mm-adapter-lumencor-spectra` | done | Binary write-only |
| `Lumenera` | — | sdk | Lumenera camera SDK |
| `MCCDAQ` | — | sdk | Measurement Computing NI-DAQ |
| `MCL_MicroDrive` | — | sdk | Mad City Labs SDK |
| `MCL_NanoDrive` | — | sdk | Mad City Labs SDK |
| `MP285` | `mm-adapter-sutter-stage` | done | Sutter MP-285, `:A` ASCII |
| `MPBLaser` | `mm-adapter-mpb-laser` | done | MPB Communications fiber laser, serial |
| `MT20` | — | sdk | Leica MT20 (check protocol) |
| `MaestroServo` | — | sdk | Check protocol |
| `Marzhauser` | `mm-adapter-marzhauser` | done | ASCII `\r` |
| `Marzhauser-LStep` | `mm-adapter-marzhauser-lstep` | done | Marzhauser LStep variant, serial |
| `MarzhauserLStepOld` | `mm-adapter-marzhauser-lstep-old` | done | Older LStep variant, serial |
| `MatrixVision` | — | sdk | Matrix Vision camera SDK |
| `MeadowlarkLC` | — | sdk | usbdrvd.h USB HID driver |
| `MicroFPGA` | `mm-adapter-microfpga` | done | FPGA controller, USB serial |
| `MicroPoint` | — | sdk | Andor MicroPoint SDK |
| `Mightex` | — | sdk | Mightex camera SDK |
| `Mightex_BLS` | — | sdk | Mightex LED SDK |
| `Mightex_C_Cam` | — | sdk | Mightex camera SDK |
| `Mightex_SB_Cam` | — | sdk | Mightex camera SDK |
| `Modbus` | — | sdk | Modbus industrial protocol library |
| `Motic` | — | sdk | Motic camera SDK |
| `MoticMicroscope` | — | sdk | Motic SDK |
| `Motic_mac` | — | sdk | Motic SDK (macOS) |
| `NI100X` | — | sdk | National Instruments DAQ |
| `NIDAQ` | — | sdk | National Instruments DAQ |
| `NIMultiAnalog` | — | sdk | National Instruments DAQ |
| `NKTSuperK` | — | sdk | Requires NKTPDLL.h proprietary Windows DLL |
| `NeoPixel` | `mm-adapter-neopixel` | done | NeoPixel LED array, serial |
| `Neos` | `mm-adapter-neos` | done | No-response serial |
| `NewportCONEX` | `mm-adapter-newport-stage` | done | ASCII `\r\n` |
| `NewportSMC` | `mm-adapter-newport-stage` | done | ASCII `\r\n` |
| `Nikon` | — | sdk | Nikon vendor protocol (SDK) |
| `NikonKs` | — | sdk | Nikon Ks SDK |
| `NikonTE2000` | — | sdk | Nikon TE2000 SDK |
| `NotificationTester` | — | sdk | Internal test utility |
| `OVP_ECS2` | — | sdk | Check protocol |
| `ObjectiveImaging` | — | sdk | Check protocol |
| `Okolab` | — | sdk | okolib.h vendor SDK |
| `Omicron` | `mm-adapter-omicron` | done | `?CMD`/`!CMD` hex `\r` |
| `OpenCVgrabber` | `mm-adapter-opencv` | done | OpenCV VideoCapture (requires `opencv` crate + OpenCV 4.x system install) |
| `OpenFlexure` | `mm-adapter-openflexure` | done | OpenFlexure stage, serial |
| `OpenUC2` | `mm-adapter-openuc2` | done | UC2 Arduino controller, serial |
| `Oxxius` | `mm-adapter-oxxius-laserboxx` | done | Oxxius LaserBoxx single laser, serial |
| `OxxiusCombiner` | `mm-adapter-oxxius` | done | ASCII `\r`, L6Cc combiner |
| `PCO_Generic` | — | sdk | PCO camera SDK |
| `PI` | — | sdk | PI (Physik Instrumente) SDK |
| `PICAM` | — | sdk | Princeton Instruments SDK |
| `PIEZOCONCEPT` | — | sdk | Check protocol |
| `PI_GCS` | — | sdk | PI GCS SDK |
| `PI_GCS_2` | — | sdk | PI GCS SDK |
| `PVCAM` | — | sdk | Photometrics PVCAM SDK |
| `ParallelPort` | — | sdk | Windows parallel port I/O |
| `PeCon2000` | `mm-adapter-pecon` | done | Raw 3-byte BCD |
| `Pecon` | `mm-adapter-pecon` | done | Raw 3-byte BCD |
| `PicardStage` | — | sdk | Check protocol |
| `Piezosystem_30DV50` | `mm-adapter-piezosystem-30dv50` | done | Piezosystem 30DV50, serial |
| `Piezosystem_NV120_1` | `mm-adapter-piezosystem-nv120` | done | Piezosystem NV-120/1, serial |
| `Piezosystem_NV40_1` | `mm-adapter-piezosystem-nv40-1` | done | Piezosystem NV-40/1, serial |
| `Piezosystem_NV40_3` | `mm-adapter-piezosystem-nv40-3` | done | Piezosystem NV-40/3, serial |
| `Piezosystem_dDrive` | `mm-adapter-piezosystem-ddrive` | done | Piezosystem dDrive, serial |
| `Piper` | — | sdk | Check protocol |
| `Pixelink` | — | sdk | Pixelink camera SDK |
| `PlayerOne` | — | sdk | Player One Astronomy SDK |
| `PointGrey` | — | sdk | FLIR/PointGrey FlyCapture SDK |
| `PrecisExcite` | `mm-adapter-precis-excite` | done | ASCII `\r` |
| `Prior` | `mm-adapter-prior` | done | ASCII `\r` |
| `PriorLegacy` | `mm-adapter-prior-legacy` | done | Legacy Prior variant, serial |
| `PriorPureFocus` | `mm-adapter-prior-purefocus` | done | Prior PureFocus, serial |
| `PrizmatixDevice` | `mm-adapter-prizmatix` | done | Prizmatix LED illuminator, serial |
| `PyDevice` | — | sdk | Python binding |
| `QCam` | — | sdk | QImaging SDK |
| `QSI` | — | sdk | QSI camera SDK |
| `Rapp` | — | sdk | obsROE_Device vendor class (serial wrapper) |
| `RappLasers` | — | sdk | Rapp laser SDK |
| `Rapp_UGA42` | — | sdk | Rapp UGA-42 vendor class |
| `RaptorEPIX` | — | sdk | Raptor EPIX SDK |
| `ReflectionFocus` | — | sdk | Check protocol |
| `Revealer` | — | sdk | Check protocol |
| `Sapphire` | `mm-adapter-sapphire` | done | ASCII `\r` |
| `Scientifica` | `mm-adapter-scientifica` | done | ASCII `\r` |
| `ScientificaMotion8` | `mm-adapter-scientifica-motion8` | done | Scientifica Motion8 variant, serial |
| `ScionCam` | — | sdk | Scion camera SDK |
| `ScopeLED` | `mm-adapter-scopeled` | done | ScopeLED illuminator, serial |
| `Sensicam` | — | sdk | PCO Sensicam SDK |
| `SequenceTester` | — | sdk | Internal test utility |
| `SerialManager` | — | sdk | Utility serial port manager |
| `SigmaKoki` | — | sdk | StCamD.h camera SDK |
| `SimpleCam` | — | sdk | Camera utility |
| `Skyra` | — | sdk | Cobolt Skyra SDK |
| `SmarActHCU-3D` | — | sdk | SmarAct SDK |
| `SouthPort` | — | sdk | Check protocol |
| `SpectralLMM5` | `mm-adapter-spectral-lmm5` | done | Hex-encoded binary `\r` |
| `Spinnaker` | — | sdk | FLIR Spinnaker SDK |
| `SpinnakerC` | — | sdk | FLIR Spinnaker SDK (C) |
| `Spot` | — | sdk | Spot camera SDK |
| `Standa` | — | sdk | Standa 8SMC SDK |
| `Standa8SMC4` | — | sdk | Standa 8SMC4 SDK |
| `StandaStage` | — | sdk | Standa SDK |
| `StarlightXpress` | — | sdk | Starlight Xpress camera SDK |
| `SutterLambda` | `mm-adapter-sutter-lambda` | done | Binary |
| `SutterLambda2` | `mm-adapter-sutter-lambda2` | done | Sutter Lambda 2 (newer protocol), serial |
| `SutterLambdaParallelArduino` | `mm-adapter-sutter-lambda-arduino` | done | Sutter Lambda + Arduino parallel, serial |
| `SutterStage` | `mm-adapter-sutter-stage` | done | `:A` ASCII |
| `TCPIPPort` | — | sdk | TCP/IP utility |
| `TISCam` | — | sdk | TIS camera SDK |
| `TSI` | — | sdk | Thorlabs Scientific Imaging SDK |
| `TUCam` | — | sdk | Tucsen camera SDK |
| `TeensyPulseGenerator` | `mm-adapter-teensy-pulse` | done | Teensy serial pulse generator |
| `TeesnySLM` | — | sdk | Teensy SLM (check) |
| `ThorlabsAPTStage` | — | sdk | Thorlabs APT SDK |
| `ThorlabsCHROLIS` | `mm-adapter-thorlabs-chrolis` | done | Thorlabs CHROLIS LED, serial |
| `ThorlabsDC40` | — | sdk | TLDC2200.h vendor SDK |
| `ThorlabsDCxxxx` | — | sdk | TLDC2200.h vendor SDK |
| `ThorlabsElliptecSlider` | `mm-adapter-elliptec` | done | Hex-position `\r` |
| `ThorlabsFilterWheel` | `mm-adapter-thorlabs-fw` | done | ASCII `\r` |
| `ThorlabsPM100x` | `mm-adapter-thorlabs-pm100x` | done | Thorlabs PM100x power meter, serial |
| `ThorlabsSC10` | `mm-adapter-thorlabs-sc10` | done | Thorlabs SC10 shutter controller, serial |
| `ThorlabsTSP01` | `mm-adapter-thorlabs-tsp01` | done | Thorlabs TSP01 temp/humidity, serial |
| `ThorlabsUSBCamera` | — | sdk | Thorlabs camera SDK |
| `Thorlabs_ELL14` | `mm-adapter-thorlabs-ell14` | done | Thorlabs ELL14 rotation stage, serial |
| `Tofra` | `mm-adapter-tofra` | done | IMS MDrive ASCII `\r` |
| `Toptica_iBeamSmartCW` | `mm-adapter-toptica-ibeam` | done | Toptica iBeam Smart CW laser, serial |
| `TriggerScope` | `mm-adapter-triggerscope` | done | Trigger scope controller, serial |
| `TriggerScopeMM` | `mm-adapter-triggerscope-mm` | done | TriggerScope MM variant, serial |
| `TwainCamera` | — | sdk | TWAIN SDK |
| `TwoPhoton` | — | sdk | Custom two-photon hardware |
| `USBManager` | — | sdk | USB utility |
| `USB_Viper_QPL` | — | sdk | USB HID |
| `UniversalMMHubSerial` | `mm-adapter-universal-hub-serial` | done | Universal serial hub, serial |
| `UniversalMMHubUsb` | — | sdk | Universal USB hub |
| `UserDefinedSerial` | — | todo | User-defined serial device |
| `Utilities` | — | sdk | Utility devices (StateDeviceShutter, etc.) |
| `VariLC` | `mm-adapter-varilc` | done | Cambridge Research VariLC liquid crystal, serial |
| `VarispecLCTF` | `mm-adapter-varispec` | done | ASCII `\r` |
| `VisiTech_iSIM` | — | sdk | VisiTech iSIM SDK |
| `Vincent` | `mm-adapter-vincent` | done | ASCII `\r` |
| `Vortran` | `mm-adapter-vortran` | done | ASCII `\r` |
| `WOSM` | — | sdk | Check protocol |
| `WieneckeSinske` | `mm-adapter-wienecke-sinske` | done | Wienecke & Sinske stage, serial |
| `XCite120PC_Exacte` | `mm-adapter-xcite` | done | ASCII `\r` |
| `XCiteLed` | `mm-adapter-xcite-led` | done | X-Cite LED variant, serial |
| `XCiteXT600` | `mm-adapter-xcite-xt600` | done | X-Cite XT600 variant, serial |
| `XLight` | `mm-adapter-xlight` | done | CrestOptics X-Light spinning disk, serial |
| `XLightV3` | `mm-adapter-xlight-v3` | done | CrestOptics X-Light V3, serial |
| `Xcite` | `mm-adapter-xcite` | done | ASCII `\r` |
| `Ximea` | — | sdk | Ximea camera SDK |
| `YodnE600` | `mm-adapter-yodn-e600` | done | Yodn E600 LED, serial |
| `Yokogawa` | `mm-adapter-yokogawa` | done | Yokogawa spinning disk, serial |
| `ZWO` | — | sdk | ZWO ASI camera SDK |
| `Zaber` | `mm-adapter-zaber` | done | ASCII `\n` (Zaber ASCII v2) |
| `ZeissAxioZoom` | — | sdk | Zeiss SDK |
| `ZeissCAN` | — | sdk | Zeiss CAN bus SDK |
| `ZeissCAN29` | — | sdk | Zeiss CAN29 SDK |
| `dc1394` | — | sdk | FireWire DC1394 |
| `iSIMWaveforms` | — | sdk | iSIM waveform utility |
| `kdv` | — | sdk | Check protocol |
| `nPoint` | — | sdk | nPoint piezo SDK |
| `pgFocus` | `mm-adapter-pgfocus` | done | pgFocus autofocus, serial |
