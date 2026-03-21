#[cfg(feature = "andor-sdk3")]
mod ffi;

#[cfg(feature = "andor-sdk3")]
pub mod camera;

#[cfg(feature = "andor-sdk3")]
pub use camera::Andor3Camera;
