use std::any::Any;
use std::sync::Arc;

use crate::exec::{CreateArcContext, CreateContext, CreatePlaceContext};
use crate::net::{self, PlaceId, TransitionId};

pub struct CreateContextStruct<'a> {
    pub net: &'a net::PetriNet,
    pub transition_name: &'a str,
    pub transition_id: TransitionId,
    pub arcs: Vec<(PlaceId, &'a net::Arc)>,
    pub registry_data: Option<Arc<dyn Any + Send + Sync>>,
}
struct CreateArcContextStruct<'a> {
    arc: &'a net::Arc,
    place_id: PlaceId,
    place: &'a net::Place,
}
struct CreatePlaceContextStruct<'a> {
    place_id: PlaceId,
    place: &'a net::Place,
}

impl<'a> CreateContext for CreateContextStruct<'a> {
    fn transition_name(&self) -> &str {
        self.transition_name
    }

    fn transition_id(&self) -> TransitionId {
        self.transition_id
    }

    fn arcs(&self) -> impl Iterator<Item = impl crate::exec::CreateArcContext> {
        self.arcs.iter().map(|&(pl_id, arc)| CreateArcContextStruct {
            arc,
            place: self.net.place(pl_id).unwrap(),
            place_id: pl_id,
        })
    }

    fn registry_data(&self) -> Option<Arc<dyn Any + Send + Sync>> {
        self.registry_data.clone()
    }
}

impl<'a> CreateArcContext for CreateArcContextStruct<'a> {
    fn arc_name(&self) -> &str {
        self.arc.name()
    }

    fn variant(&self) -> net::ArcVariant {
        self.arc.variant()
    }

    fn place_context(&self) -> impl crate::exec::CreatePlaceContext {
        CreatePlaceContextStruct { place: self.place, place_id: self.place_id }
    }
}

impl CreatePlaceContext for CreatePlaceContextStruct<'_> {
    fn place_name(&self) -> &str {
        self.place.name()
    }

    fn place_id(&self) -> net::PlaceId {
        self.place_id
    }
}
