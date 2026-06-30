use micromanager::adapters::demo_async::{AsyncDemoCamera, AsyncDemoStage};
use micromanager::{Dependency, MiniCore, MmResult};
use std::time::Duration;

fn main() -> MmResult<()> {
    let mut scope = MiniCore::new();

    scope.add_device("camera", AsyncDemoCamera::new())?;
    scope.add_device("focus", AsyncDemoStage::new())?;
    scope.add_dependency("camera", Dependency::uses_stage("focus"))?;

    scope.initialize("focus")?.wait(Duration::from_secs(2))?;
    scope.initialize("camera")?.wait(Duration::from_secs(2))?;

    let camera = scope.camera("camera")?;
    let focus = scope.stage("focus")?;

    let move_focus = focus.move_to_um(125.0).submit()?;
    let exposure = camera.set_exposure(Duration::from_millis(20)).submit()?;

    move_focus.wait(Duration::from_secs(1))?;
    exposure.wait(Duration::from_secs(1))?;

    let frame = camera.snap().submit()?.wait(Duration::from_secs(1))?;
    println!("captured {}x{} frame", frame.width, frame.height);

    camera
        .start_sequence(10, Duration::from_millis(50))
        .submit()?
        .wait(Duration::from_secs(1))?;

    for event in scope.events().take(3) {
        println!("{event:?}");
    }

    camera
        .stop_sequence()
        .submit()?
        .wait(Duration::from_secs(1))?;

    Ok(())
}
