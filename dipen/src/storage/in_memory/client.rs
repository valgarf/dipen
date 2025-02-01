use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use derive_builder::Builder;
use etcd_client::{KvClient, PutOptions};
use tokio::{join, select, sync::RwLock, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use tracing::info;

use super::InMemoryPlaceLock;
use crate::{
    error::{PetriError, Result},
    net::{
        NetChange, NetChangeEvent, PetriNetBuilder, PetriNetIds, PlaceId, Revision, TransitionId,
    },
    storage::{self},
};
use tracing::{debug, error, warn};

use super::InMemoryTransitionClient;

pub struct StorageClientData {
    place_locks: HashMap<PlaceId, Arc<InMemoryPlaceLock>>,
    revision: Arc<AtomicU64>,
    token_ids: Arc<AtomicU64>,
    tx_event_broadcast: tokio::sync::broadcast::Sender<NetChangeEvent>,
    net_ids: PetriNetIds,
    next_tr_id: AtomicU64,
    next_pl_id: AtomicU64,
}
pub struct InMemoryStorageClient {
    pub(crate) config: InMemoryConfig,
    cancel_token: Option<CancellationToken>,
    shared_data: Arc<RwLock<StorageClientData>>,
    watching_join_handle: Option<JoinHandle<()>>,
    rx_event_broadcast: tokio::sync::broadcast::Receiver<NetChangeEvent>,
}

#[derive(Builder, Clone)]
pub struct InMemoryConfig {
    #[builder(setter(custom), default = "\"/\".to_string()")]
    pub prefix: String,
    #[builder(setter(into))]
    pub node_name: String,
    #[builder(setter(into), default)]
    pub region: String,
}

macro_rules! cancelable_send {
    ($tx: expr, $event:expr, $cancel_token:expr, $rec_name:expr) => {
        if ($tx.send($event).await).is_err() {
            let cancel_token = $cancel_token;
            if !cancel_token.is_cancelled() {
                let msg = concat!($rec_name, " has been dropped.");
                warn!(msg);
                cancel_token.cancel();
            } else {
                let msg = concat!($rec_name, " has been dropped, probably due to a cancellation.");
                debug!(msg);
            }
            Err(PetriError::Cancelled())
        } else {
            Ok(())
        }
    };
}

impl InMemoryConfigBuilder {
    pub fn prefix<E: AsRef<str>>(&mut self, value: E) -> &mut Self {
        let mut value = value.as_ref().to_string();
        if !value.ends_with("/") {
            value += "/"
        }
        self.prefix = Some(value);
        self
    }
}

impl storage::traits::StorageClientConfig for InMemoryConfig {
    fn prefix(&self) -> &str {
        &self.prefix
    }

    fn node_name(&self) -> &str {
        &self.node_name
    }

    fn region(&self) -> &str {
        &self.region
    }
}

impl Clone for InMemoryStorageClient {
    /// Warning: Clones in memory client will only receive events AFTER cloning.
    /// Always create all clones before starting to run a net
    fn clone(&self) -> Self {
        // TODO: contraint could be enforced dynamically or - with a modified design - statically
        Self {
            config: self.config.clone(),
            cancel_token: None,
            watching_join_handle: None,
            shared_data: self.shared_data.clone(),
            rx_event_broadcast: self.rx_event_broadcast.resubscribe(),
        }
    }
}

impl InMemoryStorageClient {
    pub fn new(config: InMemoryConfig) -> Self {
        let (tx_event_broadcast, rx_event_broadcast) = tokio::sync::broadcast::channel(128);
        Self {
            config,
            cancel_token: None,
            watching_join_handle: None,
            shared_data: Arc::new(RwLock::new(StorageClientData {
                place_locks: Default::default(),
                revision: Arc::new(AtomicU64::new(2)),
                token_ids: Arc::new(AtomicU64::new(1)),
                tx_event_broadcast,
                net_ids: PetriNetIds::default(),
                next_tr_id: AtomicU64::new(1),
                next_pl_id: AtomicU64::new(1),
            })),
            rx_event_broadcast,
        }
    }

    /// Warning: Clones in memory client will only receive events AFTER cloning.
    /// Always create all clones before starting to run a net
    pub fn clone_with_config(&self, config: InMemoryConfig) -> Self {
        // TODO: contraint could be enforced dynamically or - with a modified design - statically
        Self {
            config,
            cancel_token: None,
            watching_join_handle: None,
            shared_data: self.shared_data.clone(),
            rx_event_broadcast: self.rx_event_broadcast.resubscribe(),
        }
    }

    async fn _create_initial_events(
        &mut self,
        tx: &tokio::sync::mpsc::Sender<NetChangeEvent>,
    ) -> Result<Revision> {
        // construct initial event
        let mut initial_event = NetChangeEvent::new(Revision(0));
        initial_event.changes.push(NetChange::Reset());

        // Note: we can ignore external lock requests here. They are only used to release locks on
        // our side, but we have not acquired any locks at this point.
        let cancel_token = self._cancel_token()?;
        cancelable_send!(tx, initial_event, cancel_token, "Net event change receiver")?;
        let start_event = NetChangeEvent::new(Revision(1));
        cancelable_send!(tx, start_event, cancel_token, "Net event change receiver")?;
        Ok(self.shared_data.read().await.revision.load(Ordering::SeqCst).into())
    }

    async fn _watch_events(
        tx_events: tokio::sync::mpsc::Sender<NetChangeEvent>,
        mut broadcast_receiver: tokio::sync::broadcast::Receiver<NetChangeEvent>,
    ) -> Result<()> {
        loop {
            let res = broadcast_receiver.recv().await;
            let evt = if let Ok(evt) = res {
                evt
            } else {
                return Err(PetriError::Cancelled());
            };
            let res = tx_events.send(evt).await;
            if res.is_err() {
                return Err(PetriError::Cancelled());
            }
        }
    }

    pub(super) async fn _get_next_id(kv: &mut KvClient, prefix: &str, ktype: &str) -> Result<i64> {
        let counter_name = [prefix, "id_generator/", ktype].concat();
        let resp = kv.put(counter_name, [], Some(PutOptions::new().with_prev_key())).await?;
        let version = match resp.prev_key() {
            Some(key) => key.version() + 1,
            None => 1,
        };
        Ok(version)
    }

    fn _cancel_token(&self) -> Result<&CancellationToken> {
        match self.cancel_token.as_ref() {
            Some(token) => Ok(token),
            None => Err(PetriError::NotConnected()),
        }
    }
}
/// Communication with etcd servers
///
/// Data Layout on etcd:
///
/// {prefix}/id_generator/pl -> version of this field is used to create unique ids for places
/// {prefix}/id_generator/tr -> ... for transitions
/// {prefix}/id_generator/to -> ... for tokens
/// {prefix}/region/{region-name}/election -> used as election for the given region. Child key values indicate running nodes.
/// {prefix}/place_ids/{place-name} -> value is id of that place
/// {prefix}/transition_ids/{transition-name} -> value is id of that transition
/// {prefix}/transition_ids/{transition-name}/region -> value is region of that transition
/// {prefix}/pl/{place-id}/{token-id} -> value is the token data, place id provides the position
/// {prefix}/pl/{place-id}/{token-id}/{transition-id} -> value currently unused, token has been taken by the given transition (leased, will be undone if cancelled)
/// {prefix}/pl/{place-id}/lock/{lease-id} -> Lock request for place by node identified by its lease. NOTE: locks are kept indefinitely. If a second lock request appears, the lock owner should free its lock as soon as possible.
/// {prefix}/tr/{region-name}/{transition-id}/request/{request-key} -> value is the input data, request to execute this manual transition. TODO: not yet implemented
/// {prefix}/tr/{region-name}/{transition-id}/request/{request-key}/response -> request has been fulfilled (transition has been executed). It may provide reponse data as value. Request owner should cler the field. TODO: not yet implemented
///
impl storage::traits::StorageClient for InMemoryStorageClient {
    type TransitionClient = InMemoryTransitionClient;
    type PlaceLockClient = InMemoryPlaceLock;
    type Config = InMemoryConfig;

    fn from_config(config: Self::Config) -> InMemoryStorageClient {
        InMemoryStorageClient::new(config)
    }

    fn config(&self) -> &Self::Config {
        &self.config
    }

    #[tracing::instrument(level = "info", skip(cancel_token, self))]
    async fn connect(&mut self, cancel_token: Option<CancellationToken>) -> Result<()> {
        let cancel_token = cancel_token.unwrap_or_default();
        self.cancel_token = Some(cancel_token.clone());
        Ok(())
    }

    #[tracing::instrument(level = "info")]
    async fn disconnect(&mut self) {
        if let Some(cancel_token) = self.cancel_token.as_ref() {
            cancel_token.cancel();
        }
        if let Some(join_handle) = self.watching_join_handle.as_mut() {
            let _ = join!(join_handle); // we ignore any errors from inside that handle
        }
        self.cancel_token = None;
        self.watching_join_handle = None;
        self.cancel_token = None;
    }

    #[tracing::instrument(level = "info", skip(tx_leader))]
    async fn campaign_for_region(
        &mut self,
        tx_leader: tokio::sync::watch::Sender<bool>,
    ) -> Result<()> {
        info!("Became elected leader for in memory client.",);
        // send errors are irrelevant here. In this case everything is shutting down anyway.
        let _ = tx_leader.send(true);
        Ok(())
    }

    async fn create_transition_client(
        &mut self,
        transition_id: TransitionId,
    ) -> Result<InMemoryTransitionClient> {
        let sd = self.shared_data.read().await;
        Ok(InMemoryTransitionClient {
            transition_id,
            tx_event_broadcast: sd.tx_event_broadcast.clone(),
            revision: Arc::clone(&sd.revision),
            token_ids: Arc::clone(&sd.token_ids),
        })
    }
    #[tracing::instrument(level = "info", skip(builder), fields(transition_count = builder.transitions().len(), place_count = builder.transitions().len()) )]
    async fn assign_ids(&mut self, builder: &PetriNetBuilder) -> Result<PetriNetIds> {
        let cancel_token = self._cancel_token()?.clone();
        let op = async {
            let mut sd = self.shared_data.write().await;
            // TODO: unnecessary clone because the borrow checker complains otherwise.
            // Can be fixed by changing the datastructure slightly
            let mut net_ids = sd.net_ids.clone();
            for tr_name in builder.transitions().keys() {
                net_ids
                    .transitions
                    .entry(tr_name.clone())
                    .or_insert_with(|| TransitionId(sd.next_tr_id.fetch_add(1, Ordering::SeqCst)));
            }
            for pl_name in builder.places().keys() {
                net_ids
                    .places
                    .entry(pl_name.clone())
                    .or_insert_with(|| PlaceId(sd.next_pl_id.fetch_add(1, Ordering::SeqCst)));
            }
            sd.net_ids = net_ids;
            Ok::<PetriNetIds, PetriError>(sd.net_ids.clone())
        };
        let result = select! {
            res = op => {res?},
            _ = cancel_token.cancelled() => {return Err(PetriError::Cancelled())}
        };

        Ok(result)
    }

    #[tracing::instrument(level = "info", skip(tx_events, _place_ids, _transition_ids))]
    async fn load_data(
        &mut self,
        tx_events: tokio::sync::mpsc::Sender<NetChangeEvent>,
        _place_ids: HashSet<PlaceId>,
        _transition_ids: HashSet<TransitionId>,
    ) -> Result<()> {
        let broadcast_receiver = self.shared_data.read().await.tx_event_broadcast.subscribe();
        let _revision = self._create_initial_events(&tx_events).await?;
        let cancel_token = self._cancel_token()?.clone();
        self.watching_join_handle = Some(tokio::spawn(async move {
            let watcher_fut = InMemoryStorageClient::_watch_events(tx_events, broadcast_receiver);
            tokio::pin!(watcher_fut);
            select! {
                res = watcher_fut =>{ match res {
                    Err(PetriError::Cancelled()) => {}
                    Err(err) => {error!("Watching in memory storage for changes failed with: {}.", err); cancel_token.cancel();}
                    _ => {}
                }}
                _ = cancel_token.cancelled() => {}
            }
        }));
        Ok(())
    }

    async fn place_lock_client(&mut self, pl_id: PlaceId) -> Result<Arc<InMemoryPlaceLock>> {
        // Note: this seems inefficient (depending on clone cost)
        // Will hopefully not be called too often
        let sd = self.shared_data.read().await;
        if let Some(pl_lock) = sd.place_locks.get(&pl_id) {
            Ok(pl_lock.clone())
        } else {
            drop(sd);
            let mut sd = self.shared_data.write().await;
            let pl_lock = sd
                .place_locks
                .entry(pl_id)
                .or_insert_with(|| Arc::new(InMemoryPlaceLock::new(pl_id)));
            Ok(pl_lock.clone())
        }
    }
}

impl Debug for InMemoryStorageClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemoryStorageClient").finish()
    }
}
