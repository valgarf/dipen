use std::time::Duration;

use dipen::{
    exec::{
        CheckStartResult, CreateArcContext, CreatePlaceContext, RunResult, TransitionExecutor,
        ValidationResult,
    },
    net::{PlaceId, TransitionId},
};
use tracing::info;

pub struct DelayedMove {
    pl_in: PlaceId,
    pl_out: PlaceId,
    tr_id: TransitionId,
    tr_name: String,
    delete: bool,
}

impl TransitionExecutor for DelayedMove {
    fn validate(ctx: &impl dipen::exec::ValidateContext) -> ValidationResult
    where
        Self: Sized,
    {
        info!("Validating transition {}", ctx.transition_name());
        if ctx.arcs_in().count() == 1 && ctx.arcs_out().count() == 1 {
            ValidationResult::succeeded()
        } else {
            ValidationResult::failed("Need exactly one incoming and one outgoing arc")
        }
    }

    fn new(ctx: &impl dipen::exec::CreateContext) -> Self
    where
        Self: Sized,
    {
        info!("Creating transition {} (id: {})", ctx.transition_name(), ctx.transition_id().0);
        let pl_in = ctx.arcs_in().next().unwrap().place_context().place_id();
        let pl_out = ctx.arcs_out().next().unwrap().place_context().place_id();
        let tr_name: String = ctx.transition_name().into();
        DelayedMove { pl_in, pl_out, tr_id: ctx.transition_id(), tr_name, delete: false }
    }

    fn check_start(&mut self, ctx: &mut impl dipen::exec::StartContext) -> CheckStartResult {
        info!("Check start of transition {} ({})", self.tr_name, self.tr_id.0);
        if ctx.tokens_at(self.pl_in).count() >= 3 {
            // if we have too many input tokens, we will delete our token at the end instead of
            // placing it.
            info!("Deleting token!");
            self.delete = true;
        }
        let next_token = ctx.tokens_at(self.pl_in).next();
        let mut result = CheckStartResult::build();
        match next_token {
            Some(to) => {
                result.take(&to);
                result.enabled()
            }
            None => result.disabled(Some(self.pl_in), None),
        }
    }

    async fn run(&mut self, ctx: &mut impl dipen::exec::RunContext) -> RunResult {
        info!("Running transition {} ({})", self.tr_name, self.tr_id.0);
        tokio::time::sleep(Duration::from_secs(1)).await;
        let mut result = RunResult::build();
        if !self.delete {
            for to in ctx.tokens() {
                result.place(to, self.pl_out);
                result.update(
                    to,
                    format!("Placed by transition {} ({})", self.tr_name, self.tr_id.0).into(),
                );
            }
        }
        // if delete is true, we simply do nothing with the token we took. All tokens we take
        // and don't place somewhere will be deleted
        result.result()
    }
}
