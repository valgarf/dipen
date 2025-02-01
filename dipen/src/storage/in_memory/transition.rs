use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use crate::{
    error::Result,
    net::{NetChange, NetChangeEvent, PlaceId, Revision, TokenId, TransitionId},
    storage,
};

use crate::storage::FencingToken;

pub struct InMemoryTransitionClient {
    pub(super) transition_id: TransitionId,
    pub(super) tx_event_broadcast: tokio::sync::broadcast::Sender<NetChangeEvent>,
    pub(super) revision: Arc<AtomicU64>,
    pub(super) token_ids: Arc<AtomicU64>,
}

impl storage::traits::TransitionClient for InMemoryTransitionClient {
    async fn start_transition(
        &mut self,
        take: impl IntoIterator<Item = (PlaceId, TokenId)> + Send,
        _fencing_tokens: &[&FencingToken],
    ) -> Result<Revision> {
        let mut changes: Vec<NetChange> = vec![];
        for (pl_id, to_id) in take {
            changes.push(NetChange::Take(pl_id, self.transition_id, to_id));
        }
        let revision = Revision(self.revision.fetch_add(1, Ordering::SeqCst));
        let _ = self.tx_event_broadcast.send(NetChangeEvent { changes, revision });
        Ok(revision)
    }

    async fn end_transition(
        &mut self,
        place: impl IntoIterator<Item = (TokenId, PlaceId, PlaceId, Vec<u8>)> + Send,
        create: impl IntoIterator<Item = (PlaceId, Vec<u8>)> + Send,
        destroy: impl IntoIterator<Item = (PlaceId, TokenId)> + Send,
        _fencing_tokens: &[&FencingToken],
    ) -> Result<Revision> {
        let mut changes: Vec<NetChange> = vec![];
        for (to_id, _, pl_id, data) in place {
            changes.push(NetChange::Update(self.transition_id, to_id, data));
            changes.push(NetChange::Place(pl_id, self.transition_id, to_id));
        }
        for (pl_id, data) in create {
            let to_id = TokenId(self.token_ids.fetch_add(1, Ordering::SeqCst));
            changes.push(NetChange::ExternalPlace(pl_id, to_id));
            changes.push(NetChange::ExternalUpdate(to_id, data));
        }
        for (pl_id, to_id) in destroy {
            changes.push(NetChange::Delete(pl_id, self.transition_id, to_id));
        }

        let revision = Revision(self.revision.fetch_add(1, Ordering::SeqCst));
        let _ = self.tx_event_broadcast.send(NetChangeEvent { changes, revision });
        Ok(revision)
    }
}
