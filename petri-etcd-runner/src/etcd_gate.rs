use std::{
    cmp::min,
    collections::{HashMap, HashSet},
    fmt::Debug,
    str::from_utf8,
    time::Duration,
};

use derive_builder::Builder;
use etcd_client::{
    DeleteOptions, EventType, GetOptions, KvClient, LeaseGrantOptions, PutOptions, Txn, TxnOp,
    WatchClient, WatchOptions,
};
use tokio::{join, select, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, trace, warn};

use crate::{
    error::{PetriError, Result},
    net::{
        NetChange, NetChangeEvent, PetriNetBuilder, PetriNetIds, PlaceId, TokenId, TransitionId,
    },
};
pub struct ETCDGate {
    pub(crate) config: ETCDConfig,
    cancel_token: Option<CancellationToken>,
    client: Option<etcd_client::Client>,
    lease: Option<i64>,
    keep_alive_join_handle: Option<JoinHandle<()>>,
    watching_join_handle: Option<JoinHandle<()>>,
}

pub struct ETCDTransitionGate {
    client: KvClient,
    transition_id: TransitionId,
    prefix: String,
    lease: i64,
}

#[derive(Builder)]
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
    pub lease_id: Option<i64>,
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
    revision: i64,
    version: i64,
}

macro_rules! check_place_match {
    ($lhs:expr, $rhs:expr, $token_id:expr, $rev:expr) => {
        if ($lhs != $rhs) {
            Err(PetriError::InconsistentState(format!(
                "Token '{}' seems to be in two places at once ({} and {}) during revision {} ({}:{})",
                $token_id.0, $lhs.0, $rhs.0, $rev, file!(), line!()
            )))
        } else {
            Ok(())
        }
    };
}

