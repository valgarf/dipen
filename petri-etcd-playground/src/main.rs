use std::{sync::Arc, time::Duration};

use etcd_client::{
    Client, ElectionClient, Error, EventType, LeaseClient, LeaseGrantOptions, WatchClient,
    WatchOptions,
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
use tokio::{select, signal, task::JoinSet, time::sleep};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};
use tracing_subscriber::{self, EnvFilter};

#[tracing::instrument(level = "info", skip(election_client))]
async fn campaign(election_client: &mut ElectionClient, lease_id: i64) -> Result<(), Error> {
    let resp = election_client.campaign("/election-name", "test-elect", lease_id).await?;
    if let Some(leader_key) = resp.leader() {
        let election_name =
            std::str::from_utf8(leader_key.name()).unwrap_or("<leader name decoding failed>");
        info!(election_name, "Became elected leader.",);
    } else {
        return Err(Error::ElectError("Leader election failed".to_string()));
    }
    Ok(())
}

#[tracing::instrument(level = "info", skip(lease_client))]
async fn keep_alive(
    lease_client: &mut LeaseClient,
    lease_id: i64,
    lease_ttl: u64,
) -> Result<(), Error> {
    let (mut keeper, mut stream) = lease_client.keep_alive(lease_id).await?;
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<(), Error>>(128);
    let fut_keep_alive = async move {
        loop {
            keeper.keep_alive().await?;
            if let Some(resp) = stream.message().await? {
                if resp.ttl() as u64 != lease_ttl {
                    return Err(Error::LeaseKeepAliveError("Lease lost".to_string()));
                }
                let resp_lease_id = resp.id();
                debug!(resp_lease_id, "Keep alive.",);
            }
            let _ = tx.send(Ok(())).await;
            sleep(Duration::from_secs(lease_ttl / 5)).await;
        }
    };
    tokio::pin!(fut_keep_alive);

    loop {
        select! {
            res = &mut fut_keep_alive => {res?; return Ok(())}, // if there is an error, return it. If the keep alive loop ended, always return.
            _ = rx.recv() => {}, // successful keep alive, reset the timer
            _ = sleep(Duration::from_secs(lease_ttl)) => {return Err(Error::LeaseKeepAliveError("Connection lost".to_string()));}
        }
    }
}

#[tracing::instrument(level = "info", skip(watch_client))]
async fn watch_all(watch_client: &mut WatchClient, lease_id: i64) -> Result<(), Error> {
    let mut start_revision = 1;
    loop {
        let opts = WatchOptions::new()
            .with_range("/config\x7f")
            .with_prev_key()
            .with_start_revision(start_revision);
        let (mut _watcher, mut watch_stream) =
            watch_client.watch("/config/test", Some(opts)).await?;

        loop {
            if let Some(msg) = watch_stream.message().await? {
                let (cluster_id, member_id, raft_term, revision) = if let Some(h) = msg.header() {
                    (h.cluster_id(), h.member_id(), h.raft_term(), h.revision())
                } else {
                    (0, 0, 0, -1)
                };
                info!(cluster_id, member_id, raft_term, revision, "Watch response");
                let watch_id = msg.watch_id();
                let created = msg.created();
                let canceled = msg.canceled();
                let compact_revision = msg.compact_revision();
                let cancel_reason = msg.cancel_reason();
                info!(
                    watch_id,
                    created, canceled, compact_revision, cancel_reason, "   Watch response",
                );
                if compact_revision > start_revision {
                    info!(compact_revision, "Watch from last compacted revision.");
                    start_revision = compact_revision;
                    break;
                }
                info!("   Events:");
                for evt in msg.events() {
                    let (key, create_rev, mod_rev, has_key) = if let Some(kv) = evt.kv() {
                        (
                            std::str::from_utf8(kv.key()).unwrap_or("<key encoding issue>"),
                            kv.create_revision(),
                            kv.mod_revision(),
                            true,
                        )
                    } else if let Some(kv) = evt.prev_kv() {
                        (
                            std::str::from_utf8(kv.key()).unwrap_or("<key encoding issue>"),
                            kv.create_revision(),
                            kv.mod_revision(),
                            false,
                        )
                    } else {
                        ("<unknown>", -1, -1, false)
                    };

                    let prev_value = evt.prev_kv().map(|kv| {
                        std::str::from_utf8(kv.value()).unwrap_or("<value encoding issue>")
                    });
                    let value = evt.kv().map(|kv| {
                        std::str::from_utf8(kv.value()).unwrap_or("<value encoding issue>")
                    });
                    let (method, prev, cur) = match evt.event_type() {
                        EventType::Put => {
                            if let Some(prev_value) = prev_value {
                                ("   PUT", prev_value, value.unwrap_or("<unknown>"))
                            } else {
                                ("CREATE", "<null>", value.unwrap_or("<unknown>"))
                            }
                        }
                        EventType::Delete => (
                            "DELETE",
                            prev_value.unwrap_or("<unknown>"),
                            value.unwrap_or("<unknown>"),
                        ),
                    };
                    info!(
                        "    - {} {}: {} -> {} ({}, {}, {})",
                        method, key, prev, cur, create_rev, mod_rev, has_key
                    );
                }
            }
        }
    }
}

