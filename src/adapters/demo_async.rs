use crate::error::MmResult;
use crate::minicore::{CommandOutcome, DeviceCommand, DeviceContext, DeviceDescriptor, MiniDevice};

pub struct AsyncDemoCamera;

pub struct AsyncDemoStage;

impl AsyncDemoCamera {
    pub fn new() -> Self {
        todo!()
    }
}

impl AsyncDemoStage {
    pub fn new() -> Self {
        todo!()
    }
}

impl MiniDevice for AsyncDemoCamera {
    fn descriptor(&self) -> DeviceDescriptor {
        todo!()
    }

    fn initialize(&mut self, ctx: &mut DeviceContext) -> MmResult<()> {
        let _ = ctx;
        todo!()
    }

    fn shutdown(&mut self, ctx: &mut DeviceContext) -> MmResult<()> {
        let _ = ctx;
        todo!()
    }

    fn submit(
        &mut self,
        command: DeviceCommand,
        ctx: &mut DeviceContext,
    ) -> MmResult<CommandOutcome> {
        let _ = (command, ctx);
        todo!()
    }
}

impl MiniDevice for AsyncDemoStage {
    fn descriptor(&self) -> DeviceDescriptor {
        todo!()
    }

    fn initialize(&mut self, ctx: &mut DeviceContext) -> MmResult<()> {
        let _ = ctx;
        todo!()
    }

    fn shutdown(&mut self, ctx: &mut DeviceContext) -> MmResult<()> {
        let _ = ctx;
        todo!()
    }

    fn submit(
        &mut self,
        command: DeviceCommand,
        ctx: &mut DeviceContext,
    ) -> MmResult<CommandOutcome> {
        let _ = (command, ctx);
        todo!()
    }
}
