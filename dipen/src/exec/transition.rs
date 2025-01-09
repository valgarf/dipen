use super::{
    create::CreationError, CheckStartResult, CreateContext, RunContext, RunResult, StartContext,
};

pub trait TransitionExecutor {
    fn new(ctx: &impl CreateContext) -> Result<Self, CreationError>
    where
        Self: Sized;
    fn check_start(&mut self, ctx: &mut impl StartContext) -> CheckStartResult;
    fn run(
        &mut self,
        ctx: &mut impl RunContext,
    ) -> impl std::future::Future<Output = RunResult> + Send + Sync;
}