#[tracing::instrument(level = "info")]
async fn playground() -> PetriResult<()> {
    let client = Client::connect(["localhost:2379"], None).await?;

    // get a lease
    let mut lease_client = client.lease_client();
    let resp = match lease_client.grant(10, Some(LeaseGrantOptions::new().with_id(123))).await {
        Ok(resp) => resp,
        Err(err) => {
            warn!("Shutting down. Granting a lease failed with: {}.", err);
            return Ok(());
        }
    };
    let lease_id = resp.id();
    let lease_ttl = resp.ttl() as u64;
    info!(lease_id, lease_ttl, "Lease granted.");

    // client.election_client().

    let shutdown_token = CancellationToken::new();

    // keep etcd lease alive
    let shutdown_token_clone = shutdown_token.clone();

    tokio::spawn(async move {
        if let Err(err) = keep_alive(&mut lease_client, lease_id, lease_ttl).await {
            warn!("Shutting down. Keep alive failed with: {}.", err)
        } else {
            warn!("Shutting down. Keep alive finished unexpectedly.")
        }
        shutdown_token_clone.cancel();
    });

    // listen to signals
    let shutdown_token_clone = shutdown_token.clone();
    tokio::spawn(async move {
        tokio::select! {
            _ = signal::ctrl_c() => {
                warn!("Shutting down. Ctrl+C pressed (or corresponding signal sent).")
            },
            _ = shutdown_token_clone.cancelled() => {},
        }
        shutdown_token_clone.cancel();
    });

    let mut election_client = client.election_client();
    select! {
        res = campaign(&mut election_client, lease_id) => {
            if let Err(err) = res {
                warn!("Shutting down. Election failed with: {}.", err);
                shutdown_token.cancel();
            }
        }
        _ = shutdown_token.cancelled() =>  {info!("Shutdown requested");}
    };

    if !shutdown_token.is_cancelled() {
        let mut watch_client = client.watch_client();
        select! {
            res = watch_all(&mut watch_client, lease_id) => {
                if let Err(err) = res {
                    warn!("Shutting down. Watching failed with: {}.", err);
                    shutdown_token.cancel();
                }
            }
            _ = shutdown_token.cancelled() =>  {info!("Shutdown requested");}
        };
    }

    shutdown_token.cancelled().await;

    info!("Cleanup...");
    debug!("Revoking lease...");
    // ignore any errors on revoke. If we cannot reach the server, we shut down. The lease will be
    // released automatically after its TTL
    if let Err(err) = client.lease_client().revoke(lease_id).await {
        warn!("Revoking lease failed with: {}.", err);
    } else {
        debug!("Lease revoked.");
    }

    info!("Bye.");
    Ok(())
}

