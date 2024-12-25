use futures::future::select_all;
use futures::FutureExt;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::unix::pipe::Receiver;
use tokio::select;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{error, trace};

use crate::net::{self, PetriNet, PlaceId, TokenId, TransitionId};
use crate::transition::{
    CheckStartChoice, CheckStartResult, CreateArcContext, CreateContext, CreatePlaceContext,
    RunContext, RunResult, RunTokenContext, StartContext, StartTokenContext, TransitionExecutor,
    ValidateArcContext, ValidateContext, ValidatePlaceContext, ValidationResult,
};
use crate::ETCDTransitionGate;

pub struct TransitionRunner {
    pub(crate) cancel_token: CancellationToken,
    pub(crate) transition_id: net::TransitionId,
    pub(crate) etcd_gate: ETCDTransitionGate,
    pub(crate) net_lock: Arc<RwLock<net::PetriNet>>,
    pub(crate) place_rx: HashMap<net::PlaceId, tokio::sync::watch::Receiver<u64>>,
    pub(crate) exec: Box<dyn TransitionExecutorDispatch>,
    pub(crate) rx_revision: tokio::sync::watch::Receiver<u64>,
}

// # context implementations
// ## validate context
pub(crate) struct ValidateContextStruct<'a> {
    pub(crate) net: &'a net::PetriNetBuilder,
    pub(crate) transition_name: &'a str,
    pub(crate) arcs: Vec<&'a net::Arc>,
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

    fn arcs(&self) -> impl Iterator<Item = impl crate::transition::ValidateArcContext> {
        self.arcs.iter().map(|arc| ValidateArcContextStruct {
            arc,
            place: self.net.places().get(arc.place()).unwrap(),
        })
    }
}

impl<'a> ValidateArcContext for ValidateArcContextStruct<'a> {
    fn arc_name(&self) -> &str {
        self.arc.name()
    }

    fn variant(&self) -> net::ArcVariant {
        self.arc.variant()
    }

    fn place_context(&self) -> impl crate::transition::ValidatePlaceContext {
        ValidatePlaceContextStruct { place: self.place }
    }
}

impl ValidatePlaceContext for ValidatePlaceContextStruct<'_> {
    fn place_name(&self) -> &str {
        self.place.name()
    }
}

// ## create context
pub(crate) struct CreateContextStruct<'a> {
    pub(crate) net: &'a net::PetriNet,
    pub(crate) transition_name: &'a str,
    pub(crate) transition_id: TransitionId,
    pub(crate) arcs: Vec<(PlaceId, &'a net::Arc)>,
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

    fn arcs(&self) -> impl Iterator<Item = impl crate::transition::CreateArcContext> {
        self.arcs.iter().map(|&(pl_id, arc)| CreateArcContextStruct {
            arc,
            place: self.net.places().get(&pl_id).unwrap(),
            place_id: pl_id,
        })
    }
}

impl<'a> CreateArcContext for CreateArcContextStruct<'a> {
    fn arc_name(&self) -> &str {
        self.arc.name()
    }

    fn variant(&self) -> net::ArcVariant {
        self.arc.variant()
    }

    fn place_context(&self) -> impl crate::transition::CreatePlaceContext {
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

// ## start context

pub(crate) struct StartContextStruct<'a> {
    net: &'a PetriNet,
}

pub(crate) struct StartTokenContextStruct<'a> {
    net: &'a PetriNet,
    token_id: TokenId,
    place_id: PlaceId,
}

impl<'a> StartContextStruct<'a> {
    fn new(net: &'a PetriNet) -> Self {
        Self { net }
    }
}
impl<'a> StartContext for StartContextStruct<'a> {
    fn tokens_at(
        &self,
        place_id: PlaceId,
    ) -> impl Iterator<Item = impl crate::transition::StartTokenContext> {
        let net = self.net;
        net.places()
            .get(&place_id)
            .unwrap()
            .token_ids()
            .iter()
            .map(move |&token_id| StartTokenContextStruct { net, token_id, place_id })
    }
}

impl StartTokenContext for StartTokenContextStruct<'_> {
    fn token_id(&self) -> TokenId {
        self.token_id
    }

    fn data(&self) -> &[u8] {
        self.net.tokens().get(&self.token_id).unwrap().data()
    }

    fn place_id(&self) -> PlaceId {
        self.place_id
    }
}

// ## run context

pub(crate) struct RunContextStruct {
    tokens: Vec<RunTokenContextStruct>,
}

pub(crate) struct RunTokenContextStruct {
    token_id: TokenId,
    orig_place_id: PlaceId,
    data: Vec<u8>,
}

impl RunContext for RunContextStruct {
    fn tokens(&self) -> impl Iterator<Item = &impl crate::transition::RunTokenContext> {
        self.tokens.iter()
    }
}

impl RunTokenContext for RunTokenContextStruct {
    fn token_id(&self) -> TokenId {
        self.token_id
    }

    fn data(&self) -> &[u8] {
        &self.data
    }

