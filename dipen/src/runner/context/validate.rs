use std::any::Any;
use std::sync::Arc;

use crate::exec::{ValidateArcContext, ValidateContext, ValidatePlaceContext};
use crate::net::{self};

pub struct ValidateContextStruct<'a> {
    pub net: &'a net::PetriNetBuilder,
    pub transition_name: &'a str,
    pub arcs: Vec<&'a net::Arc>,
    pub registry_data: Option<Arc<dyn Any + Send + Sync>>,
}
struct ValidateArcContextStruct<'a> {
    arc: &'a net::Arc,
    place: &'a net::Place,
}
struct ValidatePlaceContextStruct<'a> {
    place: &'a net::Place,
}

impl<'a> ValidateContext for ValidateContextStruct<'a> {
    fn transition_name(&self) -> &str {
        self.transition_name
    }

    fn arcs(&self) -> impl Iterator<Item = impl crate::exec::ValidateArcContext> {
        self.arcs.iter().map(|arc| ValidateArcContextStruct {
            arc,
            place: self.net.places().get(arc.place()).unwrap(),
        })
    }

    fn registry_data(&self) -> Option<Arc<dyn std::any::Any + Send + Sync>> {
        self.registry_data.clone()
    }
}

impl<'a> ValidateArcContext for ValidateArcContextStruct<'a> {
    fn arc_name(&self) -> &str {
        self.arc.name()
    }

    fn variant(&self) -> net::ArcVariant {
        self.arc.variant()
    }

    fn place_context(&self) -> impl crate::exec::ValidatePlaceContext {
        ValidatePlaceContextStruct { place: self.place }
    }
}

impl ValidatePlaceContext for ValidatePlaceContextStruct<'_> {
    fn place_name(&self) -> &str {
        self.place.name()
    }
}
