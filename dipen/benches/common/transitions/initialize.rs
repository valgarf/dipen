use std::{any::Any, str::from_utf8, sync::Arc};

use dipen::{
    exec::{
        CheckStartResult, CreateArcContext, CreatePlaceContext, RunResult, StartTokenContext,
        TransitionExecutor, ValidationResult,
    },
    net::{PlaceId, TransitionId},
};
use tracing::{error, info};

pub struct InitializeData {
    pub barrier: tokio::sync::Barrier,
    pub iterations: u16,
}
/// Initialize transition that creates a single token if none of the watched places have one.
///  
/// Note: also takes currently taken tokens into account
pub struct Initialize {
    pl_out: PlaceId,      // the place to create the token
    pl_ids: Vec<PlaceId>, // all the places to check
    tr_id: TransitionId,  // id of the transition in the petri net
    finished: bool,
    init_data: Arc<dyn Any + Sync + Send>,
}

// // For better readability encode / decode to string:
pub fn encode(remaining_iterations: u16) -> Vec<u8> {
    remaining_iterations.to_string().as_bytes().into()
}

pub fn decode(data: &[u8]) -> u16 {
    from_utf8(data).expect("Failed to decode number").parse().expect("Failed to decode number")
}

impl TransitionExecutor for Initialize {
    fn validate(ctx: &impl dipen::exec::ValidateContext) -> ValidationResult
    where
        Self: Sized,
    {
        info!("Validating transition {}", ctx.transition_name());
        if ctx.arcs_out().count() == 1 && ctx.arcs_in().count() == ctx.arcs().count() {
            ValidationResult::succeeded()
        } else {
            ValidationResult::failed(
                "Need exactly one InOut arc and may have an arbitrary number of incoming arcs!",
            )
        }
    }

    fn new(ctx: &impl dipen::exec::CreateContext) -> Self
    where
        Self: Sized,
    {
        info!("Creating transition {} (id: {})", ctx.transition_name(), ctx.transition_id().0);
        let pl_out = ctx.arcs_out().next().unwrap().place_context().place_id();
        let pl_ids = ctx.arcs_in().map(|actx| actx.place_context().place_id()).collect();

        let init_data = ctx.registry_data().expect("Missing data for transition");
        Initialize { pl_out, pl_ids, tr_id: ctx.transition_id(), finished: false, init_data }
    }

    fn check_start(&mut self, ctx: &mut impl dipen::exec::StartContext) -> CheckStartResult {
        info!("Check start of initialize transition ({})", self.tr_id.0);

        let mut result = CheckStartResult::build();
        // NOTE: the check_start function should not depend on internal state for production
        // code! This is a hack for benchmarking!
        let token_count = self
            .pl_ids
            .iter()
            .map(|&pl_id| ctx.tokens_at(pl_id).count() + ctx.taken_tokens_at(pl_id).count())
            .sum::<usize>();
        if token_count > 1 {
            error!("There should be only one token at most!");
            panic!("There should be only one token at most!")
        }

        for &pl_in in &self.pl_ids {
            let next_token = ctx.tokens_at(pl_in).next();
            if let Some(to) = next_token {
                if decode(to.data()) == 0 {
                    // found an empty token, take it and create a new one instead.
                    result.take(&to);
                    return result.enabled();
                } else {
                    // if we find any token with other data, we do not run this transition
                    return result.disabled(None, None);
                }
            }
        }
        for &pl_in in &self.pl_ids {
            if ctx.taken_tokens_at(pl_in).next().is_some() {
                return result.disabled(None, None);
            }
        }

        if token_count > 0 {
            error!("WTF?!?");
        }
        // no token found, run this transition
        result.enabled()
    }

    async fn run(&mut self, ctx: &mut impl dipen::exec::RunContext) -> RunResult {
        let mut result = RunResult::build();
        let init_data =
            self.init_data.downcast_ref::<InitializeData>().expect("Data has wrong type");
        if self.finished {
            // we are done with that token
            info!("Finished, trying to delete {} tokens", ctx.tokens().count());
            let _ = init_data.barrier.wait().await;
        } else {
            info!("Creating a new token, deleting {} tokens", ctx.tokens().count());
            // place a single newly created token on the output place
            let _ = init_data.barrier.wait().await;
            // start time is measured by the main thread at this point
            let _ = init_data.barrier.wait().await;
            result.place_new(self.pl_out, encode(init_data.iterations));
            self.finished = true;
        }
        result.result()
    }
}
