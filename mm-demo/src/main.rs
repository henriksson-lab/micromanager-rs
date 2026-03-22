use minifb::{Key, Window, WindowOptions};
use micromanager::adapters::demo::DemoAdapter;
use micromanager::CMMCore;

fn main() {
    // Set up CMMCore with DemoCamera
    let mut core = CMMCore::new();
    core.register_adapter(Box::new(DemoAdapter));
    core.load_device("Camera", "demo", "DCamera").unwrap();
    core.initialize_device("Camera").unwrap();
    core.set_camera_device("Camera").unwrap();
    core.set_exposure(25.0).unwrap();

    // Snap once to get image dimensions
    core.snap_image().unwrap();
    let first = core.get_image().unwrap();
    let width = first.width as usize;
    let height = first.height as usize;

    let mut window = Window::new(
        "DemoCamera — press Esc to close",
        width,
        height,
        WindowOptions::default(),
    )
    .expect("Failed to create window");

    // Cap to ~30 fps
    window.set_target_fps(30);

    let mut pixel_buf: Vec<u32> = vec![0u32; width * height];

    while window.is_open() && !window.is_key_down(Key::Escape) {
        core.snap_image().unwrap();
        let frame = core.get_image().unwrap();

        // Convert GRAY8 → 0x00RRGGBB (gray = R=G=B)
        for (dst, &src) in pixel_buf.iter_mut().zip(frame.data.iter()) {
            let v = src as u32;
            *dst = (v << 16) | (v << 8) | v;
        }

        window.update_with_buffer(&pixel_buf, width, height).unwrap();
    }
}
