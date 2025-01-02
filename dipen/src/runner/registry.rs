use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use super::dispatch::{TransitionExecutorDispatch, TransitionExecutorDispatchStruct};
use crate::exec::TransitionExecutor;

#[derive(Default)]
pub struct ExecutorRegistry {
    pub(super) dispatcher: HashMap<String, Box<dyn TransitionExecutorDispatch>>,
}

impl ExecutorRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn register<T: TransitionExecutor + Send + Sync + 'static>(
        &mut self,
        transition_name: &str,
        data: Option<Arc<dyn Any + Send + Sync>>,
    ) {
        self.dispatcher.insert(
            transition_name.into(),
            Box::new(TransitionExecutorDispatchStruct::<T> { executor: None, data }),
        );
    }
}
