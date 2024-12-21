use std::time::Duration;

use etcd_client::{
    Client, ElectionClient, Error, EventType, LeaseClient, LeaseGrantOptions, WatchClient,
    WatchOptions,
};
use petri_etcd_runner::{
    error::Result as PetriResult,
    net::{Arc, PetriNetBuilder, Place, Transition},
    ETCDConfigBuilder, ETCDGate,
};
use tokio::{select, signal, time::sleep};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
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
            warn!("Shutting down. Granting a lease failed with {}.", err);
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
            warn!("Shutting down. Keep alive failed with {}.", err)
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
                warn!("Shutting down. Election failed with {}.", err);
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
                    warn!("Shutting down. Watching failed with {}.", err);
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
        warn!("Revoking lease failed with {}.", err);
    } else {
        debug!("Lease revoked.");
    }

    info!("Bye.");
    Ok(())
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
    net.insert_place(Place::new("pl1"))?;
    net.insert_place(Place::new("pl2"))?;
    net.insert_transition(Transition::new("tr1", "test-region"))?;
    net.insert_transition(Transition::new("tr2", "test-region"))?;
    net.insert_arc(Arc::new("pl1", "tr1"))?;
    net.insert_arc(Arc::new("pl2", "tr2"))?;
    let config = ETCDConfigBuilder::default()
        .endpoints(["localhost:2379"])
        .prefix("/petri-test/")
        .node_name("node1")
        .region("test-region")
        .build()?;
    let etcd = ETCDGate::new(config);
    petri_etcd_runner::runner::run(&net, etcd, shutdown_token.clone()).await?;

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
        .with_env_filter(EnvFilter::try_new("info,petri_etcd_runner=trace").unwrap())
        .init();

    return playground2().await;
}
