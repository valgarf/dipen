use std::collections::HashMap;

use crate::net::{PlaceId, TokenId};

pub trait RunTokenContext: Send + Sync {
    fn token_id(&self) -> TokenId;

    fn data(&self) -> &[u8];

    fn orig_place_id(&self) -> PlaceId;
}
pub trait RunContext: Send + Sync {
    fn tokens(&self) -> impl Iterator<Item = &impl RunTokenContext> + Send + Sync;
}
#[derive(Clone)]
pub struct RunResult {
    place: Vec<(TokenId, PlaceId, PlaceId, Vec<u8>)>,
    create: Vec<(PlaceId, Vec<u8>)>,
}

impl RunResult {
    pub fn build() -> RunResultBuilder {
        RunResultBuilder {
            place: Default::default(),
            update: Default::default(),
            create: Default::default(),
        }
    }

    pub fn place(&self) -> &[(TokenId, PlaceId, PlaceId, Vec<u8>)] {
        &self.place
    }

    pub fn create(&self) -> &[(PlaceId, Vec<u8>)] {
        &self.create
    }
}

/// Allows to access the data of the run result directly by consuming the run result
/// Although this is public, its implementation is subject to change.
pub struct RunResultData {
    pub place: Vec<(TokenId, PlaceId, PlaceId, Vec<u8>)>,
    pub create: Vec<(PlaceId, Vec<u8>)>,
}

impl From<RunResult> for RunResultData {
    fn from(value: RunResult) -> Self {
        RunResultData { place: value.place, create: value.create }
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
