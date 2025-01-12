use std::any::Any;
use std::pin::Pin;
use std::sync::Arc;

use super::context::*;
use crate::exec::{CheckStartResult, CreationError, RunResult, TransitionExecutor};

pub trait TransitionExecutorDispatch: Send + Sync {
    fn clone_empty(&self) -> Box<dyn TransitionExecutorDispatch>;
    fn create(&mut self, ctx: create::CreateContextStruct) -> Result<(), CreationError>;
    fn check_start(&mut self, ctx: &mut start::StartContextStruct) -> CheckStartResult;
    fn run<'a, 'b>(
        &'a mut self,
        ctx: &'b mut run::RunContextStruct,
    ) -> Pin<Box<dyn std::future::Future<Output = RunResult> + Send + 'b>>
    where
        'a: 'b;
}
pub(crate) struct TransitionExecutorDispatchStruct<T: TransitionExecutor> {
    pub(crate) executor: Option<T>,
    pub(crate) data: Option<Arc<dyn Any + Send + Sync>>,
}

impl<T: TransitionExecutor + Send + Sync + 'static> TransitionExecutorDispatch
    for TransitionExecutorDispatchStruct<T>
{
    fn clone_empty(&self) -> Box<dyn TransitionExecutorDispatch> {
        Box::new(Self { executor: None, data: self.data.clone() })
    }

    fn create(&mut self, mut ctx: create::CreateContextStruct) -> Result<(), CreationError> {
        ctx.registry_data = self.data.clone();
        self.executor = Some(T::new(&ctx)?);
        Ok(())
    }

    fn check_start(&mut self, ctx: &mut start::StartContextStruct) -> CheckStartResult {
        self.executor.as_mut().unwrap().check_start(ctx)
    }

    fn run<'a, 'b>(
        &'a mut self,
        ctx: &'b mut run::RunContextStruct,
    ) -> Pin<Box<dyn std::future::Future<Output = RunResult> + Send + 'b>>
    where
        'a: 'b,
    {
        Box::pin(self.executor.as_mut().unwrap().run(ctx))
    }
}
