use std::{
    any::Any,
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use thiserror::Error;

use crate::net::{ArcVariant, PlaceId, TokenId, TransitionId};

#[derive(Error, Debug)]
#[error("{msg}")]
pub struct StateError {
    msg: String,
}

pub trait ValidatePlaceContext {
    fn place_name(&self) -> &str;
}
pub trait ValidateArcContext {
    fn arc_name(&self) -> &str;
    fn variant(&self) -> ArcVariant;
    fn place_context(&self) -> impl ValidatePlaceContext;
}

pub struct ValidationResult {
    pub(crate) success: bool,
    pub(crate) reason: String,
}

impl ValidationResult {
    pub fn success() -> ValidationResult {
        ValidationResult { success: true, reason: Default::default() }
    }
    pub fn failure(reason: &str) -> ValidationResult {
        ValidationResult { success: false, reason: reason.into() }
    }
}
pub trait ValidateContext {
    fn transition_name(&self) -> &str;
    fn arcs(&self) -> impl Iterator<Item = impl ValidateArcContext>;
    fn arcs_in(&self) -> impl Iterator<Item = impl ValidateArcContext> {
        self.arcs().filter(|a| a.variant().is_in())
    }
    fn arcs_out(&self) -> impl Iterator<Item = impl ValidateArcContext> {
        self.arcs().filter(|a| a.variant().is_out())
    }
    fn arcs_cond(&self) -> impl Iterator<Item = impl ValidateArcContext> {
        self.arcs().filter(|a| a.variant().is_cond())
    }
    fn arcs_by_name(&self, name: &str) -> impl Iterator<Item = impl ValidateArcContext> {
        self.arcs().filter(move |a| a.arc_name() == name)
    }
    fn arcs_by_place_name(&self, name: &str) -> impl Iterator<Item = impl ValidateArcContext> {
        self.arcs().filter(move |a| a.place_context().place_name() == name)
    }
}

pub trait CreatePlaceContext {
    fn place_name(&self) -> &str;
    fn place_id(&self) -> PlaceId;
}
pub trait CreateArcContext {
    fn arc_name(&self) -> &str;
    fn variant(&self) -> ArcVariant;
    fn place_context(&self) -> impl CreatePlaceContext;
}

pub trait CreateContext {
    fn transition_name(&self) -> &str;
    fn transition_id(&self) -> TransitionId;
    fn arcs(&self) -> impl Iterator<Item = impl CreateArcContext>;

    fn arcs_in(&self) -> impl Iterator<Item = impl CreateArcContext> {
        self.arcs().filter(|a| a.variant().is_in())
    }
    fn arcs_out(&self) -> impl Iterator<Item = impl CreateArcContext> {
        self.arcs().filter(|a| a.variant().is_out())
    }
    fn arcs_cond(&self) -> impl Iterator<Item = impl CreateArcContext> {
        self.arcs().filter(|a| a.variant().is_cond())
    }
    fn arcs_by_name(&self, name: &str) -> impl Iterator<Item = impl CreateArcContext> {
        self.arcs().filter(move |a| a.arc_name() == name)
    }
    fn arcs_by_place_name(&self, name: &str) -> impl Iterator<Item = impl CreateArcContext> {
        self.arcs().filter(move |a| a.place_context().place_name() == name)
    }
    fn registry_data(&self) -> Option<Arc<dyn Any + Send + Sync>>;
}

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
    fn tokens_at(&self, place_id: PlaceId) -> impl Iterator<Item = impl StartTokenContext>;
    fn taken_tokens_at(
        &self,
        place_id: PlaceId,
    ) -> impl Iterator<Item = impl StartTakenTokenContext>;
}

pub trait RunTokenContext: Send + Sync {
    fn token_id(&self) -> TokenId;

    fn data(&self) -> &[u8];

    fn orig_place_id(&self) -> PlaceId;
}
pub trait RunContext: Send + Sync {
    fn tokens(&self) -> impl Iterator<Item = &impl RunTokenContext> + Send + Sync;
}

#[derive(Default)]
pub(crate) struct EnabledData {
    pub(crate) take: Vec<(PlaceId, TokenId)>,
}

#[derive(Default)]
pub(crate) struct DisabledData {
    pub(crate) wait_for: Option<PlaceId>,
    pub(crate) auto_recheck: Duration,
}
pub struct CheckStartResult {
    pub(crate) choice: CheckStartChoice,
}
pub(crate) enum CheckStartChoice {
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

pub struct RunResult {
    pub(crate) place: Vec<(TokenId, PlaceId, PlaceId, Vec<u8>)>,
    pub(crate) create: Vec<(PlaceId, Vec<u8>)>,
}

impl RunResult {
    pub fn build() -> RunResultBuilder {
        RunResultBuilder {
            place: Default::default(),
            update: Default::default(),
            create: Default::default(),
        }
    }
}

pub struct RunResultBuilder {
    place: HashMap<TokenId, (Vec<u8>, PlaceId, PlaceId)>, // orig data, orig place, target place
    update: HashMap<TokenId, (Vec<u8>, PlaceId)>,         // updated data, orig place
    create: Vec<(PlaceId, Vec<u8>)>,                      // new data, new place
}

impl RunResultBuilder {
    pub fn place(&mut self, to: &impl RunTokenContext, place_id: PlaceId) {
        // TODO check that place id is an output arc of the current transition
        self.place.insert(to.token_id(), (to.data().into(), to.orig_place_id(), place_id));
    }

    pub fn update(&mut self, to: &impl RunTokenContext, data: Vec<u8>) {
        self.update.insert(to.token_id(), (data, to.orig_place_id()));
    }

    pub fn place_new(&mut self, place_id: PlaceId, data: Vec<u8>) {
        self.create.push((place_id, data));
    }

    pub fn result(mut self) -> RunResult {
        let mut place_vec = Vec::<(TokenId, PlaceId, PlaceId, Vec<u8>)>::new();
        for (to_id, (orig_data, orig_pl_id, pl_id)) in self.place.drain() {
            match self.update.remove(&to_id) {
                Some((data, _)) => {
                    place_vec.push((to_id, orig_pl_id, pl_id, data));
                }
                None => {
                    place_vec.push((to_id, orig_pl_id, pl_id, orig_data));
                }
            }
        }
        for (to_id, (data, pl_id)) in self.update.drain() {
            place_vec.push((to_id, pl_id, pl_id, data));
        }
        RunResult { place: place_vec, create: self.create }
    }
}

pub trait TransitionExecutor {
    fn validate(ctx: &impl ValidateContext) -> ValidationResult;
    fn new(ctx: &impl CreateContext) -> Self;
    fn check_start(&mut self, ctx: &mut impl StartContext) -> CheckStartResult;
    fn run(
        &mut self,
        ctx: &mut impl RunContext,
    ) -> impl std::future::Future<Output = RunResult> + Send + Sync;
}
