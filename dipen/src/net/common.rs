use std::collections::{HashMap, HashSet};

#[derive(Eq, PartialEq, Clone, Copy, PartialOrd, Ord, Hash, Debug)]
pub struct PlaceId(pub u64);
#[derive(Eq, PartialEq, Clone, Copy, PartialOrd, Ord, Hash, Debug)]
pub struct TransitionId(pub u64);
#[derive(Eq, PartialEq, Clone, Copy, PartialOrd, Ord, Hash, Debug)]
pub struct TokenId(pub u64);

#[derive(Clone)]
pub struct Place {
    name: String,
    pub(super) token_ids: HashSet<TokenId>,
    pub(super) taken_token_ids: HashMap<TokenId, TransitionId>,
    pub(super) output_locking: bool,
}

impl Place {
    pub fn new<S: AsRef<str>>(name: S, output_locking: bool) -> Self {
        Place {
            name: name.as_ref().to_string(),
            token_ids: Default::default(),
            taken_token_ids: Default::default(),
            output_locking,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn token_ids(&self) -> &HashSet<TokenId> {
        &self.token_ids
    }

    pub fn taken_token_ids(&self) -> &HashMap<TokenId, TransitionId> {
        &self.taken_token_ids
    }

    pub fn output_locking(&self) -> bool {
        self.output_locking
    }
}

#[derive(Clone)]
pub struct Transition {
    name: String,
    region: String,
    pub(super) token_ids: HashSet<TokenId>,
}

impl Transition {
    pub fn new<S1: AsRef<str>, S2: AsRef<str>>(name: S1, region: S2) -> Self {
        Transition {
            name: name.as_ref().to_string(),
            region: region.as_ref().to_string(),
            token_ids: Default::default(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn region(&self) -> &str {
        &self.region
    }

    pub fn token_ids(&self) -> &HashSet<TokenId> {
        &self.token_ids
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum ArcVariant {
    In,
    Out,
    InOut,
    Cond,
    OutCond,
}

impl ArcVariant {
    /// True for all incoming arcs (In | InOut)
    pub fn is_in(&self) -> bool {
        [Self::In, Self::InOut].contains(self)
    }

    /// True for all outgoing arcs (Out | InOut | OutCond)
    pub fn is_out(&self) -> bool {
        [Self::Out, Self::InOut, Self::OutCond].contains(self)
    }

    /// True for all arcs usable as condition (In | InOut | Cond | OutCond)
    pub fn is_cond(&self) -> bool {
        [Self::In, Self::InOut, Self::Cond, Self::OutCond].contains(self)
    }
}

#[derive(Clone)]
pub struct Arc {
    place: String,
    transition: String,
    variant: ArcVariant,
    name: String,
}

impl Arc {
    pub fn new<S1: AsRef<str>, S2: AsRef<str>>(
        place: S1,
        transition: S2,
        variant: ArcVariant,
        name: String,
    ) -> Self {
        Arc {
            place: place.as_ref().to_string(),
            transition: transition.as_ref().to_string(),
            variant,
            name,
        }
    }

    pub fn place(&self) -> &str {
        &self.place
    }

    pub fn transition(&self) -> &str {
        &self.transition
    }
    pub fn variant(&self) -> ArcVariant {
        self.variant
    }
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenPosition {
    Place(PlaceId),
    Transition(TransitionId),
}

pub struct Token {
    pub(super) data: Vec<u8>,
    pub(super) position: TokenPosition,
    pub(super) last_place: PlaceId,
}

impl Token {
    pub fn new(last_place: PlaceId, position: TokenPosition) -> Self {
        Token { data: Default::default(), position, last_place }
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn position(&self) -> TokenPosition {
        self.position.clone()
    }

    pub fn last_place(&self) -> PlaceId {
        self.last_place
    }
}