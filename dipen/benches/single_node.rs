/// Benchmark for a single node with N independent nets
///
/// Uses N identical copies of the following net:
///
///   tr2 ──► pl1 ◄─► tr-init
///   ▲       │        ▲    
///   │       ▼        │    
///  pl2 ◄── tr1       │    
///   │                │    
///   └────────────────┘
///
/// tr-init fires in the beginning and places a token on 'pl1' with a number value.
/// tr1 and tr2 always take this single token, reduce the number by 1 and place it
/// on their output. When the value is down to 0, tr-init takes the token away again.
///
/// Benchmark starts when all the tr-init transitions run for the first time (synchronised with a
/// barrier). Benchmark ends when the tr-init transitions fire for the second time (synchronized
/// with a barrier again).
///
/// etcd is running locally with a single node. Real deployments would have multiple etcd notes that
/// need to communicate, so changes would need more time, i.e. a chain of transitions that need to
/// happen one after the other (as in the example net) are likely much slower.
/// Furthermore, real transitions would likely have side effects, which need time to be executed.
///
/// TODO: benchmark with a realistic deployment (e.g. 3 etcd nodes on different machines.)
use std::any::Any;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use criterion::criterion_group;
use criterion::criterion_main;
use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::SamplingMode;
use criterion::Throughput;
use dipen::error::PetriError;
use dipen::etcd::ETCDConfigBuilder;
use dipen::etcd::ETCDGate;
use dipen::net;
use dipen::net::PetriNetBuilder;
use dipen::runner::ExecutorRegistry;
use tokio::runtime::Runtime;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::error;
use tracing::info;
use tracing::warn;

#[path = "common/mod.rs"]
mod common;

async fn run_benchmark(size: u64, iterations: u16) -> Duration {
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    let run = async move {
        let mut net = PetriNetBuilder::default();
        let mut executors = ExecutorRegistry::new();
        let barrier = tokio::sync::Barrier::new((size + 1) as usize);
        let init_data = Arc::new(common::transitions::InitializeData { barrier, iterations });
        let any_data = init_data.clone() as Arc<dyn Any + Send + Sync>;

        for i in 1..=size {
            let pl1 = format!("pl{i}-1");
            let pl2 = format!("pl{i}-2");
            let tr1 = format!("tr{i}-1");
            let tr2 = format!("tr{i}-2");
            let tr_init = format!("tr{i}-init");
            net.insert_place(net::Place::new(&pl1, false));
            net.insert_place(net::Place::new(&pl2, false));
            net.insert_transition(net::Transition::new(&tr1, "region-1"));
            net.insert_transition(net::Transition::new(&tr2, "region-1"));
            net.insert_arc(net::Arc::new(&pl1, &tr1, net::ArcVariant::In, ""))?;
            net.insert_arc(net::Arc::new(&pl2, &tr1, net::ArcVariant::Out, ""))?;
            net.insert_arc(net::Arc::new(&pl2, &tr2, net::ArcVariant::In, ""))?;
            net.insert_arc(net::Arc::new(&pl1, &tr2, net::ArcVariant::Out, ""))?;
            net.insert_transition(net::Transition::new(&tr_init, "region-1"));
            net.insert_arc(net::Arc::new(&pl1, &tr_init, net::ArcVariant::InOut, ""))?;
            net.insert_arc(net::Arc::new(&pl2, &tr_init, net::ArcVariant::In, ""))?;
            executors.register::<common::transitions::Move>(&tr1, None);
            executors.register::<common::transitions::Move>(&tr2, None);
            executors
                .register::<common::transitions::Initialize>(&tr_init, Some(Arc::clone(&any_data)));
        }
        let shutdown_token = CancellationToken::new();
        let net = Arc::new(net);
        let config = ETCDConfigBuilder::default()
            .endpoints(["localhost:2379"])
            .prefix("bench-single-node/")
            .node_name("node1")
            .region("region-1")
            .lease_ttl(Duration::from_secs(20))
            .build()?;

        let etcd = ETCDGate::new(config);
        let run = dipen::runner::run(Arc::clone(&net), etcd, executors, shutdown_token.clone());
        let mut join_set = JoinSet::new();

        join_set.spawn(async {
            match run.await {
                Ok(_) => {
                    info!("Run finished");
                }
                Err(err) => {
                    error!("Run finished with: {}", err);
                }
            }
        });

        join_set.spawn(async move {
            warn!(" ## Waiting for start ({} iters)", init_data.iterations);
            init_data.barrier.wait().await;
            let start = Instant::now();
            common::transitions::EXECUTION_COUNT.store(0, Ordering::SeqCst);
            init_data.barrier.wait().await;
            warn!(" ## Started");
            init_data.barrier.wait().await;
            let _ = tx.send(start.elapsed());
            warn!(" ## Finished");
            shutdown_token.cancel();
        });
        join_set.join_all().await;
        Ok::<(), PetriError>(())
    };

    run.await.expect("Benchmark failed");
    let count = common::transitions::EXECUTION_COUNT.load(Ordering::SeqCst) as u64;
    let expected = size * iterations as u64;
    if count != expected {
        error!("Wrong count! expected: {}, actual: {}", expected, count);
        panic!("Bechmark is broken.")
    } else {
        warn!(" ## finished single run. Execution count expected: {}, actual: {}", expected, count);
    }
    rx.try_recv().expect("Should have a result!")
}

fn benchmark_single_node(c: &mut Criterion) {
    // uncomment for debugging issues:
    tracing_subscriber::fmt()
        .with_span_events(
            tracing_subscriber::fmt::format::FmtSpan::CLOSE
                | tracing_subscriber::fmt::format::FmtSpan::NEW,
        )
        .compact()
        .with_env_filter(
            // tracing_subscriber::EnvFilter::try_new("info,dipen=debug").unwrap(),
            // tracing_subscriber::EnvFilter::try_new("warn").unwrap(),
            tracing_subscriber::EnvFilter::try_new("error").unwrap(),
        )
        .init();

    let rt = Runtime::new().expect("Failed to create tokio runtime");
    let mut group = c.benchmark_group("single_node");
    for &num_nets in [1, 2, 3, 4, 6, 8, 12, 16, 24, 32, 48, 64, 96, 128].iter() {
        group.throughput(Throughput::Elements(num_nets));
        group.sampling_mode(SamplingMode::Linear);
        group.sample_size(15);
        group.measurement_time(Duration::from_secs(120));
        group.bench_with_input(BenchmarkId::new("single-node", num_nets), &num_nets, |b, &size| {
            b.to_async(&rt).iter_custom(|iters| run_benchmark(size, iters as u16));
        });
    }
    group.finish();
}

criterion_group!(benches, benchmark_single_node);
criterion_main!(benches);
