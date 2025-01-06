//! # DiPeN - Distributed Petri Net runner
//!
//! This crate implements a distributed runner to use petri nets as a workflow engine, that is,
//! driving real workflows, not just simulating them.
//!
//! The petri net differs from the classical petri net in multiple areas:
//! - Tokens are colored: Tokens can have arbitrary data attached to them.
//! - Transitions are timed: You are expected to write asynchronous code to be executed when a
//!   transition is firing.
//! - Transition have arbitrary guard expressions: You can write any logic you want to decide
//!   whether a transition can be fired.
//!
//! If the last paragraph sounded like gibberish to you, read the [Petri Net](#petri-net) section for
//! an introduction on DiPeN's version of petri nets.
//! Otherwise, you can skip this and continue with [Getting Started](#getting-started).
//!
//! ## Petri Net
//!
//! A petri net is a graph that has two node types: places and transitions.
//! A transition and a place can be connected with an arc, there are no connections between two
//! places or between two transitions.
//!
//! A simple example might look like this,
//! <p>
#![doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/diagrams/minimal-graph.svg"))]
//! </p>
//! where places are represented as circles, the transition is represented as a rectangle and arcs
//! are represented as arrows. One arc is going into the transition, the place on the left is also
//! referred to as 'input place' for that transition. Accordingly, the place on the right is an
//! 'output place', as the arc is outgoing.
//!
//! The current state is represented by tokens.
//! In DiPeN, each token is different and you can attach arbitrary data to it.
//! This is also known as a 'colored' Petri net.
//!
//! Tokens are represented as small dots, e.g. the starting state for
//! our example net could look like this,
//! <p>
#![doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/diagrams/minimal-graph-start.svg"))]
//! </p>
//! with two tokens on the left place.
//!
//! Petri nets progress by firing transitions. The transition in the middle might have some logic to
//! decide that it can fire, if there is at least one token on an input place. This logic is
//! something you would implement for every transition when using this library. This is also called
//! a 'guard expression'.
//!
//! When the transition starts firing, it can decide to take tokens from its input place, lets
//! assume it takes one token, then we end up with the following state,
//! <p>
#![doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/diagrams/minimal-graph-firing.svg"))]
//! </p>
//! where one token is placed on the transition. Note that we also show an empty circle on the input
//! place. In DiPeN it is possible to still see tokens that are currently being used in a transition
//! from their original places.
//!
//! The firing of a transition is represented as an async function in the code and is it up to you
//! how to implement that.
//! Transitions may take some time to complete, this is known as a 'timed transition'.
//! At the end of the firing process, a transition can place the token it has taken from the input
//! place and put it into anoutput place.
//! It could also destroy the token or create new ones, but let us assume the token is moved, we
//! end up with,
//! <p>
#![doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/diagrams/minimal-graph-finished1.svg"))]
//! </p>
//! where the token is on the output place and not accessible from the input place or the transition
//! anymore.
//!
//! The transition may fire a second time and move the second token, resulting in a final state of
//! <p>
#![doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/diagrams/minimal-graph-finished2.svg"))]
//! </p>
//!
//! In DiPeN we have 5 different arc types you can use:
//! <p>
#![doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/diagrams/possible-arcs.svg"))]
//! </p>
//!
//! - In: An incoming arc, the transition can use the attached place and its tokens to decide
//!       whether it should fire. When starting to fire, it can take tokens from this place
//! - Out: An outgoing arc, the transition can place tokens here when it finishes firing. Those
//!        token can be from input places or they can be newly created ones.
//! - Cond: The transition can decide whether to fire based on the attached place and its tokens,
//!         but no tokens are moved from or to this place.
//! - InOut: A combination of the incoming and outgoing arc. Tokens can be taken and placed here.
//! - OutCond: A combination of an outgoing arc with a condition. The state of the attached place
//!            and its tokens can be used to decide whether to fire and tokens can be placed here,
//!            but no tokens can be taken.
//!
//! ## Getting Started
//!
//! The minimal example below implements the following net:
#![doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/diagrams/getting-started.svg"))]
//!
//! The transition fires only if the output place is empty.
//! If it fires, it places a token on the output place.
//!
//! ```
//! # use std::{sync::Arc, time::Duration};
//! #
//! # use dipen::{
//! #     error::Result as PetriResult,
//! #     etcd::{ETCDConfigBuilder, ETCDGate},
//! #     exec::{
//! #         CheckStartResult, CreateArcContext, CreatePlaceContext, RunResult, TransitionExecutor,
//! #         ValidationResult,
//! #     },
//! #     net,
//! #     runner::ExecutorRegistry,
//! # };
//! # use tokio_util::sync::CancellationToken;
//! # use tracing_subscriber::EnvFilter;
//!
//! #[tokio::main]
//! async fn main() -> PetriResult<()> {
//! #    // initialize logging
//! #    tracing_subscriber::fmt()
//! #        .with_span_events(
//! #            tracing_subscriber::fmt::format::FmtSpan::CLOSE
//! #                | tracing_subscriber::fmt::format::FmtSpan::NEW,
//! #        )
//! #        .compact()
//! #        .with_env_filter(EnvFilter::try_new("info,dipen=debug").unwrap())
//! #        .init();
//!     // shutdown token is passed to the runner. You need to cancel it to
//!     // stop the runner
//!     let shutdown_token = CancellationToken::new();
//! #    // test logic: stop after 5 seconds
//! #    let shutdown_token_clone = shutdown_token.clone();
//! #    tokio::spawn(async move {
//! #        tokio::time::sleep(Duration::from_millis(100)).await;
//! #        shutdown_token_clone.cancel();
//! #    });
//!
//!     // set up petri net
//!     let mut net = net::PetriNetBuilder::default();
//!     net.insert_place(net::Place::new("pl", true));
//!     net.insert_transition(net::Transition::new("tr", "test-region"));
//!     net.insert_arc(net::Arc::new("pl", "tr", net::ArcVariant::OutCond, ""))?;
//!     let net = Arc::new(net);
//!
//!     // set up transition executors for each transition
//!     let mut executors = ExecutorRegistry::new();
//!     // PlaceNew is the transition defined below
//!     // Note: you could reuse the same implementation
//!     // for multiple different transitions in the net.
//!     executors.register::<PlaceNew>("tr", None);
//!                                                 
//!
//!     // configure etcd connection
//!     let config = ETCDConfigBuilder::default()
//!         .endpoints(["localhost:2379"])
//!         .prefix("doctest-01/") // all etcd keys are prefixed with that.
//!         .node_name("node1") // to identify the node in logs / on the etcd server
//!         .region("test-region") // must match your transition regions
//!         .build()?;
//!     let etcd = ETCDGate::new(config);
//!
//!     dipen::runner::run(net, etcd, executors, shutdown_token.clone()).await
//! }
//!
//! // Implement the PlaceNew transition
//! pub struct PlaceNew {
//!     pl_out: net::PlaceId, // the place to create the token
//! }
//!
//! impl TransitionExecutor for PlaceNew {
//!     // Called in the beginning for every transition in the net that is
//!     // associated with this executor to check for misconfigurations.
//!     fn validate(ctx: &impl dipen::exec::ValidateContext) -> ValidationResult
//!     where
//!         Self: Sized,
//!     {
//!         // check if the transition can work with the configured arcs.
//!         // If it cannot, return a reason.
//!         if ctx.arcs().count() == 1
//!            && ctx.arcs_out().count() == 1
//!            && ctx.arcs_cond().count() == 1
//!         {
//!             ValidationResult::succeeded()
//!         } else {
//!             ValidationResult::failed("Need exactly one conditional outgoing arc!")
//!         }
//!     }
//!
//!     // After validation, an instance is created for every transition in the net.
//!     // This function should not fail, check all preconditions in `validate`.
//!     fn new(ctx: &impl dipen::exec::CreateContext) -> Self
//!     where
//!         Self: Sized,
//!     {
//!         let pl_out = ctx.arcs_out().next().unwrap().place_context().place_id();
//!         Self { pl_out }
//!     }
//!
//!     // Called to check if the transition can fire.
//!     // If it can fire, the result should contain all the tokens this transition
//!     // wants to take.
//!     // Note: this is called multiple times, even if `result.enabled()` is called.
//!     // Currently at least two times: First to check if it can fire with the
//!     // current local state of the runner, then again after acquiring locks on all
//!     // the input places.
//!     fn check_start(
//!         &mut self,
//!         ctx: &mut impl dipen::exec::StartContext
//!     ) -> CheckStartResult {
//!         let result = CheckStartResult::build();
//!         if ctx.tokens_at(self.pl_out).count() > 0 {
//!             // already a token placed, do not run
//!             return result.disabled(None, None);
//!         }
//!         // no token found, run this transition
//!         result.enabled()
//!
//!         // Note: You can store data on your transition, but only use it for the
//!         // next run.
//!     }
//!
//!     // Called when the transition actually started firing. At this point, tokens
//!     // are booked to the transition and can be accessed with the context.
//!     // The result should list all moved / newly created tokens.
//!     // Tokens that were taken but not placed in this function, are automatically
//!     // deleted after the `run` method finishes.
//!     async fn run(&mut self, _ctx: &mut impl dipen::exec::RunContext) -> RunResult {
//!         let mut result = RunResult::build();
//!         // place a single newly created token on the output place
//!         result.place_new(self.pl_out, "newly created".into());
//!         result.result()
//!     }
//! }
//! ```
//!
//! ## Implementation
//!
//! The current implementation does roughly the following:
//! - build a reduced petri net, only containing the region we want to run
//! - call [`exec::TransitionExecutor::validate`] on all transitions, stopping with an error in case
//!   of a failure
//! - connect to etcd
//! - try to become elected leader for the given region - this ensures only one runner is active for
//!   a region. It also allows to start multiple runners and a second one will immediatly start as
//!   soon as the first one revokes or looses its etcd lease.
//! - spawn a transition runner for each transition in the net, which does:
//!     * cancel a running transition. We are not yet running. If a transition is running according
//!       to etcd, it is cancelled and all the tokens are placed back on their input place.
//!       In a node crash this results in a transition firing a second time, probably with exactly
//!       the same data. It is a good idea to have this in mind when writing a
//!       [`exec::TransitionExecutor::run`] method. Ideally the method would be idempotent.
//!     * create a transition instance using [`exec::TransitionExecutor::new`]
//!     * on every change of one of the condition or input arcs for this transition, run
//!       [`exec::TransitionExecutor::check_start`]. Note: As an optimization,
//!       [`check_start`](exec::TransitionExecutor::check_start) can return a specific place to watch.
//!       In this case only changes on that place trigger a recheck.
//!     * if we can start, acquire a distributed lock to the places connected with conditional or
//!       input arcs and run [`check_start`](exec::TransitionExecutor::check_start) again, possibly
//!       with a different state.
//!     * If we can still run, take the tokens indicated by the result of the
//!       [`check_start`](exec::TransitionExecutor::check_start) function, using the fencing tokens
//!       from the acquired locks to ensure we are allowed to do so.
//!     * release the locks (at least locally, see note below)
//!     * Call [`exec::TransitionExecutor::run`]
//!     * Use the return value to move / create tokens on etcd, again by acquiring locks on the
//!       output places and using the fencing tokens to ensure we are allowed to place tokens.
//!     * Loop: check for changes on the connected conditional or input places again.
//!
//! Note that acquiring distributed locks takes a while. To optimize this, the acquired locks are
//! not given back until some other runner requests them. For all places that are only used within
//! one runner, the locks are only taken once. There still is local locking between the different
//! transition tasks, but this is much faster than communicating with etcd.
//!
//! Another optimization concerns places at boundaries between regions.
//! Assume that the following part of a net,
//! <p>
#![doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/diagrams/locking-opt.svg"))]
//! </p>
//! runs on two different runners. The transitions on the left are in one region, the transitions
//! on the right in another one.
//!
//! Normally, everytime a token is passed from the left to the right region, the transition in the
//! left region needs to acquire a lock before placing the token and afterwards the region on the
//! right needs a lock to take the token.
//!
//! However, the output locking is unnecessary, if this place were to be used like a queue, that is,
//! the transitions on the right only ever take the oldest token from the place.
//! In this case, the transitions on the right do exactly the same, even if another token were to be
//! added to the place. Even if this token is added while the right region holds the place's lock.
//!
//! To facilitate that, output locking can be disabled for individual places. Only do this if it
//! does not introduce any logic problems. Always check: For every transition taking tokens from
//! this place, running [`check_start`](exec::TransitionExecutor::check_start) should give the same
//! result, even if additional tokens were to be placed.
//!
//! ## Benchmarks
//!
//! ### Single node
//!
//! Uses N identical copies of the following net:
//! <p>
#![doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/diagrams/bench-single-node.svg"))]
//! </P>
//! Transition 'I' fires in the beginning and places a token on 'P1' with a number value.
//! 'T1' and 'T2' always take this single token, reduce the number by 1 and place it
//! on their output place. When the value is down to 0, 'I' takes the token away again.  
//!
//! The benchmark starts when all the 'I' transitions run for the first time (synchronised with a
//! barrier). Benchmark ends when the 'I' transitions fire for the second time (synchronized
//! with a barrier again).
//!
//! etcd is running locally with a single node. Real deployments would have multiple etcd notes that
//! need to communicate, so changes would need more time, i.e. a chain of transitions that need to
//! happen one after the other (as in the example net) are likely much slower.
//! Furthermore, real transitions would likely have side effects, which need time to be executed.
//!
//! <p>
#![doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/benches/single_node_throughput_log.svg"))]
//! </p>
//!
//! ### Locking
//! Note that the single-node benchmark above never measures acquiring a lock from etcd.
//!
//! When running a single net as shown above, but with 'T1' and 'T2' in different regions,
//! throughput drops to ~5 transitions/s on my machine.
//! One transition requires two lock changes (input and output place) so it currently costs roughly
//! 100 ms to change the lock owner between different runners.
//!
//! CPU: Intel(R) Core(TM) i5-9600 CPU @ 3.10GHz (6 cores)  
//! SSD: Corsair Force MP510
//!

pub mod error;
pub mod etcd;
pub mod exec;
pub mod net;
pub mod runner;

pub use error::Result;