macro_rules! cancelable_send {
    ($tx: expr, $event:expr, $cancel_token:expr) => {
        if ($tx.send($event).await).is_err() {
            let cancel_token = $cancel_token;
            if !cancel_token.is_cancelled() {
                warn!("Net event change receiver has been dropped.");
                cancel_token.cancel();
            } else {
                debug!(
                    "Net event change receiver has been dropped, probably due to a cancellation."
                );
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
/// <prefix>/id_generator -> version of this field is used to create unique ids
/// <prefix>/region/<region-name>/election -> used as election for the given region. Child key values indicate running nodes.
/// <prefix>/place_ids/<place-name> -> value is id of that place
/// <prefix>/transition_ids/<transition-name> -> value is id of that transition
/// <prefix>/transition_ids/<transition-name>/region -> value is region of that transition
/// <prefix>/pl/<place-id>/<token-id> -> value is the token data, place id provides the position
/// <prefix>/pl/<place-id>/<token-id>/<transition-id> -> value currently unused, token has been taken by the given transition (leased, will be undone if cancelled)
/// <prefix>/pl/<place-id>/lock -> value is the node name (leased) TODO: use actual locking mechanism? or election?
/// <prefix>/pl/<place-id>/lock/<transition-id> -> value is the node name, used to request a lock. Lock owner should free it as soon as possible
/// <prefix>/tr/<region-name>/<transition-id>/request/<request-key> -> value is the input data, request to execute this manual transition.
/// <prefix>/tr/<region-name>/<transition-id>/request/<request-key>/response -> request has been fulfilled (transition has been executed). It may provide reponse data as value. Request owner should cle
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
                self._client()?.lease_client(),
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
        self.cancel_token = None;
        self.keep_alive_join_handle = None;
        self.watching_join_handle = None;
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
        let mut election_client = self._client()?.election_client();
        let op = election_client.campaign(
            election_name.clone(),
            self.config.node_name.as_ref() as &str,
            lease_id,
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
            client: self._client()?.kv_client(),
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
                let tr_id = self._get_or_assign_id(key).await?;
                result.transitions.insert(tr_name.clone(), TransitionId(tr_id as u64));
                let key = self.config.prefix.clone() + "transition_ids/" + tr_name + "/region";
                self._check_or_assign_region(key).await?;
            }
            for pl_name in builder.places().keys() {
                let key = self.config.prefix.clone() + "place_ids/" + pl_name;
                let pl_id = self._get_or_assign_id(key).await?;
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

    #[tracing::instrument(level = "info", skip(tx, place_ids, _transition_ids))]
    pub async fn load_data(
        &mut self,
        tx: tokio::sync::mpsc::Sender<NetChangeEvent>,
        place_ids: HashSet<PlaceId>,
        _transition_ids: HashSet<TransitionId>,
    ) -> Result<()> {
        let revision = self._create_initial_events(&tx, &place_ids).await?;
        let prefix = self.config.prefix.clone();
        let watch_client = self._client()?.watch_client();
        let cancel_token = self._cancel_token()?.clone();
        self.watching_join_handle = Some(tokio::spawn(async move {
            let watcher_fut = ETCDGate::_watch_events(
                prefix,
                revision,
                watch_client,
                tx,
                place_ids,
                cancel_token.clone(),
            );
            tokio::pin!(watcher_fut);
            select! {
                res = watcher_fut =>{ match res {
                    Err(PetriError::Cancelled()) => {}
                    Err(err) => {error!("Watching etcd for changes failed with {}.", err); cancel_token.cancel();}
                    _ => {}
                }}
                _ = cancel_token.cancelled() => {}
            }
        }));
        Ok(())
    }

    async fn _create_initial_events(
        &mut self,
        tx: &tokio::sync::mpsc::Sender<NetChangeEvent>,
        place_ids: &HashSet<PlaceId>,
    ) -> Result<i64> {
        // construct initial event
        let mut initial_event = NetChangeEvent::new(0);
        initial_event.changes.push(NetChange::Reset());

        // query all data for the initial event
        let mut kv_client = self._client()?.kv_client();
        let resp = kv_client
            .get(
                self.config.prefix.clone() + "pl/",
                Some(GetOptions::new().with_prefix().with_serializable()),
            )
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
                revision,
                version: 1, // ensures that all events are 'created'  events
            })
            .collect();

        let evts = ETCDGate::_analyze_place_events(self.config.prefix.clone(), &events, place_ids)?;
        let cancel_token = self._cancel_token()?;
        cancelable_send!(tx, initial_event, cancel_token)?;
        for evt in evts {
            cancelable_send!(tx, evt, cancel_token)?;
        }

        Ok(revision)
    }

    async fn _watch_events(
        prefix: String,
        revision: i64,
        mut watch_client: WatchClient,
        tx: tokio::sync::mpsc::Sender<NetChangeEvent>,
        place_ids: HashSet<PlaceId>,
        cancel_token: CancellationToken,
    ) -> Result<()> {
        let watch = watch_client.watch(
            prefix.clone() + "pl/",
            Some(WatchOptions::new().with_prefix().with_start_revision(revision).with_prev_key()),
        );
        let (_watcher, mut stream) = watch.await?;
        while let Some(resp) = stream.message().await? {
            let header =
                resp.header().ok_or(PetriError::Other("header missing from etcd reply.".into()))?;
            let revision = header.revision();
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
                    revision,
                    version: evt.kv().unwrap().version(),
                })
                .collect();

            let evts = ETCDGate::_analyze_place_events(prefix.clone(), &events, &place_ids)?;
            for evt in evts {
                cancelable_send!(tx, evt, cancel_token.clone())?;
            }
        }
        Ok(())
    }

    fn _analyze_place_events(
        prefix: String,
        events: &[ETCDEvent],
        place_ids: &HashSet<PlaceId>,
    ) -> Result<Vec<NetChangeEvent>> {
        // <prefix>/pl/<place-id>/<token-id> -> value is the token data, place id provides the position
        // <prefix>/pl/<place-id>/<token-id>/<transition-id> -> value is the token data, token has been taken by the given transition (leased, will be undone if cancelled)

        let mut result = Vec::new();
        let mut destroyed = HashMap::<TokenId, (PlaceId, Vec<u8>)>::new();
        let mut created = HashMap::<TokenId, (PlaceId, Vec<u8>)>::new();
        let mut updated = HashMap::<TokenId, (PlaceId, Vec<u8>)>::new();
        let mut taken = HashMap::<TokenId, (PlaceId, TransitionId)>::new();
        let mut released = HashMap::<TokenId, (PlaceId, TransitionId)>::new();
        for chunk in events.chunk_by(|e1, e2| e1.revision == e2.revision) {
            // a single revision
            let mut change_evt = NetChangeEvent::new(chunk.first().unwrap().revision as u64);
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
                            if evt.version == 1 {
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
                    change_evt.changes.push(NetChange::Update(token_id, transition_id, data));
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
                            let c = NetChange::Update(token_id, transition_id, data_created);
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

            result.push(change_evt);
        }

        Ok(result)
    }

    fn _split_key<'a>(prefix: &str, key: &'a [u8]) -> Result<Vec<&'a str>> {
        let key = &from_utf8(key)?[prefix.len()..];
        Ok(key.split('/').collect())
    }

    async fn _get_next_id(&mut self) -> Result<i64> {
        let counter_name = self.config.prefix.clone() + "id_generator";
        let resp =
            self._client()?.put(counter_name, [], Some(PutOptions::new().with_prev_key())).await?;
        let version = match resp.prev_key() {
            Some(key) => key.version() + 1,
            None => 1,
        };
        Ok(version)
    }

    async fn _get_or_assign_id(&mut self, key: String) -> Result<i64> {
        let mut kv = self._client()?.kv_client();
        let resp = kv.get(key.clone(), None).await?;
        let id: i64 = if resp.kvs().is_empty() {
            // safeguard with some kind of lock? unlikely to be a problem here
            let id = self._get_next_id().await?;
            kv.put(key.clone(), id.to_string(), None).await?;
            debug!(key, id, "Assigned id.");
            id
        } else {
            from_utf8(resp.kvs().first().unwrap().value())?.parse()?
        };
        Ok(id)
    }

    async fn _check_or_assign_region(&mut self, key: String) -> Result<()> {
        let mut kv = self._client()?.kv_client();
        let resp = kv.get(key.clone(), None).await?;
        if resp.kvs().is_empty() {
            // safeguard with some kind of lock? unlikely to be a problem here
            kv.put(key.clone(), self.config.region.clone(), None).await?;
            debug!(key, "Assigned region.");
        } else {
            let region = from_utf8(resp.kvs().first().unwrap().value())?;
            if region != self.config.region {
                let config_region = &self.config.region;
                return Err(PetriError::ValueError(format!("Region mismatch on transition '{key}': configured region='{config_region}', region on etcd='{region}'.")));
            }
        };
        Ok(())
    }

    fn _client(&mut self) -> Result<&mut etcd_client::Client> {
        match self.client.as_mut() {
            Some(client) => Ok(client),
            None => Err(PetriError::NotConnected()),
        }
    }

    fn _lease(&mut self) -> Result<i64> {
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
        lease_id: i64,
        lease_ttl: Duration,
    ) -> Result<()> {
        let (mut keeper, mut stream) = lease_client.keep_alive(lease_id).await?;
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
        lease_id: i64,
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
        debug!("Waiting {:#?} before revoking lease.", wait_time);
        tokio::time::sleep(lease_ttl / 10).await;
        if let Err(err) = lease_client.revoke(lease_id).await {
            warn!("Revoking lease failed with {}.", err);
        } else {
            debug!("Lease revoked.");
        }
    }

    #[tracing::instrument(level = "info")]
    async fn _get_lease(&mut self) -> Result<()> {
        let mut opts = LeaseGrantOptions::new();
        if let Some(lease_id) = self.config.lease_id {
            opts = opts.with_id(lease_id);
        }
        let grant_res = self
            ._client()?
            .lease_client()
            .grant(self.config.lease_ttl.as_secs() as i64, Some(opts))
            .await;

        let grant_resp = grant_res.inspect_err(|_| {
            warn!("Granting a lease failed.");
        })?;

        let lease_id = grant_resp.id();
        let lease_ttl = grant_resp.ttl() as u64;
        self.lease = Some(lease_id);
        info!(lease_id, lease_ttl, "Lease granted.");
        Ok(())
    }

    // pub async fn ensure_net
}

