use std::{fmt::Display, str::from_utf8};

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

impl Display for NetChangeEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "revision={}, changes=[", self.revision)?;
        for (idx, change) in self.changes.iter().enumerate() {
            if idx == 0 {
                write!(f, "{}", change)?;
            } else {
                write!(f, ", {}", change)?;
            }
        }
        write!(f, "]")?;
        Ok(())
    }
}

impl Display for NetChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetChange::Take(place_id, transition_id, token_id) => {
                write!(f, "Take({}: {} <- {})", token_id.0, transition_id.0, place_id.0)
            }
            NetChange::Place(place_id, transition_id, token_id) => {
                write!(f, "Place({}: {} -> {})", token_id.0, transition_id.0, place_id.0)
            }
            NetChange::Delete(place_id, transition_id, token_id) => {
                write!(
                    f,
                    "Delete({} at transition {} (<- {})",
                    token_id.0, transition_id.0, place_id.0,
                )
            }
            NetChange::Update(token_id, transition_id, data) => {
                write!(
                    f,
                    "Update({} at transition {}: '{}')",
                    token_id.0,
                    transition_id.0,
                    from_utf8(data).unwrap_or("<not valid utf8>")
                )
            }
            NetChange::Reset() => write!(f, "Reset()"),
            NetChange::ExternalPlace(place_id, token_id) => {
                write!(f, "ExternalPlace({}: ? -> {})", token_id.0, place_id.0)
            }
            NetChange::ExternalDelete(place_id, token_id) => {
                write!(f, "ExternalDelete({} at place {})", token_id.0, place_id.0)
            }
            NetChange::ExternalUpdate(token_id, data) => {
                write!(
                    f,
                    "ExternalUpdate({} at ?: '{}')",
                    token_id.0,
                    from_utf8(data).unwrap_or("<not valid utf8>")
                )
            }
        }
    }
}
