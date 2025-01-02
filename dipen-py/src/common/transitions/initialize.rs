use std::time::Duration;

use dipen::{
    exec::{
        CheckStartResult, CreateArcContext, CreatePlaceContext, RunResult, TransitionExecutor,
        ValidationResult,
    },
    net::{PlaceId, TransitionId},
};
use tracing::info;

/// Initialize transition that creates a single token if none of the watched places have one.
///  
/// Note: also takes currently taken tokens into account
pub struct Initialize {
    pl_out: PlaceId,      // the place to create the token
    pl_ids: Vec<PlaceId>, // all the places to check
    tr_id: TransitionId,  // id of the transition in the petri net
}

impl TransitionExecutor for Initialize {
    fn validate(ctx: &impl dipen::exec::ValidateContext) -> ValidationResult
    where
        Self: Sized,
    {
        info!("Validating transition {}", ctx.transition_name());
        if ctx.arcs_out().count() == 1
            && ctx.arcs_in().count() == 0
            && ctx.arcs_cond().count() == ctx.arcs().count()
        {
            ValidationResult::succeeded()
        } else {
            ValidationResult::failed(
                "Need exactly one conditional outgoing arc, no incoming arcs and may have an arbitrary number of conditional arcs!",
            )
        }
    }

    fn new(ctx: &impl dipen::exec::CreateContext) -> Self
    where
        Self: Sized,
    {
        info!("Creating transition {} (id: {})", ctx.transition_name(), ctx.transition_id().0);
        let pl_out = ctx.arcs_out().next().unwrap().place_context().place_id();
        let pl_ids = ctx.arcs_cond().map(|actx| actx.place_context().place_id()).collect();
        Initialize { pl_out, pl_ids, tr_id: ctx.transition_id() }
    }

    fn check_start(&mut self, ctx: &mut impl dipen::exec::StartContext) -> CheckStartResult {
        info!("Check start of initialize transition ({})", self.tr_id.0);

        let result = CheckStartResult::build();
        let num_existing: usize = self
            .pl_ids
            .iter()
            .map(|&pl_id| ctx.tokens_at(pl_id).count() + ctx.taken_tokens_at(pl_id).count())
            .sum();
        if num_existing > 0 {
            // if we can find any token, we do not create a new one
            return result.disabled(None, None);
        }
        // no token found, run this transition
        result.enabled()
    }

    async fn run(&mut self, _ctx: &mut impl dipen::exec::RunContext) -> RunResult {
        info!("Running initialize transition ({})", self.tr_id.0);
        tokio::time::sleep(Duration::from_secs(1)).await;
        let mut result = RunResult::build();
        // place a single newly created token on the output place
        result.place_new(self.pl_out, "newly created".into());
        result.result()
    }
}
