use crate::error::Result;
use crate::net::{NetChangeEvent, PetriNetBuilder, PetriNetIds, PlaceId, TransitionId};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use super::{PlaceLockClient, TransitionClient};

pub trait StorageClientConfig {
    fn prefix(&self) -> &str;
    fn node_name(&self) -> &str;
    fn region(&self) -> &str;
}

pub trait StorageClient {
    type TransitionClient: TransitionClient + Send + Sync;
    type PlaceLockClient: PlaceLockClient + Send + Sync;
    type Config: StorageClientConfig + Send + Sync;
    fn config(&self) -> &Self::Config;
    fn connect(
        &mut self,
        cancel_token: Option<CancellationToken>,
    ) -> impl std::future::Future<Output = Result<()>> + Send;
    fn disconnect(&mut self) -> impl std::future::Future<Output = ()> + Send;
    fn campaign_for_region(
        &mut self,
        tx_leader: watch::Sender<bool>,
    ) -> impl std::future::Future<Output = Result<()>> + Send;
    fn create_transition_client(
        &mut self,
        transition_id: TransitionId,
    ) -> Result<Self::TransitionClient>;
    fn assign_ids(
        &mut self,
        builder: &PetriNetBuilder,
    ) -> impl std::future::Future<Output = Result<PetriNetIds>> + Send;
    fn load_data(
        &mut self,
        tx_events: mpsc::Sender<NetChangeEvent>,
        place_ids: HashSet<PlaceId>,
        _transition_ids: HashSet<TransitionId>,
    ) -> impl std::future::Future<Output = Result<()>> + Send;
    fn place_lock_client(&mut self, pl_id: PlaceId) -> Result<Arc<Self::PlaceLockClient>>;
}
