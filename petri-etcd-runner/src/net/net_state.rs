use std::collections::HashMap;

use super::{
    Arc, NetChange, NetChangeEvent, Place, PlaceId, Token, TokenId, Transition, TransitionId,
};
use crate::{
    error::{PetriError, Result},
    net::common::TokenPosition,
};

#[derive(Default)]
pub struct PetriNet {
    pub(super) places: HashMap<PlaceId, Place>,
    pub(super) transitions: HashMap<TransitionId, Transition>,
    pub(super) arcs: HashMap<(PlaceId, TransitionId), Arc>,
    tokens: HashMap<TokenId, Token>,
    revision: u64,
}

macro_rules! get_place {
    ($self:expr, $place_id:expr) => {
        $self.places.get_mut(&$place_id).ok_or_else(|| {
            PetriError::InconsistentState(format!("Could not find place '{}'", $place_id.0))
        })
    };
}
macro_rules! get_transition {
    ($self:expr, $transition_id:expr) => {
        $self.transitions.get_mut(&$transition_id).ok_or_else(|| {
            PetriError::InconsistentState(format!(
                "Could not find transition '{}'",
                $transition_id.0
            ))
        })
    };
}

macro_rules! get_token {
    ($self:expr, $token_id:expr) => {
        $self.tokens.get_mut(&$token_id).ok_or_else(|| {
            PetriError::InconsistentState(format!("Could not find token '{}'", $token_id.0))
        })
    };
}

macro_rules! assert_state {
    ($value:expr, $msg:expr) => {
        if ($value) {
            Ok(())
        } else {
            Err(PetriError::InconsistentState($msg))
        }
    };
}

impl PetriNet {
    pub fn places(&self) -> &HashMap<PlaceId, Place> {
        &self.places
    }

    pub fn transitions(&self) -> &HashMap<TransitionId, Transition> {
        &self.transitions
    }

    pub fn arcs(&self) -> &HashMap<(PlaceId, TransitionId), Arc> {
        &self.arcs
    }

    pub fn tokens(&self) -> &HashMap<TokenId, Token> {
        &self.tokens
    }

    pub fn apply_change_event(&mut self, evt: NetChangeEvent) -> Result<()> {
        assert!(self.revision < evt.revision || self.revision == 0);
        for change in evt.changes {
            self._apply_change(change)?;
        }
        Ok(())
    }