impl Debug for ETCDGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ETCDGate").field("lease", &self.lease.unwrap_or(-1)).finish()
    }
}

// pub struct ETCDTransitionGate {
//     client: KvClient,
//     prefix: String,
//     lease: i64,
// }

#[inline]
fn _token_path(prefix: &str, pl_id: PlaceId, to_id: TokenId) -> String {
    format!("{prefix}pl/{pl_id}/{to_id}", prefix = prefix, pl_id = pl_id.0, to_id = to_id.0)
}

#[inline]
fn _token_taken_path(prefix: &str, pl_id: PlaceId, to_id: TokenId, tr_id: TransitionId) -> String {
    format!("{to_path}/{tr_id}", to_path = _token_path(prefix, pl_id, to_id), tr_id = tr_id.0)
}
impl ETCDTransitionGate {
    pub async fn start_transition(
        &mut self,
        take: impl IntoIterator<Item = (TokenId, PlaceId)>,
    ) -> Result<()> {
        // TODO: check locks
        let txn = Txn::new().and_then(
            take.into_iter()
                .map(|(to_id, pl_id)| {
                    TxnOp::put(
                        _token_taken_path(&self.prefix, pl_id, to_id, self.transition_id),
                        "",
                        Some(PutOptions::new().with_lease(self.lease)),
                    )
                })
                .collect::<Vec<_>>(),
        );
        self.client.txn(txn).await?;
        Ok(())
    }

    pub async fn end_transition(
        &mut self,
        place: impl IntoIterator<Item = (TokenId, PlaceId, PlaceId, Vec<u8>)>,
    ) -> Result<()> {
        // TODO: check locks + token is still at place + token is taken by this transition
        let ops = place
            .into_iter()
            .flat_map(|(to_id, orig_pl_id, new_pl_id, data)| {
                let delete_op = if orig_pl_id != new_pl_id {
                    TxnOp::delete(
                        _token_path(&self.prefix, orig_pl_id, to_id),
                        Some(DeleteOptions::new().with_prefix()),
                    )
                } else {
                    TxnOp::delete(
                        _token_taken_path(&self.prefix, orig_pl_id, to_id, self.transition_id),
                        None,
                    )
                };
                let put_op = TxnOp::put(_token_path(&self.prefix, new_pl_id, to_id), data, None);
                [delete_op, put_op]
            })
            .collect::<Vec<_>>();
        let txn = Txn::new().and_then(ops);
        self.client.txn(txn).await?;
        Ok(())
    }
}
