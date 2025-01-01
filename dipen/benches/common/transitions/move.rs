use std::sync::atomic::{AtomicUsize, Ordering};

use dipen::{
    exec::{
        CheckStartResult, CreateArcContext, CreatePlaceContext, RunResult, RunTokenContext,
        StartTokenContext, TransitionExecutor, ValidationResult,
    },
    net::{PlaceId, TransitionId},
};

use super::initialize::{decode, encode};

pub struct Move {
    pl_in: PlaceId,
    pl_out: PlaceId,
    tr_id: TransitionId,
    tr_name: String,
}

pub static EXECUTION_COUNT: AtomicUsize = AtomicUsize::new(0);

impl TransitionExecutor for Move {
    fn validate(ctx: &impl dipen::exec::ValidateContext) -> ValidationResult
    where
        Self: Sized,
    {
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
        let pl_in = ctx.arcs_in().next().unwrap().place_context().place_id();
        let pl_out = ctx.arcs_out().next().unwrap().place_context().place_id();
        let tr_name: String = ctx.transition_name().into();

        Move { pl_in, pl_out, tr_id: ctx.transition_id(), tr_name }
    }

    fn check_start(&mut self, ctx: &mut impl dipen::exec::StartContext) -> CheckStartResult {
        // info!("Check start of move transition ({})", self.tr_id.0);
        let next_token = ctx.tokens_at(self.pl_in).next();
        let mut result = CheckStartResult::build();
        match next_token {
            Some(to) => {
                if decode(to.data()) == 0 {
                    result.disabled(Some(self.pl_in), None)
                } else {
                    result.take(&to);
                    result.enabled()
                }
            }
            None => result.disabled(Some(self.pl_in), None),
        }
    }

    async fn run(&mut self, ctx: &mut impl dipen::exec::RunContext) -> RunResult {
        EXECUTION_COUNT.fetch_add(1, Ordering::SeqCst);
        let mut result = RunResult::build();
        for to in ctx.tokens() {
            result.place(to, self.pl_out);
            let num = decode(to.data());
            result.update(to, encode(num - 1));
        }
        result.result()
    }
}
