use std::{
    cmp::min,
    collections::{HashMap, HashSet},
    fmt::Debug,
    str::from_utf8,
    sync::Arc,
    time::Duration,
};

use derive_builder::Builder;
use etcd_client::{
    Compare, CompareOp, EventType, GetOptions, KvClient, LeaseGrantOptions, PutOptions, Txn, TxnOp,
    WatchClient, WatchOptions,
};
use tokio::{join, select, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, trace, warn};

use crate::{
    error::{PetriError, Result},
    etcd::PlaceLock,
    net::{
        NetChange, NetChangeEvent, PetriNetBuilder, PetriNetIds, PlaceId, Revision, TokenId,
        TransitionId,
    },
};

use super::{ETCDTransitionGate, LeaseId, Version};

pub struct ETCDGate {
    pub(crate) config: ETCDConfig,
    cancel_token: Option<CancellationToken>,
    client: Option<etcd_client::Client>,
    lease: Option<LeaseId>,
    keep_alive_join_handle: Option<JoinHandle<()>>,
    watching_join_handle: Option<JoinHandle<()>>,
    lock_requests_handle: Option<JoinHandle<()>>,
    place_locks: HashMap<PlaceId, Arc<PlaceLock>>,
}

#[derive(Builder, Clone)]
pub struct ETCDConfig {
    #[builder(setter(custom))]
    pub endpoints: Vec<String>,
    #[builder(setter(custom), default = "\"/\".to_string()")]
    pub prefix: String,
    #[builder(setter(into))]
    pub node_name: String,
    #[builder(setter(into), default)]
    pub region: String,
    #[builder(setter(custom), default)]
    pub connect_options: Option<etcd_client::ConnectOptions>,
    #[builder(setter(strip_option), default)]
    pub lease_id: Option<LeaseId>,
    #[builder(default = "Duration::from_secs(10)")]
    pub lease_ttl: Duration,
}

impl ETCDConfigBuilder {
    pub fn endpoints<E: AsRef<str>, S: AsRef<[E]>>(&mut self, value: S) -> &mut Self {
        self.endpoints = Some(value.as_ref().iter().map(|ep| ep.as_ref().to_string()).collect());
        self
    }

    pub fn prefix<E: AsRef<str>>(&mut self, value: E) -> &mut Self {
        let mut value = value.as_ref().to_string();
        if !value.ends_with("/") {
            value += "/"
        }
        self.prefix = Some(value);
        self
    }
}

struct ETCDEvent<'a> {
    event_type: EventType,
    key: &'a [u8],
    value: &'a [u8],
    revision: Revision,
    version: Version,
}

impl Debug for ETCDEvent<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ETCDEvent")
            .field("event_type", &self.event_type)
            .field("key", &from_utf8(self.key).unwrap_or("<not valid utf8>"))
            .field("value", &from_utf8(self.value).unwrap_or("<not valid utf8>"))
            .field("revision", &self.revision.0)
            .field("version", &self.version.0)
            .finish()
    }
}

