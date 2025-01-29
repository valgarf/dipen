use crate::error::Result;
use crate::net::{PlaceId, Revision, TokenId};
use crate::storage::FencingToken;

pub trait TransitionClient {
    fn start_transition(
        &mut self,
        take: impl IntoIterator<Item = (PlaceId, TokenId)> + Send,
        fencing_tokens: &[&FencingToken],
    ) -> impl std::future::Future<Output = Result<Revision>> + Send;

    fn end_transition(
        &mut self,
        place: impl IntoIterator<Item = (TokenId, PlaceId, PlaceId, Vec<u8>)> + Send,
        create: impl IntoIterator<Item = (PlaceId, Vec<u8>)> + Send,
        destroy: impl IntoIterator<Item = (PlaceId, TokenId)> + Send,
        fencing_tokens: &[&FencingToken],
    ) -> impl std::future::Future<Output = Result<Revision>> + Send;
}
