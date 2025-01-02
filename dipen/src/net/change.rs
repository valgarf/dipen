use std::{fmt::Display, str::from_utf8};

use unicode_segmentation::UnicodeSegmentation;

use super::{common::Revision, PlaceId, TokenId, TransitionId};

#[derive(Debug)]
pub enum NetChange {
    // changes during normal run (with transition that is responsible)
    Take(PlaceId, TransitionId, TokenId),
    Place(PlaceId, TransitionId, TokenId),
    Delete(PlaceId, TransitionId, TokenId),
    Update(TransitionId, TokenId, Vec<u8>),
    Reset(), // delete ALL tokens, for (re)loads
    // external interactions (transitions our code does not know about / manual interactions)
    ExternalPlace(PlaceId, TokenId),
    ExternalDelete(PlaceId, TokenId),
    ExternalUpdate(TokenId, Vec<u8>),
}

#[derive(Default, Debug)]
pub struct NetChangeEvent {
    pub changes: Vec<NetChange>,
    pub revision: Revision,
}

impl NetChangeEvent {
    pub fn new(revision: Revision) -> Self {
        NetChangeEvent { changes: Default::default(), revision }
    }
}

impl Display for NetChangeEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "revision={}, changes=[", self.revision.0)?;
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

fn _data_as_string(data: &[u8], max_len: usize) -> String {
    let s = from_utf8(data).unwrap_or("<not valid utf8>");
    if s.len() <= max_len {
        // Note: length in bytes, but each grapheme must have one byte at least.
        return s.into();
    }
    let mut graphemes = s.graphemes(true).take(max_len + 1).collect::<Vec<_>>();
    if graphemes.len() > max_len {
        graphemes.remove(max_len);
        graphemes[max_len - 1] = ".";
        graphemes[max_len - 2] = ".";
        graphemes[max_len - 3] = ".";
    }

    graphemes.concat()
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
            NetChange::Update(transition_id, token_id, data) => {
                write!(
                    f,
                    "Update({} at transition {}: '{}')",
                    token_id.0,
                    transition_id.0,
                    _data_as_string(data, 100)
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
                write!(f, "ExternalUpdate({} at ?: '{}')", token_id.0, _data_as_string(data, 100))
            }
        }
    }
}