    fn _apply_change(&mut self, change: NetChange) -> Result<()> {
        match change {
            NetChange::Take(pl_id, tr_id, to_id) => {
                let pl = get_place!(self, pl_id)?;
                let tr = get_transition!(self, tr_id)?;
                let to = get_token!(self, to_id)?;
                assert_state!(
                    to.position == TokenPosition::Place(pl_id),
                    format!("Token '{}' at '{:?}', expected '{}'.", to_id.0, to.position, pl_id.0)
                )?;
                assert_state!(
                    to.last_place == pl_id,
                    format!(
                        "Token '{}' last place was '{}', expected '{}'.",
                        to_id.0, to.last_place.0, pl_id.0
                    )
                )?;
                to.position = TokenPosition::Transition(tr_id);
                assert_state!(
                    pl.token_ids.remove(&to_id),
                    format!("Token '{}' not found at place '{}'.", to_id.0, pl_id.0)
                )?;
                assert_state!(
                    tr.token_ids.insert(to_id),
                    format!("Token '{}' already at transition '{}'.", to_id.0, tr_id.0)
                )?;
            }
            NetChange::Place(pl_id, tr_id, to_id) => {
                let pl = get_place!(self, pl_id)?;
                let tr = get_transition!(self, tr_id)?;
                let to = get_token!(self, to_id)?;
                assert_state!(
                    to.position == TokenPosition::Transition(tr_id),
                    format!("Token '{}' at '{:?}', expected '{}'.", to_id.0, to.position, tr_id.0)
                )?;
                to.position = TokenPosition::Place(pl_id);
                to.last_place = pl_id;
                assert_state!(
                    pl.token_ids.insert(to_id),
                    format!("Token '{}' already at place '{}'.", to_id.0, pl_id.0)
                )?;
                assert_state!(
                    tr.token_ids.remove(&to_id),
                    format!("Token '{}' not found at transition '{}'.", to_id.0, tr_id.0)
                )?;
            }
            NetChange::Delete(pl_id, tr_id, to_id) => {
                let tr = get_transition!(self, tr_id)?;
                let to = get_token!(self, to_id)?;
                assert_state!(
                    to.position == TokenPosition::Transition(tr_id),
                    format!("Token '{}' at '{:?}', expected '{}'.", to_id.0, to.position, tr_id.0)
                )?;
                assert_state!(
                    to.last_place == pl_id,
                    format!(
                        "Token '{}' last place was '{}', expected '{}'.",
                        to_id.0, to.last_place.0, pl_id.0
                    )
                )?;
                assert_state!(
                    tr.token_ids.remove(&to_id),
                    format!("Token '{}' not found at transition '{}'.", to_id.0, tr_id.0)
                )?;
                self.tokens.remove(&to_id);
            }
            NetChange::Update(to_id, tr_id, data) => {
                let to = get_token!(self, to_id)?;
                assert_state!(
                    to.position == TokenPosition::Transition(tr_id),
                    format!("Token '{}' at '{:?}', expected '{}'.", to_id.0, to.position, tr_id.0)
                )?;
                to.data = data;
            }
            NetChange::Reset() => {
                self.places.values_mut().for_each(|pl| pl.token_ids.clear());
                self.transitions.values_mut().for_each(|tr| tr.token_ids.clear());
                self.tokens.clear();
            }
            NetChange::ExternalPlace(pl_id, to_id) => {
                let to = self
                    .tokens
                    .entry(to_id)
                    .or_insert_with(|| Token::new(pl_id, TokenPosition::Place(pl_id)));
                if to.position != TokenPosition::Place(pl_id) {
                    match to.position.clone() {
                        TokenPosition::Place(old_pl_id) => {
                            let old_pl = get_place!(self, old_pl_id)?;
                            assert_state!(
                                old_pl.token_ids.remove(&to_id),
                                format!(
                                    "Token '{}' not found at place '{}'.",
                                    to_id.0, old_pl_id.0
                                )
                            )?;
                        }
                        TokenPosition::Transition(old_tr_id) => {
                            let old_tr = get_transition!(self, old_tr_id)?;
                            assert_state!(
                                old_tr.token_ids.remove(&to_id),
                                format!(
                                    "Token '{}' not found at transition '{}'.",
                                    to_id.0, old_tr_id.0
                                )
                            )?;
                        }
                    }
                    to.position = TokenPosition::Place(pl_id);
                }
                to.last_place = pl_id;
                let pl = get_place!(self, pl_id)?;
                pl.token_ids.insert(to_id); // we don't care if it already exists or not
            }
            NetChange::ExternalDelete(pl_id, to_id) => {
                let to = get_token!(self, to_id)?;
                assert_state!(
                    to.last_place == pl_id,
                    format!(
                        "Token '{}' last place was '{}', expected '{}'.",
                        to_id.0, to.last_place.0, pl_id.0
                    )
                )?;
                match to.position.clone() {
                    TokenPosition::Place(cur_pl_id) => {
                        assert_state!(
                            cur_pl_id == pl_id,
                            format!(
                                "Token '{}' position is place '{}', expected '{}'.",
                                to_id.0, cur_pl_id.0, pl_id.0
                            )
                        )?;
                        let pl = get_place!(self, pl_id)?;
                        assert_state!(
                            pl.token_ids.remove(&to_id),
                            format!("Token '{}' not found at place '{}'.", to_id.0, pl_id.0)
                        )?;
                    }
                    TokenPosition::Transition(cur_tr_id) => {
                        let cur_tr = get_transition!(self, cur_tr_id)?;
                        assert_state!(
                            cur_tr.token_ids.remove(&to_id),
                            format!(
                                "Token '{}' not found at transition '{}'.",
                                to_id.0, cur_tr_id.0
                            )
                        )?;
                    }
                }
                self.tokens.remove(&to_id);
            }
            NetChange::ExternalUpdate(to_id, data) => {
                let to = get_token!(self, to_id)?;
                to.data = data;
            }
        }
        Ok(())
    }
}
