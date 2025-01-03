use std::{collections::HashSet, time::Duration};

use crate::net::{PlaceId, TokenId, TransitionId};

pub trait StartTokenContext {
    fn token_id(&self) -> TokenId;

    fn place_id(&self) -> PlaceId;

    fn data(&self) -> &[u8];
}

pub trait StartTakenTokenContext {
    fn token_id(&self) -> TokenId;

    fn transition_id(&self) -> TransitionId;

    fn place_id(&self) -> PlaceId;

    fn data(&self) -> &[u8];
}

pub trait StartContext {
    fn tokens(&self) -> impl Iterator<Item = impl StartTokenContext>;
    fn taken_tokens(&self) -> impl Iterator<Item = impl StartTakenTokenContext>;
    fn tokens_at(&self, place_id: PlaceId) -> impl Iterator<Item = impl StartTokenContext>;
    fn taken_tokens_at(
        &self,
        place_id: PlaceId,
    ) -> impl Iterator<Item = impl StartTakenTokenContext>;
}

#[derive(Default, Clone)]
pub struct EnabledData {
    pub take: Vec<(PlaceId, TokenId)>,
}

#[derive(Default, Clone)]
pub struct DisabledData {
    pub wait_for: Option<PlaceId>,
    pub auto_recheck: Duration,
}

#[derive(Clone)]
pub struct CheckStartResult {
    choice: CheckStartChoice,
    // Note: fields not public to enforce the use of the builder
}

#[derive(Clone)]
pub enum CheckStartChoice {
    Enabled(EnabledData),
    Disabled(DisabledData),
}

#[derive(Default)]
pub struct CheckStartResultBuilder {
    taken: HashSet<(PlaceId, TokenId)>,
}

impl CheckStartResult {
    pub fn build() -> CheckStartResultBuilder {
        CheckStartResultBuilder::default()
    }

    pub fn to_choice(self) -> CheckStartChoice {
        self.choice
    }
}

impl CheckStartResultBuilder {
    pub fn take(&mut self, token: &impl StartTokenContext) {
        self.taken.insert((token.place_id(), token.token_id()));
    }

    pub fn enabled(self) -> CheckStartResult {
        CheckStartResult {
            choice: CheckStartChoice::Enabled(EnabledData {
                take: self.taken.iter().copied().collect(),
            }),
        }
    }

    pub fn disabled(
        self,
        wait_for: Option<PlaceId>,
        auto_recheck: Option<Duration>,
    ) -> CheckStartResult {
        CheckStartResult {
            choice: CheckStartChoice::Disabled(DisabledData {
                wait_for,
                auto_recheck: if let Some(d) = auto_recheck { d } else { Duration::new(0, 0) },
            }),
        }
    }
}