    fn orig_place_id(&self) -> PlaceId {
        self.orig_place_id
    }
}

// ## dispatch
pub trait TransitionExecutorDispatch: Send {
    fn clone_empty(&self) -> Box<dyn TransitionExecutorDispatch>;
    fn validate(&self, ctx: &ValidateContextStruct) -> ValidationResult;
    fn create(&mut self, ctx: &CreateContextStruct);
    fn check_start(&mut self, ctx: &mut StartContextStruct) -> CheckStartResult;
    fn run<'a, 'b>(
        &'a mut self,
        ctx: &'b mut RunContextStruct,
    ) -> Pin<Box<dyn std::future::Future<Output = RunResult> + Send + 'b>>
    where
        'a: 'b;
}
pub(crate) struct TransitionExecutorDispatchStruct<T: TransitionExecutor> {
    pub(crate) executor: Option<T>,
}

impl<T: TransitionExecutor + Send + 'static> TransitionExecutorDispatch
    for TransitionExecutorDispatchStruct<T>
{
    fn clone_empty(&self) -> Box<dyn TransitionExecutorDispatch> {
        Box::new(Self { executor: None })
    }
    fn validate(&self, ctx: &ValidateContextStruct) -> ValidationResult {
        T::validate(ctx)
    }

    fn create(&mut self, ctx: &CreateContextStruct) {
        self.executor = Some(T::new(ctx));
    }

    fn check_start(&mut self, ctx: &mut StartContextStruct) -> CheckStartResult {
        self.executor.as_mut().unwrap().check_start(ctx)
    }

    fn run<'a, 'b>(
        &'a mut self,
        ctx: &'b mut RunContextStruct,
    ) -> Pin<Box<dyn std::future::Future<Output = RunResult> + Send + 'b>>
    where
        'a: 'b,
    {
        Box::pin(self.executor.as_mut().unwrap().run(ctx))
    }
}

// runner implementation

impl TransitionRunner {
    #[tracing::instrument(level = "debug", skip(self), fields(transition_id=self.transition_id.0))]
    pub async fn run_transition(mut self) {
        let net = self.net_lock.read().await;
        let ctx = CreateContextStruct {
            net: &net,
            transition_name: net.transitions().get(&self.transition_id).unwrap().name(),
            transition_id: self.transition_id,
            // TODO: optimize by storing all arcs corresnponding to one transition
            arcs: net
                .arcs()
                .iter()
                .filter(|(&(_, tr_id), _)| tr_id == self.transition_id)
                .map(|(&(pl_id, _), arc)| (pl_id, arc))
                .collect(),
        };
        self.exec.create(&ctx);
        drop(ctx);
        drop(net);

        let mut wait_for: Option<net::PlaceId> = None;
        loop {
            let wait_for_change = async {
                match wait_for {
                    None => {
                        let futures: Vec<_> =
                            self.place_rx.values_mut().map(|rec| rec.changed().boxed()).collect();
                        let _ = select_all(futures).await;
                    }
                    Some(pl_id) => {
                        let _ = self.place_rx.get_mut(&pl_id).unwrap().changed().await;
                    }
                }
            };

            select! {
                _ = wait_for_change => {},
                _ = self.cancel_token.cancelled()  => {return;}
            }
            for rec in self.place_rx.values_mut() {
                rec.mark_unchanged();
            }

            let net = self.net_lock.read().await;
            let mut ctx = StartContextStruct::new(&net);
            let start_res = self.exec.check_start(&mut ctx);
            let take_tokens = match start_res.choice {
                CheckStartChoice::Disabled(data) => {
                    wait_for = data.wait_for;
                    // TODO: handle auto recheck
                    continue;
                }
                CheckStartChoice::Enabled(data) => data.take,
            };
            // TODO: locking and check again afterwards
            trace!("Running transition.");

            let _ = self.etcd_gate.start_transition(take_tokens.clone()).await;

            let mut ctx = RunContextStruct {
                tokens: take_tokens
                    .into_iter()
                    .map(|(to_id, orig_pl_id)| RunTokenContextStruct {
                        token_id: to_id,
                        orig_place_id: orig_pl_id,
                        data: net.tokens().get(&to_id).unwrap().data().into(),
                    })
                    .collect(),
            };

            // TODO: release locks
            drop(net);

            let op = self.exec.run(&mut ctx);

            let res = select! {
                res = op => {res},
                _ = self.cancel_token.cancelled()  => {return;}
                // NOTE: if we want external cancellation, wait for it here
            };

            // TODO: acquire locks on output places
            let revision = match self.etcd_gate.end_transition(res.place).await {
                Ok(revision) => revision,
                Err(err) => {
                    error!("End transition failed with error: {}", err);
                    return;
                }
            };
            // TODO: release locks

            select! {
                _ = self.rx_revision.wait_for(move |&rev| {rev>=revision}) => {},
                _ = self.cancel_token.cancelled()  => {return;}
            }

            trace!("Finished transition.");
        }
    }
}
