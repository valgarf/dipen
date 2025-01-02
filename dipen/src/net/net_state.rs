use std::collections::{BTreeMap, HashMap, HashSet};

use super::{
    common::Revision, Arc, NetChange, NetChangeEvent, Place, PlaceId, Token, TokenId, Transition,
    TransitionId,
};
use crate::{
    error::{PetriError, Result},
    net::common::TokenPosition,
};

#[derive(Default)]
pub struct PetriNet {
    // TODO: also keep a reference to currently 'taken' tokens in each place
    pub(super) places: HashMap<PlaceId, Place>,
    pub(super) transitions: HashMap<TransitionId, Transition>,
    pub(super) arcs: BTreeMap<(TransitionId, PlaceId), Arc>,
    tokens: HashMap<TokenId, Token>,
    revision: Revision,
}

macro_rules! get_place {
    ($self:expr, $place_id:expr) => {
        $self.places.get_mut(&$place_id).ok_or_else(|| {
            PetriError::InconsistentState(format!("Could not find place '{}'", $place_id.0))
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
            Err(PetriError::InconsistentState(format!("{} ({}:{})", $msg, file!(), line!())))
        }
    };
}

impl PetriNet {
    pub fn place(&self, pl_id: PlaceId) -> Option<&Place> {
        self.places.get(&pl_id)
    }

    pub fn places(&self) -> impl Iterator<Item = (PlaceId, &Place)> {
        self.places.iter().map(|(&pl_id, pl)| (pl_id, pl))
    }

    pub fn transition(&self, tr_id: TransitionId) -> Option<&Transition> {
        self.transitions.get(&tr_id)
    }

    pub fn transitions(&self) -> impl Iterator<Item = (TransitionId, &Transition)> {
        self.transitions.iter().map(|(&tr_id, tr)| (tr_id, tr))
    }

    pub fn arcs(&self) -> impl Iterator<Item = (TransitionId, PlaceId, &Arc)> {
        self.arcs.iter().map(|(&(tr_id, pl_id), arc)| (tr_id, pl_id, arc))
    }

    pub fn arcs_for(&self, tr_id: TransitionId) -> impl Iterator<Item = (PlaceId, &Arc)> {
        self.arcs
            .range((tr_id, PlaceId(0))..(TransitionId(tr_id.0 + 1), PlaceId(0)))
            .map(|(&(_, pl_id), arc)| (pl_id, arc))
    }

    pub fn token(&self, to_id: TokenId) -> Option<&Token> {
        self.tokens.get(&to_id)
    }

    pub fn tokens(&self) -> impl Iterator<Item = (TokenId, &Token)> {
        self.tokens.iter().map(|(&to_id, to)| (to_id, to))
    }

    pub fn revision(&self) -> Revision {
        self.revision
    }

    /// apply a change event (with a list of changes) to the net and return modified places
    pub fn apply_change_event(&mut self, evt: NetChangeEvent) -> Result<HashSet<PlaceId>> {
        assert!(self.revision < evt.revision || self.revision == Revision(0));
        let mut modified_places = HashSet::<PlaceId>::new();
        for change in evt.changes {
            modified_places.extend(self._apply_change(change)?);
        }
        self.revision = evt.revision;
        Ok(modified_places)
    }

    /// apply a change to the net and return modified places
    fn _apply_change(&mut self, change: NetChange) -> Result<HashSet<PlaceId>> {
        let mut modified = HashSet::new();
        match change {
            NetChange::Take(pl_id, tr_id, to_id) => {
                let pl = get_place!(self, pl_id)?;
                let tr = self.transitions.get_mut(&tr_id);
                let to = get_token!(self, to_id)?;
                assert_state!(
                    to.position == TokenPosition::Place(pl_id),
                    format!(
                        "Token '{}' at '{:?}', expected place '{}'.",
                        to_id.0, to.position, pl_id.0
                    )
                )?;
                assert_state!(
                    to.last_place == pl_id,
                    format!(
                        "Token '{}' last place was '{}', expected place'{}'.",
                        to_id.0, to.last_place.0, pl_id.0
                    )
                )?;
                to.position = TokenPosition::Transition(tr_id);
                assert_state!(
                    pl.token_ids.remove(&to_id),
                    format!("Token '{}' not found at place '{}'.", to_id.0, pl_id.0)
                )?;
                assert_state!(
                    pl.taken_token_ids.insert(to_id, tr_id).is_none(),
                    format!("Token '{}' already taken from place '{}'.", to_id.0, pl_id.0)
                )?;

                if let Some(tr) = tr {
                    assert_state!(
                        tr.token_ids.insert(to_id),
                        format!("Token '{}' already at transition '{}'.", to_id.0, tr_id.0)
                    )?;
                }
                modified.insert(pl_id);
            }
            NetChange::Place(pl_id, tr_id, to_id) => {
                let tr = self.transitions.get_mut(&tr_id);
                let to = get_token!(self, to_id)?;
                let old_pl = get_place!(self, to.last_place)?;
                modified.insert(to.last_place);
                assert_state!(
                    to.position == TokenPosition::Transition(tr_id),
                    format!(
                        "Token '{}' at '{:?}', expected transition '{}'.",
                        to_id.0, to.position, tr_id.0
                    )
                )?;
                assert_state!(
                    old_pl.taken_token_ids.remove(&to_id).is_some(),
                    format!("Token '{}' not taken from place '{}'.", to_id.0, to.last_place.0)
                )?;
                to.position = TokenPosition::Place(pl_id);
                to.last_place = pl_id;
                let pl = get_place!(self, pl_id)?;
                assert_state!(
                    pl.token_ids.insert(to_id),
                    format!("Token '{}' already at place '{}'.", to_id.0, pl_id.0)
                )?;
                if let Some(tr) = tr {
                    assert_state!(
                        tr.token_ids.remove(&to_id),
                        format!("Token '{}' not found at transition '{}'.", to_id.0, tr_id.0)
                    )?;
                }
                modified.insert(pl_id);
            }
            NetChange::Delete(pl_id, tr_id, to_id) => {
                let tr = self.transitions.get_mut(&tr_id);
                let to = get_token!(self, to_id)?;
                let old_pl = get_place!(self, to.last_place)?;
                assert_state!(
                    to.position == TokenPosition::Transition(tr_id),
                    format!(
                        "Token '{}' at '{:?}', expected transition '{}'.",
                        to_id.0, to.position, tr_id.0
                    )
                )?;
                assert_state!(
                    to.last_place == pl_id,
                    format!(
                        "Token '{}' last place was '{}', expected place '{}'.",
                        to_id.0, to.last_place.0, pl_id.0
                    )
                )?;
                assert_state!(
                    old_pl.taken_token_ids.remove(&to_id).is_some(),
                    format!("Token '{}' not taken from place '{}'.", to_id.0, to.last_place.0)
                )?;
                if let Some(tr) = tr {
                    assert_state!(
                        tr.token_ids.remove(&to_id),
                        format!("Token '{}' not found at transition '{}'.", to_id.0, tr_id.0)
                    )?;
                }
                self.tokens.remove(&to_id);
                modified.insert(pl_id);
            }
            NetChange::Update(tr_id, to_id, data) => {
                let to = get_token!(self, to_id)?;
                assert_state!(
                    to.position == TokenPosition::Transition(tr_id),
                    format!(
                        "Token '{}' at '{:?}', expected transition '{}'.",
                        to_id.0, to.position, tr_id.0
                    )
                )?;
                to.data = data;
                modified.insert(to.last_place);
            }
            NetChange::Reset() => {
                self.places.values_mut().for_each(|pl| pl.token_ids.clear());
                self.transitions.values_mut().for_each(|tr| tr.token_ids.clear());
                self.tokens.clear();
                modified.extend(self.places.keys());
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
                            let old_tr = self.transitions.get_mut(&old_tr_id);
                            if let Some(old_tr) = old_tr {
                                assert_state!(
                                    old_tr.token_ids.remove(&to_id),
                                    format!(
                                        "Token '{}' not found at transition '{}'.",
                                        to_id.0, old_tr_id.0
                                    )
                                )?;
                            }
                        }
                    }
                    to.position = TokenPosition::Place(pl_id);
                }
                let old_pl = get_place!(self, to.last_place)?;
                old_pl.taken_token_ids.remove(&to_id);
                modified.insert(to.last_place);
                modified.insert(pl_id);
                to.last_place = pl_id;
                let pl = get_place!(self, pl_id)?;
                pl.token_ids.insert(to_id); // we don't care if it already exists or not
            }
            NetChange::ExternalDelete(pl_id, to_id) => {
                let to = get_token!(self, to_id)?;
                assert_state!(
                    to.last_place == pl_id,
                    format!(
                        "Token '{}' last place was '{}', expected place '{}'.",
                        to_id.0, to.last_place.0, pl_id.0
                    )
                )?;
                let old_pl = get_place!(self, to.last_place)?;
                old_pl.taken_token_ids.remove(&to_id);
                match to.position.clone() {
                    TokenPosition::Place(cur_pl_id) => {
                        assert_state!(
                            cur_pl_id == pl_id,
                            format!(
                                "Token '{}' position is place '{}', expected place '{}'.",
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
                        let cur_tr = self.transitions.get_mut(&cur_tr_id);
                        if let Some(cur_tr) = cur_tr {
                            assert_state!(
                                cur_tr.token_ids.remove(&to_id),
                                format!(
                                    "Token '{}' not found at transition '{}'.",
                                    to_id.0, cur_tr_id.0
                                )
                            )?;
                        }
                    }
                }
                self.tokens.remove(&to_id);
                modified.insert(pl_id);
            }
            NetChange::ExternalUpdate(to_id, data) => {
                let to = get_token!(self, to_id)?;
                to.data = data;
                modified.insert(to.last_place);
            }
        }
        Ok(modified)
    }
}
