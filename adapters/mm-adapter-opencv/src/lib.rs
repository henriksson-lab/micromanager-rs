/// OpenCV VideoCapture camera adapter.
///
/// Wraps any camera accessible via `cv::VideoCapture` (webcams, GigE cameras,
/// V4L2 devices, IP streams, video files) behind the MicroManager `Camera` trait.
///
/// # Properties
///
/// | Property | R/W | Description |
/// |---|---|---|
/// | `CameraIndex` | R | OpenCV device index (set before initialize) |
/// | `FrameWidth` | R | Current capture width in pixels |
/// | `FrameHeight` | R | Current capture height in pixels |
/// | `FPS` | R/W | Requested capture frame rate |
/// | `PixelFormat` | R/W | `GRAY8` or `BGR8` |
/// | `Exposure` | R/W | Exposure in ms (maps to `CAP_PROP_EXPOSURE`) |
///
/// # Pixel formats
///
/// - `GRAY8` — 1 byte/pixel grayscale (converted from BGR via `cvtColor`)
/// - `BGR8` — 3 bytes/pixel in OpenCV native BGR order
///
/// # Usage
///
/// ```rust,no_run
/// use mm_adapter_opencv::OpenCvCamera;
/// use mm_device::traits::{Camera, Device};
///
/// let mut cam = OpenCvCamera::new(0); // device index 0
/// cam.initialize().unwrap();
/// cam.snap_image().unwrap();
/// let buf = cam.get_image_buffer().unwrap();
/// println!("{}x{} image, {} bytes", cam.get_image_width(), cam.get_image_height(), buf.len());
/// ```

#[cfg(feature = "opencv")]
pub mod camera;
#[cfg(feature = "opencv")]
pub use camera::OpenCvCamera;
