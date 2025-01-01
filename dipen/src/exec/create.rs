use std::{any::Any, sync::Arc};

use crate::net::{ArcVariant, PlaceId, TransitionId};

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
