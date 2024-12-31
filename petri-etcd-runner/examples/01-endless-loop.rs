use std::sync::Arc;

use petri_etcd_runner::{
    error::Result as PetriResult,
    net,
    net::{ArcVariant, PetriNetBuilder, Place, Transition},
    runner::ExecutorRegistry,
    ETCDConfigBuilder, ETCDGate,
};
use tokio::signal;
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

    let mut executors = ExecutorRegistry::new();

    net.insert_place(Place::new("pl1", true))?;
    net.insert_place(Place::new("pl2", true))?;
    net.insert_transition(Transition::new("tr1", "region-1"))?;
    net.insert_transition(Transition::new("tr2", "region-1"))?;
    net.insert_arc(net::Arc::new("pl1", "tr1", ArcVariant::In, "".into()))?;
    net.insert_arc(net::Arc::new("pl2", "tr1", ArcVariant::Out, "".into()))?;
    net.insert_arc(net::Arc::new("pl2", "tr2", ArcVariant::In, "".into()))?;
    net.insert_arc(net::Arc::new("pl1", "tr2", ArcVariant::Out, "".into()))?;
    net.insert_transition(Transition::new("tr-init", "region-1"))?;
    net.insert_arc(net::Arc::new("pl1", "tr-init", ArcVariant::OutCond, "".into()))?;
    net.insert_arc(net::Arc::new("pl2", "tr-init", ArcVariant::Cond, "".into()))?;
    executors.register::<common::transitions::DelayedMove>("tr1", None);
    executors.register::<common::transitions::DelayedMove>("tr2", None);
    executors.register::<common::transitions::Initialize>("tr-init", None);

    let net = Arc::new(net);
    let config = ETCDConfigBuilder::default()
        .endpoints(["localhost:2379"])
        .prefix("/01-endless-loop/")
        .node_name("node1")
        .region("region-1")
        .build()?;

    let etcd = ETCDGate::new(config);
    let run =
        petri_etcd_runner::runner::run(Arc::clone(&net), etcd, executors, shutdown_token.clone());
    match run.await {
        Ok(_) => {}
        Err(err) => {
            error!("Run finished with error: {}", err);
        }
    }

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

    return run().await;
}
