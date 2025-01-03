use std::sync::Arc;

use dipen::{
    error::Result as PetriResult,
    etcd::{ETCDConfigBuilder, ETCDGate},
    net,
    net::{ArcVariant, PetriNetBuilder, Place, Transition},
    runner::ExecutorRegistry,
};
use tokio::{signal, task::JoinSet};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

#[path = "common/mod.rs"]
mod common;

#[tracing::instrument(level = "info")]
async fn run() -> PetriResult<()> {
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

    net.insert_place(Place::new("pl1", true));
    net.insert_place(Place::new("pl2", true));
    net.insert_transition(Transition::new("tr1", "region-1"));
    net.insert_transition(Transition::new("tr2", "region-2"));
    net.insert_arc(net::Arc::new("pl1", "tr1", ArcVariant::In, ""))?;
    net.insert_arc(net::Arc::new("pl2", "tr1", ArcVariant::Out, ""))?;
    net.insert_arc(net::Arc::new("pl2", "tr2", ArcVariant::In, ""))?;
    net.insert_arc(net::Arc::new("pl1", "tr2", ArcVariant::Out, ""))?;
    net.insert_transition(Transition::new("tr-init", "region-1"));
    net.insert_arc(net::Arc::new("pl1", "tr-init", ArcVariant::OutCond, ""))?;
    net.insert_arc(net::Arc::new("pl2", "tr-init", ArcVariant::Cond, ""))?;
    executors1.register::<common::transitions::DelayedMove>("tr1", None);
    executors2.register::<common::transitions::DelayedMove>("tr2", None);
    executors1.register::<common::transitions::Initialize>("tr-init", None);

    let net = Arc::new(net);
    let config = ETCDConfigBuilder::default()
        .endpoints(["localhost:2379"])
        .prefix("02-split-loop/")
        .node_name("node1")
        .region("region-1")
        .build()?;

    let etcd = ETCDGate::new(config);
    let run1 = dipen::runner::run(Arc::clone(&net), etcd, executors1, shutdown_token.clone());

    let net = Arc::new(net);
    let config = ETCDConfigBuilder::default()
        .endpoints(["localhost:2379"])
        .prefix("02-split-loop/")
        .node_name("node2")
        .region("region-2")
        .build()?;

    let etcd = ETCDGate::new(config);
    let run2 = dipen::runner::run(Arc::clone(&net), etcd, executors2, shutdown_token.clone());

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
        .with_env_filter(EnvFilter::try_new("info,dipen=debug").unwrap())
        .init();

    return run().await;
}
