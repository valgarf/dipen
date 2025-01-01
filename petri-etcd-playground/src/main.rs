use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use petri_etcd_runner::{
    error::Result as PetriResult,
    net,
    net::{ArcVariant, PetriNetBuilder, Place, PlaceId, Transition, TransitionId},
    runner::ExecutorRegistry,
    transition::{
        CheckStartResult, CreateArcContext, CreatePlaceContext, RunResult, TransitionExecutor,
        ValidationResult,
    },
    ETCDConfigBuilder, ETCDGate,
};
use tokio::{signal, task::JoinSet};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use tracing_subscriber::{self, EnvFilter};

struct SimpleTrans {
    pl_in: PlaceId,
    pl_out: PlaceId,
    tr_id: TransitionId,
    delete: bool,
}

static EXECUTION_COUNT: AtomicUsize = AtomicUsize::new(0);

impl TransitionExecutor for SimpleTrans {
    fn validate(ctx: &impl petri_etcd_runner::transition::ValidateContext) -> ValidationResult
    where
        Self: Sized,
    {
        info!("Validating transition {}", ctx.transition_name());
        if ctx.arcs_in().next().is_some() && ctx.arcs_out().next().is_some() {
            ValidationResult::success()
        } else {
            ValidationResult::failure("Need at least one incoming and one outgoing arc")
        }
    }

    fn new(ctx: &impl petri_etcd_runner::transition::CreateContext) -> Self
    where
        Self: Sized,
    {
        info!("Creating transition {} (id: {})", ctx.transition_name(), ctx.transition_id().0);
        let pl_in = ctx.arcs_in().next().unwrap().place_context().place_id();
        let pl_out = ctx.arcs_out().next().unwrap().place_context().place_id();
        SimpleTrans { pl_in, pl_out, tr_id: ctx.transition_id(), delete: false }
    }

