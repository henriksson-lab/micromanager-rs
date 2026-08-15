//! Pure-Rust Photometrics (PVCAM) camera adapter.
//!
//! Unlike the vendor-SDK adapters, this one speaks the PVCAM device protocol
//! directly — no closed `libpvcam`. The protocol was recovered by clean-room
//! reverse engineering of the SDK; the full spec lives in
//! `extra_Photometrix/PVCAM_REVERSE_ENGINEERING.md` and the host-command code map
//! in `extra_Photometrix/pvcam_host_command_codes.md`.
//!
//! Layers:
//! - [`protocol`]  — bus-agnostic wire encoders/decoders (pure Rust, tested)
//! - [`transport`] — the [`transport::PvcamBus`] carrier trait (+ a test mock)
//! - [`pcie`]      — PCIe carrier over the GPLv2 `pvcam_pcie.ko` ioctl ABI (scaffold)
//! - [`usb`]       — USB carrier over libusb control+bulk (scaffold)
//! - [`camera`]    — [`camera::PvcamCamera`] implementing `Device` + `Camera`
//!
//! The protocol layer is fully implemented; the OS carriers are scaffolds (their
//! constants are transcribed from the RE notes, the syscalls/libusb calls are
//! marked `TODO`) so the crate builds with no extra dependencies.

pub mod camera;
pub mod pcie;
pub mod protocol;
pub mod transport;
pub mod usb;

pub use camera::PvcamCamera;

use crate::traits::{AdapterModule, AnyDevice, DeviceInfo};
use crate::types::DeviceType;

pub const DEVICE_NAME_CAMERA: &str = "PVCamNative";

static DEVICE_LIST: &[DeviceInfo] = &[DeviceInfo {
    name: DEVICE_NAME_CAMERA,
    description: "Photometrics camera (pure-Rust, reverse-engineered PVCAM protocol)",
    device_type: DeviceType::Camera,
}];

pub struct PvcamNativeAdapter;

impl AdapterModule for PvcamNativeAdapter {
    fn module_name(&self) -> &'static str {
        "pvcam_native"
    }
    fn devices(&self) -> &'static [DeviceInfo] {
        DEVICE_LIST
    }
    fn create_device(&self, name: &str) -> Option<AnyDevice> {
        match name {
            DEVICE_NAME_CAMERA => Some(AnyDevice::Camera(Box::new(PvcamCamera::new()))),
            _ => None,
        }
    }
}
