use std::{any::Any, sync::Arc};

use crate::net::ArcVariant;

pub trait ValidatePlaceContext {
    fn place_name(&self) -> &str;
}
pub trait ValidateArcContext {
    fn arc_name(&self) -> &str;
    fn variant(&self) -> ArcVariant;
    fn place_context(&self) -> impl ValidatePlaceContext;
}

#[derive(Clone)]
pub struct ValidationResult {
    success: bool,
    reason: String,
}

impl ValidationResult {
    pub fn succeeded() -> ValidationResult {
        ValidationResult { success: true, reason: Default::default() }
    }
    pub fn failed(reason: impl Into<String>) -> ValidationResult {
        ValidationResult { success: false, reason: reason.into() }
    }

    pub fn success(&self) -> bool {
        self.success
    }

    /// Failure reason. None if and onlfy if success() == true
    pub fn reason(&self) -> Option<&str> {
        if self.success {
            None
        } else {
            Some(&self.reason)
        }
    }
}
pub trait ValidateContext {
    fn transition_name(&self) -> &str;
    fn arcs(&self) -> impl Iterator<Item = impl ValidateArcContext>
    where
        Self: Sized;
    fn arcs_in(&self) -> impl Iterator<Item = impl ValidateArcContext>
    where
        Self: Sized,
    {
        self.arcs().filter(|a| a.variant().is_in())
    }
    fn arcs_out(&self) -> impl Iterator<Item = impl ValidateArcContext>
    where
        Self: Sized,
    {
        self.arcs().filter(|a| a.variant().is_out())
    }
    fn arcs_cond(&self) -> impl Iterator<Item = impl ValidateArcContext>
    where
        Self: Sized,
    {
        self.arcs().filter(|a| a.variant().is_cond())
    }
    fn arcs_by_name(&self, name: &str) -> impl Iterator<Item = impl ValidateArcContext>
    where
        Self: Sized,
    {
        self.arcs().filter(move |a| a.arc_name() == name)
    }
    fn arcs_by_place_name(&self, name: &str) -> impl Iterator<Item = impl ValidateArcContext>
    where
        Self: Sized,
    {
        self.arcs().filter(move |a| a.place_context().place_name() == name)
    }
    fn registry_data(&self) -> Option<Arc<dyn Any + Send + Sync>>;
}