    fn check_start(
        &mut self,
        ctx: &mut impl petri_etcd_runner::transition::StartContext,
    ) -> CheckStartResult {
        // info!("Check start of transition {}", self.tr_id.0);
        if ctx.tokens_at(self.pl_in).count() >= 3 {
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

    async fn run(&mut self, ctx: &mut impl petri_etcd_runner::transition::RunContext) -> RunResult {
        EXECUTION_COUNT.fetch_add(1, Ordering::Relaxed);
        // info!("Running transition {}", self.tr_id.0);
        tokio::time::sleep(Duration::from_secs(1)).await;
        let mut result = RunResult::build();
        if !self.delete {
            for to in ctx.tokens() {
                result.place(to, self.pl_out);
                result.update(to, format!("Placed by transition {}", self.tr_id.0).into());
            }
        }
        result.result()
    }
}

struct InitializeTrans {
    pl_out: PlaceId,
    pl_ids: Vec<PlaceId>,
    tr_id: TransitionId,
}

impl TransitionExecutor for InitializeTrans {
    fn validate(ctx: &impl petri_etcd_runner::transition::ValidateContext) -> ValidationResult
    where
        Self: Sized,
    {
        info!("Validating transition {}", ctx.transition_name());
        if ctx.arcs_out().next().is_some() && ctx.arcs_cond().count() == ctx.arcs().count() {
            ValidationResult::success()
        } else {
            ValidationResult::failure(
                "Need at least one outgoing arc and all arcs must be conditional ones!",
            )
        }
    }

    fn new(ctx: &impl petri_etcd_runner::transition::CreateContext) -> Self
    where
        Self: Sized,
    {
        info!("Creating transition {} (id: {})", ctx.transition_name(), ctx.transition_id().0);
        let pl_out = ctx.arcs_out().next().unwrap().place_context().place_id();
        let pl_ids = ctx.arcs_cond().map(|actx| actx.place_context().place_id()).collect();
        InitializeTrans { pl_out, pl_ids, tr_id: ctx.transition_id() }
    }

    fn check_start(
        &mut self,
        ctx: &mut impl petri_etcd_runner::transition::StartContext,
    ) -> CheckStartResult {
        // info!("Check start of transition {}", self.tr_id.0);

        let result = CheckStartResult::build();
        let num_existing: usize = self
            .pl_ids
            .iter()
            .map(|&pl_id| ctx.tokens_at(pl_id).count() + ctx.taken_tokens_at(pl_id).count())
            .sum();
        if num_existing > 0 {
            return result.disabled(None, None);
        }
        result.enabled()
    }

    async fn run(
        &mut self,
        _ctx: &mut impl petri_etcd_runner::transition::RunContext,
    ) -> RunResult {
        info!("Running transition {}", self.tr_id.0);
        tokio::time::sleep(Duration::from_secs(1)).await;
        let mut result = RunResult::build();
        result.place_new(self.pl_out, "newly created".into());
        result.result()
    }
}

#[tracing::instrument(level = "info")]
async fn playground() -> PetriResult<()> {
    let shutdown_token = CancellationToken::new();
    let shutdown_token_clone = shutdown_token.clone();
    tokio::spawn(async move {
        tokio::select! {
            _ = signal::ctrl_c() => {
                warn!("Shutting down. Ctrl+C pressed (or corresponding signal sent).")
            },
            _ = shutdown_token_clone.cancelled() => {
                warn!("Shutting down. Shutdown token has been invoked (probably due to some previous error).")
            },
        }
        shutdown_token_clone.cancel();
    });

    let mut net = PetriNetBuilder::default();

    let mut executors1 = ExecutorRegistry::new();
    let mut executors2 = ExecutorRegistry::new();

    net.insert_place(Place::new("pl1", true))?;
    net.insert_place(Place::new("pl2", true))?;
    net.insert_transition(Transition::new("tr1", "test-region-1"))?;
    net.insert_transition(Transition::new("tr2", "test-region-1"))?;
    net.insert_arc(net::Arc::new("pl1", "tr1", ArcVariant::In, "".into()))?;
    net.insert_arc(net::Arc::new("pl2", "tr1", ArcVariant::Out, "".into()))?;
    net.insert_arc(net::Arc::new("pl2", "tr2", ArcVariant::In, "".into()))?;
    net.insert_arc(net::Arc::new("pl1", "tr2", ArcVariant::Out, "".into()))?;
    net.insert_transition(Transition::new("tr-init", "test-region-1"))?;
    net.insert_arc(net::Arc::new("pl1", "tr-init", ArcVariant::OutCond, "".into()))?;
    net.insert_arc(net::Arc::new("pl2", "tr-init", ArcVariant::Cond, "".into()))?;
    executors1.register::<SimpleTrans>("tr1", None);
    executors1.register::<SimpleTrans>("tr2", None);
    executors2.register::<SimpleTrans>("tr2", None);
    executors1.register::<InitializeTrans>("tr-init", None);

    // for i in 1..65 {
    //     let pl1 = format!("pl{i}-1");
    //     let pl2 = format!("pl{i}-2");
    //     let tr1 = format!("tr{i}-1");
    //     let tr2 = format!("tr{i}-2");
    //     let tr_init = format!("tr{i}-init");
    //     net.insert_place(Place::new(&pl1, false))?;
    //     net.insert_place(Place::new(&pl2, false))?;
    //     net.insert_transition(Transition::new(&tr1, "test-region-1"))?;
    //     net.insert_transition(Transition::new(&tr2, "test-region-1"))?;
    //     net.insert_arc(net::Arc::new(&pl1, &tr1, ArcVariant::In, "".into()))?;
    //     net.insert_arc(net::Arc::new(&pl2, &tr1, ArcVariant::Out, "".into()))?;
    //     net.insert_arc(net::Arc::new(&pl2, &tr2, ArcVariant::In, "".into()))?;
    //     net.insert_arc(net::Arc::new(&pl1, &tr2, ArcVariant::Out, "".into()))?;
    //     net.insert_transition(Transition::new(&tr_init, "test-region-1"))?;
    //     net.insert_arc(net::Arc::new(&pl1, &tr_init, ArcVariant::OutCond, "".into()))?;
    //     net.insert_arc(net::Arc::new(&pl2, &tr_init, ArcVariant::Cond, "".into()))?;
    //     executors1.register::<SimpleTrans>(&tr1);
    //     executors1.register::<SimpleTrans>(&tr2);
    //     executors1.register::<InitializeTrans>(&tr_init);
    // }
    let net = Arc::new(net);
    let config = ETCDConfigBuilder::default()
        .endpoints(["localhost:2379"])
        .prefix("petri-test/")
        .node_name("node1")
        .region("test-region-1")
        .lease_id(102)
        .build()?;

    let etcd = ETCDGate::new(config);
    let run1 =
        petri_etcd_runner::runner::run(Arc::clone(&net), etcd, executors1, shutdown_token.clone());

    // let config = ETCDConfigBuilder::default()
    //     .endpoints(["localhost:2379"])
    //     .prefix("petri-test/")
    //     .node_name("node2")
    //     .region("test-region-2")
    //     .build()?;

    // let etcd = ETCDGate::new(config);
    // let run2 =
    //     petri_etcd_runner::runner::run(Arc::clone(&net), etcd, executors2, shutdown_token.clone());

    let start = Instant::now();
    let mut join_set = JoinSet::new();

    join_set.spawn(async {
        match run1.await {
            Ok(_) => {
                info!("Run1 finished");
            }
            Err(err) => {
                error!("Run1 finished with: {}", err);
            }
        }
    });

    // join_set.spawn(async {
    //     match run2.await {
    //         Ok(_) => {
    //             info!("Run2 finished")
    //         }
    //         Err(err) => {
    //             error!("Run2 finished with: {}", err)
    //         }
    //     }
    // });

    join_set.join_all().await;
    let end = Instant::now();
    let d = end - start;
    let num_exec = EXECUTION_COUNT.load(Ordering::SeqCst);
    warn!(
        "Time: {}, executions: {} => {} transitions/s",
        d.as_secs_f64(),
        num_exec,
        num_exec as f64 / d.as_secs_f64()
    );
    info!("Bye.");
    Ok(())
}

#[tokio::main]
async fn main() -> PetriResult<()> {
    tracing_subscriber::fmt()
        .with_span_events(
            tracing_subscriber::fmt::format::FmtSpan::CLOSE
                | tracing_subscriber::fmt::format::FmtSpan::NEW,
        )
        .compact()
        .with_env_filter(EnvFilter::try_new("info,petri_etcd_runner=debug").unwrap())
        .init();

    return playground().await;
}
