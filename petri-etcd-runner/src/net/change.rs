use super::{PlaceId, TokenId, TransitionId};

#[derive(Debug)]
pub enum NetChange {
    // changes during normal run (with transition that is responsible)
    Take(PlaceId, TransitionId, TokenId),
    Place(PlaceId, TransitionId, TokenId),
    Delete(PlaceId, TransitionId, TokenId),
    Update(TokenId, TransitionId, Vec<u8>),
    Reset(), // delete ALL tokens, for (re)loads
    // external interactions (transitions our code does not know about / manual interactions)
    ExternalPlace(PlaceId, TokenId),
    ExternalDelete(PlaceId, TokenId),
    ExternalUpdate(TokenId, Vec<u8>),
}

#[derive(Default, Debug)]
pub struct NetChangeEvent {
    pub changes: Vec<NetChange>,
    pub revision: u64,
}

impl NetChangeEvent {
    pub fn new(revision: u64) -> Self {
        NetChangeEvent { changes: Default::default(), revision }
    }
}
