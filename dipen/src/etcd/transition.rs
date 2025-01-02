use etcd_client::{Compare, CompareOp, DeleteOptions, KvClient, Txn, TxnOp};

use crate::{
    error::{PetriError, Result},
    net::{PlaceId, Revision, TokenId, TransitionId},
};

use super::{ETCDGate, FencingToken, LeaseId};

pub struct ETCDTransitionGate {
    pub(super) client: KvClient,
    pub(super) transition_id: TransitionId,
    pub(super) prefix: String,
    pub(super) lease: LeaseId,
}

impl ETCDTransitionGate {
    pub async fn start_transition(
        &mut self,
        take: impl IntoIterator<Item = (PlaceId, TokenId)>,
        fencing_tokens: &[&FencingToken],
    ) -> Result<Revision> {
        let txn = Txn::new()
            .when(
                fencing_tokens
                    .iter()
                    .map(|&fto| Compare::lease(fto.0.clone(), CompareOp::Equal, self.lease.0))
                    .collect::<Vec<Compare>>(),
            )
            .and_then(
                take.into_iter()
                    .map(|(pl_id, to_id)| {
                        TxnOp::put(
                            _token_taken_path(&self.prefix, pl_id, to_id, self.transition_id),
                            "",
                            None,
                            //Some(PutOptions::new().with_lease(self.lease)),
                        )
                    })
                    .collect::<Vec<_>>(),
            );
        let res = self.client.txn(txn).await?;
        if res.succeeded() {
            Ok(res
                .header()
                .expect("Missing header from etcd in 'start_transition' call.")
                .revision()
                .into())
        } else {
            Err(PetriError::InconsistentState(format!(
                "Failed to execute transaction on etcd to start transition '{}' (lease lost?)",
                self.transition_id.0
            )))
        }
    }

    pub async fn end_transition(
        &mut self,
        place: impl IntoIterator<Item = (TokenId, PlaceId, PlaceId, Vec<u8>)>,
        create: impl IntoIterator<Item = (PlaceId, Vec<u8>)>,
        destroy: impl IntoIterator<Item = (PlaceId, TokenId)>,
        fencing_tokens: &[&FencingToken],
    ) -> Result<Revision> {
        // check if token is still at place? Should not happen if locks work correctly
        let cond = fencing_tokens
            .iter()
            .map(|&fto| Compare::lease(fto.0.clone(), CompareOp::Equal, self.lease.0))
            .collect::<Vec<Compare>>();

        let mut new_tokens = vec![];
        for (pl_id, data) in create {
            let new_id = ETCDGate::_get_next_id(&mut self.client, &self.prefix, "to").await?;
            new_tokens.push((TokenId(new_id as u64), pl_id, data));
        }
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
            .chain(new_tokens.into_iter().map(|(to_id, pl_id, data)| {
                TxnOp::put(_token_path(&self.prefix, pl_id, to_id), data, None)
            }))
            .chain(destroy.into_iter().map(|(pl_id, to_id)| {
                TxnOp::delete(
                    _token_path(&self.prefix, pl_id, to_id),
                    Some(DeleteOptions::new().with_prefix()),
                )
            }))
            .collect::<Vec<_>>();

        let txn = Txn::new().when(cond).and_then(ops);
        let resp = self.client.txn(txn).await?;
        if !resp.succeeded() {
            return Err(PetriError::InconsistentState(format!(
                "Failed to execute transaction on etcd to end transition '{}' (lease lost?)",
                self.transition_id.0
            )));
        }
        let header =
            resp.header().ok_or(PetriError::Other("header missing from etcd reply.".into()))?;
        Ok(header.revision().into())
    }
}

#[inline]
fn _token_path(prefix: &str, pl_id: PlaceId, to_id: TokenId) -> String {
    format!("{prefix}pl/{pl_id}/{to_id}", prefix = prefix, pl_id = pl_id.0, to_id = to_id.0)
}

#[inline]
fn _token_taken_path(prefix: &str, pl_id: PlaceId, to_id: TokenId, tr_id: TransitionId) -> String {
    format!("{to_path}/{tr_id}", to_path = _token_path(prefix, pl_id, to_id), tr_id = tr_id.0)
}
