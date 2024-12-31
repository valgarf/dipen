use std::{any::Any, sync::Arc};

use byteorder::{ReadBytesExt, WriteBytesExt, LE};
use petri_etcd_runner::{
    net::{PlaceId, TransitionId},
    transition::{
        CheckStartResult, CreateArcContext, CreatePlaceContext, RunResult, StartTokenContext,
        TransitionExecutor, ValidationResult,
    },
};
use tracing::info;

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

impl TransitionExecutor for Initialize {
    fn validate(ctx: &impl petri_etcd_runner::transition::ValidateContext) -> ValidationResult
    where
        Self: Sized,
    {
        info!("Validating transition {}", ctx.transition_name());
        if ctx.arcs_out().count() == 1 && ctx.arcs_in().count() == ctx.arcs().count() {
            ValidationResult::success()
        } else {
            ValidationResult::failure(
                "Need exactly one InOut arc and may have an arbitrary number of incoming arcs!",
            )
        }
    }

    fn new(ctx: &impl petri_etcd_runner::transition::CreateContext) -> Self
    where
        Self: Sized,
    {
        info!("Creating transition {} (id: {})", ctx.transition_name(), ctx.transition_id().0);
        let pl_out = ctx.arcs_out().next().unwrap().place_context().place_id();
        let pl_ids = ctx.arcs_in().map(|actx| actx.place_context().place_id()).collect();

        let init_data = ctx.registry_data().expect("Missing data for transition");
        Initialize { pl_out, pl_ids, tr_id: ctx.transition_id(), finished: false, init_data }
    }

    fn check_start(
        &mut self,
        ctx: &mut impl petri_etcd_runner::transition::StartContext,
    ) -> CheckStartResult {
        info!("Check start of initialize transition ({})", self.tr_id.0);

        let mut result = CheckStartResult::build();
        // NOTE: the check_start function should not depend on internal state for production
        // code! This is a hack for benchmarking!
        for &pl_in in &self.pl_ids {
            let next_token = ctx.tokens_at(pl_in).next();
            if let Some(to) = next_token {
                if to.data().read_u16::<LE>().expect("failed to read number") == 0 {
                    // found an empty token, take it and create a new one instead.
                    result.take(&to);
                    return result.enabled();
                } else {
                    // if we find any token with other data, we do not run this transition
                    return result.disabled(None, None);
                }
            }
        }

        // no token found, run this transition
        result.enabled()
    }

    async fn run(&mut self, _: &mut impl petri_etcd_runner::transition::RunContext) -> RunResult {
        let mut result = RunResult::build();
        let init_data =
            self.init_data.downcast_ref::<InitializeData>().expect("Data has wrong type");
        if self.finished {
            // we are done with that token
            let _ = init_data.barrier.wait().await;
        } else {
            // place a single newly created token on the output place
            let _ = init_data.barrier.wait().await;
            let mut result_data: Vec<u8> = vec![];
            result_data.write_u16::<LE>(init_data.iterations).expect("failed to read number");
            result.place_new(self.pl_out, result_data);
            self.finished = true;
        }
        result.result()
    }
}