struct SimpleTrans {
    pl_in: PlaceId,
    pl_out: PlaceId,
    tr_id: TransitionId,
}

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
        SimpleTrans { pl_in, pl_out, tr_id: ctx.transition_id() }
    }

    fn check_start(
        &mut self,
        ctx: &mut impl petri_etcd_runner::transition::StartContext,
    ) -> CheckStartResult {
        info!("Check start of transition {}", self.tr_id.0);
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
        info!("Running transition {}", self.tr_id.0);
        tokio::time::sleep(Duration::from_secs(1)).await;
        let mut result = RunResult::build();
        for to in ctx.tokens() {
            result.place(to, self.pl_out);
            result.update(to, format!("Placed by transition {}", self.tr_id.0).into());
        }
        result.result()
    }
}
#[tracing::instrument(level = "info")]
async fn playground2() -> PetriResult<()> {
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

    net.insert_place(Place::new("pl1"))?;
    net.insert_place(Place::new("pl2"))?;
    net.insert_transition(Transition::new("tr1", "test-region-1"))?;
    net.insert_transition(Transition::new("tr2", "test-region-2"))?;
    net.insert_arc(net::Arc::new("pl1", "tr1", ArcVariant::In, "".into()))?;
    net.insert_arc(net::Arc::new("pl2", "tr1", ArcVariant::Out, "".into()))?;
    net.insert_arc(net::Arc::new("pl2", "tr2", ArcVariant::In, "".into()))?;
    net.insert_arc(net::Arc::new("pl1", "tr2", ArcVariant::Out, "".into()))?;
    executors1.register::<SimpleTrans>("tr1");
    executors2.register::<SimpleTrans>("tr2");

    // for i in 1..10 {
    //     let pl1 = format!("pl{i}-1");
    //     let pl2 = format!("pl{i}-2");
    //     let tr1 = format!("tr{i}-1");
    //     let tr2 = format!("tr{i}-2");
    //     net.insert_place(Place::new(&pl1))?;
    //     net.insert_place(Place::new(&pl2))?;
    //     net.insert_transition(Transition::new(&tr1, "test-region"))?;
    //     net.insert_transition(Transition::new(&tr2, "test-region"))?;
    //     net.insert_arc(net::Arc::new(&pl1, &tr1, ArcVariant::In, "".into()))?;
    //     net.insert_arc(net::Arc::new(&pl2, &tr1, ArcVariant::Out, "".into()))?;
    //     net.insert_arc(net::Arc::new(&pl2, &tr2, ArcVariant::In, "".into()))?;
    //     net.insert_arc(net::Arc::new(&pl1, &tr2, ArcVariant::Out, "".into()))?;
    //     executors.register::<SimpleTrans>(&tr1);
    //     executors.register::<SimpleTrans>(&tr2);
    // }
    let net = Arc::new(net);
    let config = ETCDConfigBuilder::default()
        .endpoints(["localhost:2379"])
        .prefix("/petri-test/")
        .node_name("node1")
        .region("test-region-1")
        .build()?;

    let etcd = ETCDGate::new(config);
    let run1 =
        petri_etcd_runner::runner::run(Arc::clone(&net), etcd, executors1, shutdown_token.clone());

    let config = ETCDConfigBuilder::default()
        .endpoints(["localhost:2379"])
        .prefix("/petri-test/")
        .node_name("node2")
        .region("test-region-2")
        .build()?;

    let etcd = ETCDGate::new(config);
    let run2 =
        petri_etcd_runner::runner::run(Arc::clone(&net), etcd, executors2, shutdown_token.clone());

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

    join_set.spawn(async {
        match run2.await {
            Ok(_) => {
                info!("Run2 finished")
            }
            Err(err) => {
                error!("Run2 finished with: {}", err)
            }
        }
    });

    join_set.join_all().await;
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

    return playground2().await;
}
