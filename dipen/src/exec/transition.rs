use super::{
    CheckStartResult, CreateContext, RunContext, RunResult, StartContext, ValidateContext,
    ValidationResult,
};

pub trait TransitionExecutor {
    fn validate(ctx: &impl ValidateContext) -> ValidationResult;
    fn new(ctx: &impl CreateContext) -> Self;
    fn check_start(&mut self, ctx: &mut impl StartContext) -> CheckStartResult;
    fn run(
        &mut self,
        ctx: &mut impl RunContext,
    ) -> impl std::future::Future<Output = RunResult> + Send + Sync;
}