macro_rules! check_place_match {
    ($lhs:expr, $rhs:expr, $token_id:expr, $rev:expr) => {
        if ($lhs != $rhs) {
            Err(PetriError::InconsistentState(format!(
                "Token '{}' seems to be in two places at once ({} and {}) during revision {} ({}:{})",
                $token_id.0, $lhs.0, $rhs.0, $rev.0, file!(), line!()
            )))
        } else {
            Ok(())
        }
    };
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
impl ETCDGate {
    pub fn new(config: ETCDConfig) -> Self {
        ETCDGate {
            config,
            cancel_token: None,
            client: None,
            lease: None,
            keep_alive_join_handle: None,
            watching_join_handle: None,
            lock_requests_handle: None,
            place_locks: Default::default(),
        }
    }

    #[tracing::instrument(level = "info", skip(cancel_token, self))]
    pub async fn connect(&mut self, cancel_token: Option<CancellationToken>) -> Result<()> {
        let cancel_token = cancel_token.unwrap_or_default();
        self.cancel_token = Some(cancel_token.clone());
        let result: Result<()> = async move {
            let client = etcd_client::Client::connect(
                &self.config.endpoints,
                self.config.connect_options.clone(),
            )
            .await?;
            self.client = Some(client);
            self._get_lease().await?;
            self.keep_alive_join_handle = Some(tokio::spawn(ETCDGate::_keep_alive(
                self._cancel_token()?.clone(),
                self._client_mut()?.lease_client(),
                self._lease()?,
                self.config.lease_ttl,
            )));
            Ok(())
        }
        .await;
        // catch:
        result.inspect_err(|_| {
            cancel_token.cancel();
        })
    }

    #[tracing::instrument(level = "info")]
    pub async fn disconnect(&mut self) {
        if let Some(cancel_token) = self.cancel_token.as_ref() {
            cancel_token.cancel();
        }
        if let Some(join_handle) = self.keep_alive_join_handle.as_mut() {
            let _ = join!(join_handle); // we ignore any errors from inside that handle
        }
        if let Some(join_handle) = self.watching_join_handle.as_mut() {
            let _ = join!(join_handle); // we ignore any errors from inside that handle
        }
        if let Some(join_handle) = self.lock_requests_handle.as_mut() {
            let _ = join!(join_handle); // we ignore any errors from inside that handle
        }
        self.cancel_token = None;
        self.keep_alive_join_handle = None;
        self.watching_join_handle = None;
        self.lock_requests_handle = None;
        self.client = None;
        self.lease = None;
    }

    #[tracing::instrument(level = "info")]
    pub async fn campaign_for_region(&mut self) -> Result<()> {
        if self.config.region.is_empty() {
            return Err(PetriError::ConfigError(
                "Method 'campaign_for_region' requires a region to be set.".to_string(),
            ));
        }
        let election_name =
            self.config.prefix.clone() + "region/" + self.config.region.as_ref() + "/election";
        let lease_id = self._lease()?;
        let mut election_client = self._client_mut()?.election_client();
        let op = election_client.campaign(
            election_name.clone(),
            self.config.node_name.as_ref() as &str,
            lease_id.0,
        );
        let mut dur = Duration::from_secs(10);

        let waiter = async {
            loop {
                tokio::time::sleep(dur).await;
                info!(election_name, "Waiting on leadership.");
                dur = min(dur * 2, Duration::from_secs(300));
            }
        };
        let resp = select! {
            resp = op => {resp?}
            _ = self._cancel_token()?.cancelled() => {return Err(PetriError::Cancelled())}
            _ = waiter => {panic!("waiter should never finish.")}
        };
        if let Some(leader_key) = resp.leader() {
            let election_name =
                std::str::from_utf8(leader_key.name()).unwrap_or("<leader name decoding failed>");
            info!(election_name, "Became elected leader.",);
        } else {
            return Err(PetriError::ETCDError(etcd_client::Error::ElectError(
                "Leader election failed".to_string(),
            )));
        }
        Ok(())
    }

    pub fn create_transition_gate(
        &mut self,
        transition_id: TransitionId,
    ) -> Result<ETCDTransitionGate> {
        Ok(ETCDTransitionGate {
            client: self._client_mut()?.kv_client(),
            transition_id,
            prefix: self.config.prefix.clone(),
            lease: self._lease()?,
        })
    }
    #[tracing::instrument(level = "info", skip(builder), fields(transition_count = builder.transitions().len(), place_count = builder.transitions().len()) )]
    pub async fn assign_ids(&mut self, builder: &PetriNetBuilder) -> Result<PetriNetIds> {
        let cancel_token = self._cancel_token()?.clone();
        let op = async {
            let mut result = PetriNetIds::default();
            for tr_name in builder.transitions().keys() {
                let key = self.config.prefix.clone() + "transition_ids/" + tr_name;
                let tr_id = self._get_or_assign_id(key, "tr").await?;
                result.transitions.insert(tr_name.clone(), TransitionId(tr_id as u64));
                let key = self.config.prefix.clone() + "transition_ids/" + tr_name + "/region";
                self._check_or_assign_region(key).await?;
            }
            for pl_name in builder.places().keys() {
                let key = self.config.prefix.clone() + "place_ids/" + pl_name;
                let pl_id = self._get_or_assign_id(key, "pl").await?;
                result.places.insert(pl_name.clone(), PlaceId(pl_id as u64));
            }
            Ok::<PetriNetIds, PetriError>(result)
        };
        let result = select! {
            res = op => {res?},
            _ = cancel_token.cancelled() => {return Err(PetriError::Cancelled())}
        };

        Ok(result)
    }

    #[tracing::instrument(level = "info", skip(tx_events, place_ids, _transition_ids))]
    pub async fn load_data(
        &mut self,
        tx_events: tokio::sync::mpsc::Sender<NetChangeEvent>,
        place_ids: HashSet<PlaceId>,
        _transition_ids: HashSet<TransitionId>,
    ) -> Result<()> {
        let revision = self._create_initial_events(&tx_events, &place_ids).await?;
        let prefix = self.config.prefix.clone();
        let watch_client = self._client_mut()?.watch_client();
        let cancel_token = self._cancel_token()?.clone();
        let lease = self._lease()?;
        let (tx_locks, rx_locks) = tokio::sync::mpsc::channel(128);
        let mut place_locks = HashMap::<PlaceId, Arc<PlaceLock>>::new();
        for &pl_id in &place_ids {
            place_locks.insert(pl_id, self.place_lock(pl_id)?);
        }
        self.lock_requests_handle = Some(tokio::spawn(async move {
            let fut = ETCDGate::_lock_requests(place_locks, rx_locks);
            select! {
                res = fut =>{ match res {
                    Err(PetriError::Cancelled()) => {}
                    Err(err) => {error!("Handling lock requests failed with: {}.", err); cancel_token.cancel();}
                    _ => {}
                }}
                _ = cancel_token.cancelled() => {}
            }
        }));
        let cancel_token = self._cancel_token()?.clone();
        self.watching_join_handle = Some(tokio::spawn(async move {
            let watcher_fut = ETCDGate::_watch_events(
                prefix,
                revision,
                watch_client,
                tx_events,
                tx_locks,
                place_ids,
                cancel_token.clone(),
                lease,
            );
            tokio::pin!(watcher_fut);
            select! {
                res = watcher_fut =>{ match res {
                    Err(PetriError::Cancelled()) => {}
                    Err(err) => {error!("Watching etcd for changes failed with: {}.", err); cancel_token.cancel();}
                    _ => {}
                }}
                _ = cancel_token.cancelled() => {}
            }
        }));
        Ok(())
    }

    pub fn place_lock(&mut self, pl_id: PlaceId) -> Result<Arc<PlaceLock>> {
        let lease = self._lease()?;
        let prefix = &self.config.prefix;
        // Note: this seems inefficient (depending on clone cost)
        // Will hopefully not be called too often
        let client = self._client()?.clone();
        let place_lock = self.place_locks.entry(pl_id).or_insert_with(|| {
            Arc::new(PlaceLock::new(client.lock_client(), prefix.clone(), pl_id, lease))
        });
        Ok(place_lock.clone())
    }

    async fn _create_initial_events(
        &mut self,
        tx: &tokio::sync::mpsc::Sender<NetChangeEvent>,
        place_ids: &HashSet<PlaceId>,
    ) -> Result<Revision> {
        // construct initial event
        let mut initial_event = NetChangeEvent::new(Revision(0));
        initial_event.changes.push(NetChange::Reset());

        // query all data for the initial event
        let mut kv_client = self._client_mut()?.kv_client();
        let resp = kv_client
            .get(self.config.prefix.clone() + "pl/", Some(GetOptions::new().with_prefix()))
            .await?;
        let header =
            resp.header().ok_or(PetriError::Other("header missing from etcd reply.".into()))?;
        let revision = header.revision();

        let events: Vec<ETCDEvent> = resp
            .kvs()
            .iter()
            .map(|kv| ETCDEvent {
                event_type: EventType::Put,
                key: kv.key(),
                value: kv.value(),
                revision: revision.into(),
                version: 1.into(), // ensures that all events are 'created'  events
            })
            .collect();

        let (evts, _external_lock_requests) = ETCDGate::_analyze_place_events(
            self.config.prefix.clone(),
            &events,
            place_ids,
            self._lease()?,
        )?;
        // Note: we can ignore external lock requests here. They are only used to release locks on
        // our side, but we have not acquired any locks at this point.
        let cancel_token = self._cancel_token()?;
        cancelable_send!(tx, initial_event, cancel_token, "Net event change receiver")?;
        if evts.is_empty() {
            // Note: net waits for data loading. If this is the first run for this net, there might
            // not be any data to load. We send an empty event with revision 1.
            // The etcd cluster should always be at revision 1 at this point. Assigning ids to the
            // transitions requires revisions.
            let start_event = NetChangeEvent::new(Revision(1));
            cancelable_send!(tx, start_event, cancel_token, "Net event change receiver")?;
        } else {
            for evt in evts {
                cancelable_send!(tx, evt, cancel_token, "Net event change receiver")?;
            }
        }

        Ok(revision.into())
    }

    async fn _lock_requests(
        place_locks: HashMap<PlaceId, Arc<PlaceLock>>,
        mut rx_locks: tokio::sync::mpsc::Receiver<HashSet<PlaceId>>,
    ) -> Result<()> {
        while let Some(msg) = rx_locks.recv().await {
            let mut place_ids: Vec<PlaceId> = msg.iter().copied().collect();
            place_ids.sort();
            for pl_id in place_ids {
                let pl_lock = place_locks.get(&pl_id).ok_or(PetriError::ValueError(format!(
                    "no place lock for place id {}",
                    pl_id.0
                )))?;
                pl_lock.external_acquire().await?;
            }
            //
        }
        Err(PetriError::Cancelled())
    }

    #[allow(clippy::too_many_arguments)]
    async fn _watch_events(
        prefix: String,
        revision: Revision,
        mut watch_client: WatchClient,
        tx_events: tokio::sync::mpsc::Sender<NetChangeEvent>,
        tx_locks: tokio::sync::mpsc::Sender<HashSet<PlaceId>>,
        place_ids: HashSet<PlaceId>,
        cancel_token: CancellationToken,
        lease: LeaseId,
    ) -> Result<()> {
        let watch = watch_client.watch(
            prefix.clone() + "pl/",
            Some(
                WatchOptions::new()
                    .with_prefix()
                    .with_start_revision((revision.0 + 1) as i64)
                    .with_prev_key(),
            ),
        );
        let (_watcher, mut stream) = watch.await?;
        while let Some(resp) = stream.message().await? {
            let events: Vec<ETCDEvent> = resp
                .events()
                .iter()
                .map(|evt| ETCDEvent {
                    event_type: evt.event_type(),
                    key: evt.kv().unwrap().key(),
                    value: if evt.event_type() == EventType::Put {
                        evt.kv().unwrap().value()
                    } else {
                        evt.prev_kv().unwrap().value()
                    },
                    revision: evt.kv().unwrap().mod_revision().into(),
                    version: evt.kv().unwrap().version().into(),
                })
                .collect();

            let (evts, lock_requests) =
                ETCDGate::_analyze_place_events(prefix.clone(), &events, &place_ids, lease)?;
            for evt in evts {
                cancelable_send!(
                    tx_events,
                    evt,
                    cancel_token.clone(),
                    "Net event change receiver"
                )?;
            }
            if !lock_requests.is_empty() {
                cancelable_send!(
                    tx_locks,
                    lock_requests,
                    cancel_token.clone(),
                    "Lock request receiver"
                )?;
            }
        }
        Ok(())
    }

    fn _analyze_place_events(
        prefix: String,
        events: &[ETCDEvent],
        place_ids: &HashSet<PlaceId>,
        lease: LeaseId,
    ) -> Result<(Vec<NetChangeEvent>, HashSet<PlaceId>)> {
        // {prefix}/pl/{place-id}/{token-id} -> value is the token data, place id provides the position
        // {prefix}/pl/{place-id}/{token-id}/{transition-id} -> value is the token data, token has been taken by the given transition (leased, will be undone if cancelled)

        let mut change_events = Vec::new();
        let mut destroyed = HashMap::<TokenId, (PlaceId, Vec<u8>)>::new();
        let mut created = HashMap::<TokenId, (PlaceId, Vec<u8>)>::new();
        let mut updated = HashMap::<TokenId, (PlaceId, Vec<u8>)>::new();
        let mut taken = HashMap::<TokenId, (PlaceId, TransitionId)>::new();
        let mut released = HashMap::<TokenId, (PlaceId, TransitionId)>::new();
        let mut external_lock_requests = HashSet::<PlaceId>::new();
        for chunk in events.chunk_by(|e1, e2| e1.revision == e2.revision) {
            // a single revision

            //// debug code:
            // let chunk = chunk.iter().collect::<Vec<_>>();
            // warn!("New chunk: {:?}", chunk);

            let mut change_evt = NetChangeEvent::new(chunk.first().unwrap().revision);
            destroyed.clear();
            created.clear();
            updated.clear();
            taken.clear();
            released.clear();
            for evt in chunk {
                let split = ETCDGate::_split_key(&prefix, evt.key)?;
                if split.len() < 3 || split.len() > 4 {
                    continue;
                }
                assert!(split[0] == "pl");
                let place_id = match split[1].parse() {
                    Ok(v) => PlaceId(v),
                    Err(_) => continue,
                };
                if !place_ids.contains(&place_id) {
                    continue;
                }
                if split[2] == "lock" && split.len() == 4 {
                    if evt.event_type == EventType::Put {
                        if let Ok(lease_for_lock) = i64::from_str_radix(split[3], 16) {
                            if LeaseId(lease_for_lock) != lease {
                                external_lock_requests.insert(place_id);
                                // lock requested by someone else on a place used by us
                            }
                        }
                    }
                } else {
                    let token_id = match split[2].parse() {
                        Ok(v) => TokenId(v),
                        Err(_) => continue,
                    };
                    if split.len() > 3 {
                        let transition_id = match split[3].parse() {
                            Ok(v) => TransitionId(v),
                            Err(_) => continue,
                        };
                        match evt.event_type {
                            EventType::Put => {
                                taken.insert(token_id, (place_id, transition_id));
                            }
                            EventType::Delete => {
                                released.insert(token_id, (place_id, transition_id));
                            }
                        }
                    } else {
                        match evt.event_type {
                            EventType::Put => {
                                if evt.version == Version(1) {
                                    created.insert(token_id, (place_id, evt.value.into()));
                                } else {
                                    updated.insert(token_id, (place_id, evt.value.into()));
                                }
                            }
                            EventType::Delete => {
                                destroyed.insert(token_id, (place_id, evt.value.into()));
                            }
                        }
                    }
                }
            }

            // === possible cases ===
            // (A1) id taken && (released || destroyed || updated)
            //  -> ERROR
            // (A2) id taken && created -> initial loading
            //  -> EXTERNAL_UPDATE + EXTERNAL_PLACE + TAKE
            // (A3) id taken && NOT created -> transition started firing
            //  -> TAKE
            // == taken handled => NOT taken is implied below ==
            // (B1) id updated && (created || destroyed)
            //  -> ERROR
            // (B2) id updated && released -> transition finished (place on original place)
            //  -> UPDATE + PLACE
            // (B3) id updated && NOT released -> manual interaction
            //  -> EXTERNAL_UPDATE
            // == updated handled => NOT updated is implied below ==
            // (C1) id destroyed && released && created -> transition finished
            //  -> PLACE (if value changed, first UPDATE)
            // (C2) id destroyed && released && NOT created -> transition finished (token destroyed or moved outside known places)
            //  -> DELETE
            // (C3) id destroyed && NOT released && created -> manual interaction
            //  -> EXTERNAL_PLACE (if value changed, first EXTERNAL_UPDATE)
            // (C4) id destroyed && NOT released && NOT created -> manual interaction
            //  -> EXTERNAL_DELETE
            // == destroyed handled => NOT destroyed is implied below ==
            // (D1) id released && created
            //  -> ERROR
            // (D2) id released && NOT created  -> transition finished (place on original place)
            //  -> PLACE
            // == released handled => NOT released is implied below ==
            // (E1) id created -> transition finished (token created / moved from outside known places) / manual interaction
            //  -> EXTERNAL_UPDATE + EXTERNAL_PLACE

            // handling 'taken' (A cases)
            for (token_id, (place_id, transition_id)) in taken.drain() {
                if released.contains_key(&token_id)
                    || destroyed.contains_key(&token_id)
                    || updated.contains_key(&token_id)
                {
                    // case (A1)
                    return Err(PetriError::InconsistentState(format!(
                        "Token '{}' taken and further modified in a single revision",
                        token_id.0
                    )));
                } else if let Some((place2_id, data)) = created.remove(&token_id) {
                    // case (A2)
                    check_place_match!(place_id, place2_id, token_id, change_evt.revision)?;
                    change_evt.changes.push(NetChange::ExternalPlace(place_id, token_id));
                    change_evt.changes.push(NetChange::ExternalUpdate(token_id, data));
                } // else case (A3)
                change_evt.changes.push(NetChange::Take(place_id, transition_id, token_id));
            }

            // handling 'updated' (B cases)
            for (token_id, (place_id, data)) in updated.drain() {
                if destroyed.contains_key(&token_id) || created.contains_key(&token_id) {
                    // case (B1)
                    return Err(PetriError::InconsistentState(format!(
                        "Token '{}' has been updated AND created or destroyed in a single revision",
                        token_id.0
                    )));
                } else if let Some((place2_id, transition_id)) = released.remove(&token_id) {
                    check_place_match!(place_id, place2_id, token_id, change_evt.revision)?;
                    // case (B2)
                    change_evt.changes.push(NetChange::Update(transition_id, token_id, data));
                    change_evt.changes.push(NetChange::Place(place_id, transition_id, token_id));
                } else {
                    // case (B3)
                    change_evt.changes.push(NetChange::ExternalUpdate(token_id, data));
                }
            }

            // handling 'destroyed' (C cases)
            for (token_id, (place_id, data_destroyed)) in destroyed.drain() {
                let opt_transition_id =
                    if let Some((place2_id, transition_id)) = released.remove(&token_id) {
                        check_place_match!(place_id, place2_id, token_id, change_evt.revision)?;
                        Some(transition_id)
                    } else {
                        None
                    };

                let opt_created = created.remove(&token_id);

                match (opt_transition_id, opt_created) {
                    (Some(transition_id), Some((pl_created, data_created))) => {
                        // case (C1)
                        if data_destroyed != data_created {
                            let c = NetChange::Update(transition_id, token_id, data_created);
                            change_evt.changes.push(c);
                        }
                        let c = NetChange::Place(pl_created, transition_id, token_id);
                        change_evt.changes.push(c);
                    }
                    (Some(transition_id), None) => {
                        // case (C2)
                        let c = NetChange::Delete(place_id, transition_id, token_id);
                        change_evt.changes.push(c);
                    }
                    (None, Some((pl_created, data_created))) => {
                        // case (C3)
                        let c = NetChange::ExternalPlace(pl_created, token_id);
                        change_evt.changes.push(c);
                        if data_destroyed != data_created {
                            let c = NetChange::ExternalUpdate(token_id, data_created);
                            change_evt.changes.push(c);
                        }
                    }
                    (None, None) => {
                        // case (C4)
                        let c = NetChange::ExternalDelete(place_id, token_id);
                        change_evt.changes.push(c);
                    }
                }
            }

            // handling 'released' (D cases)
            for (token_id, (place_id, transition_id)) in released.drain() {
                if created.contains_key(&token_id) {
                    // case (D1)
                    return Err(PetriError::InconsistentState(format!(
                        "Token '{}' released and created in a single revision",
                        token_id.0
                    )));
                }
                // case (D2)
                change_evt.changes.push(NetChange::Place(place_id, transition_id, token_id));
            }

            // handling 'created' (E cases)
            for (token_id, (place_id, data)) in created.drain() {
                // case (E1)
                change_evt.changes.push(NetChange::ExternalPlace(place_id, token_id));
                change_evt.changes.push(NetChange::ExternalUpdate(token_id, data));
            }

            change_events.push(change_evt);
        }

        Ok((change_events, external_lock_requests))
    }

    fn _split_key<'a>(prefix: &str, key: &'a [u8]) -> Result<Vec<&'a str>> {
        let key = &from_utf8(key)?[prefix.len()..];
        Ok(key.split('/').collect())
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

    async fn _get_or_assign_id(&mut self, key: String, ktype: &str) -> Result<i64> {
        let mut kv = self._client_mut()?.kv_client();
        loop {
            let resp = kv.get(key.clone(), None).await?;
            let id: i64 = if resp.kvs().is_empty() {
                // safeguard with some kind of lock? unlikely to be a problem here
                let id = ETCDGate::_get_next_id(&mut kv, &self.config.prefix, ktype).await?;
                let txn = Txn::new()
                    // when 'key' has not yet been created ...
                    .when([Compare::create_revision(key.clone(), CompareOp::Less, 1)])
                    // ... put 'id' into the key
                    .and_then([TxnOp::put(key.clone(), id.to_string(), None)]);
                let resp = kv.txn(txn).await?;
                if !resp.succeeded() {
                    debug!(key, id, "Parallel id assignment, retrying...");
                    continue;
                }
                debug!(key, id, "Assigned id.");
                id
            } else {
                from_utf8(resp.kvs().first().unwrap().value())?.parse()?
            };
            return Ok(id);
        }
    }

    async fn _check_or_assign_region(&mut self, key: String) -> Result<()> {
        let mut kv = self._client_mut()?.kv_client();
        loop {
            let resp = kv.get(key.clone(), None).await?;
            if resp.kvs().is_empty() {
                let txn = Txn::new()
                    // when 'key' has not yet been created ...
                    .when([Compare::create_revision(key.clone(), CompareOp::Less, 1)])
                    // ... put the region into the key
                    .and_then([TxnOp::put(key.clone(), self.config.region.clone(), None)]);
                let resp = kv.txn(txn).await?;
                if !resp.succeeded() {
                    debug!(key, "Parallel region assignment, retrying...");
                    continue;
                }
                debug!(key, "Assigned region.");
            } else {
                let region = from_utf8(resp.kvs().first().unwrap().value())?;
                if region != self.config.region {
                    let config_region = &self.config.region;
                    return Err(PetriError::ValueError(format!("Region mismatch on transition '{key}': configured region='{config_region}', region on etcd='{region}'.")));
                }
            };
            break;
        }
        Ok(())
    }

    fn _client(&self) -> Result<&etcd_client::Client> {
        match self.client.as_ref() {
            Some(client) => Ok(client),
            None => Err(PetriError::NotConnected()),
        }
    }

    fn _client_mut(&mut self) -> Result<&mut etcd_client::Client> {
        match self.client.as_mut() {
            Some(client) => Ok(client),
            None => Err(PetriError::NotConnected()),
        }
    }

    fn _lease(&mut self) -> Result<LeaseId> {
        match self.lease {
            Some(lease) => Ok(lease),
            None => Err(PetriError::NotConnected()),
        }
    }

    fn _cancel_token(&self) -> Result<&CancellationToken> {
        match self.cancel_token.as_ref() {
            Some(token) => Ok(token),
            None => Err(PetriError::NotConnected()),
        }
    }

    async fn _keep_alive_loop(
        lease_client: &mut etcd_client::LeaseClient,
        lease_id: LeaseId,
        lease_ttl: Duration,
    ) -> Result<()> {
        let (mut keeper, mut stream) = lease_client.keep_alive(lease_id.0).await?;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<()>>(128);
        let fut_keep_alive = async move {
            loop {
                keeper.keep_alive().await?;
                if let Some(resp) = stream.message().await? {
                    if resp.ttl() as u64 != lease_ttl.as_secs() {
                        return Err(PetriError::ETCDError(
                            etcd_client::Error::LeaseKeepAliveError("Lease lost".to_string()),
                        ));
                    }
                    let resp_lease_id = resp.id();
                    trace!(resp_lease_id, "Keep alive.",);
                }
                let _ = tx.send(Ok(())).await;
                tokio::time::sleep(lease_ttl / 5).await;
            }
        };
        tokio::pin!(fut_keep_alive);

        loop {
            select! {
                res = &mut fut_keep_alive => {res?; return Ok(())}, // if there is an error, return it. If the keep alive loop ended, always return.
                _ = rx.recv() => {}, // successful keep alive, reset the timer
                _ = tokio::time::sleep(lease_ttl/2) => { // we want to fail fast and cease all operation before the lease expires.
                    return Err(PetriError::ETCDError(etcd_client::Error::LeaseKeepAliveError("Connection lost".to_string())));
                }
            }
        }
    }

    #[tracing::instrument(level = "info", skip(cancel_token, lease_client))]
    async fn _keep_alive(
        cancel_token: CancellationToken,
        mut lease_client: etcd_client::LeaseClient,
        lease_id: LeaseId,
        lease_ttl: Duration,
    ) {
        select! {
            res = ETCDGate::_keep_alive_loop(&mut lease_client, lease_id, lease_ttl) => {let _ = res.inspect_err(|err|{
                error!("Keep alive failed with: {}.", err)
            });}
            _ = cancel_token.cancelled() => {}
        };
        cancel_token.cancel();
        let wait_time = lease_ttl / 10;
        // give some time to shut everything else down.
        info!("Waiting {:#?} before revoking lease.", wait_time);
        tokio::time::sleep(lease_ttl / 10).await;
        if let Err(err) = lease_client.revoke(lease_id.0).await {
            warn!("Revoking lease failed with: {}.", err);
        } else {
            info!("Lease revoked.");
        }
    }

    #[tracing::instrument(level = "info")]
    async fn _get_lease(&mut self) -> Result<()> {
        let mut opts = LeaseGrantOptions::new();
        if let Some(lease_id) = self.config.lease_id {
            opts = opts.with_id(lease_id.0);
        }
        let grant_res = self
            ._client_mut()?
            .lease_client()
            .grant(self.config.lease_ttl.as_secs() as i64, Some(opts))
            .await;

        let grant_resp = grant_res.inspect_err(|_| {
            warn!("Granting a lease failed.");
        })?;

        let lease_id = grant_resp.id();
        let lease_ttl = grant_resp.ttl() as u64;
        self.lease = Some(LeaseId(lease_id));
        info!(lease_id, lease_ttl, "Lease granted.");
        Ok(())
    }

    // pub async fn ensure_net
}

impl Debug for ETCDGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ETCDGate").field("lease", &self.lease.unwrap_or(LeaseId(-1)).0).finish()
    }
}
