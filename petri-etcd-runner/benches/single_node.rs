use std::any::Any;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use criterion::criterion_group;
use criterion::criterion_main;
use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::SamplingMode;
use criterion::Throughput;
use petri_etcd_runner::error::PetriError;
use petri_etcd_runner::net;
use petri_etcd_runner::net::PetriNetBuilder;
use petri_etcd_runner::runner::ExecutorRegistry;
use petri_etcd_runner::ETCDConfigBuilder;
use petri_etcd_runner::ETCDGate;
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
            net.insert_place(net::Place::new(&pl1, false))?;
            net.insert_place(net::Place::new(&pl2, false))?;
            net.insert_transition(net::Transition::new(&tr1, "region-1"))?;
            net.insert_transition(net::Transition::new(&tr2, "region-1"))?;
            net.insert_arc(net::Arc::new(&pl1, &tr1, net::ArcVariant::In, "".into()))?;
            net.insert_arc(net::Arc::new(&pl2, &tr1, net::ArcVariant::Out, "".into()))?;
            net.insert_arc(net::Arc::new(&pl2, &tr2, net::ArcVariant::In, "".into()))?;
            net.insert_arc(net::Arc::new(&pl1, &tr2, net::ArcVariant::Out, "".into()))?;
            net.insert_transition(net::Transition::new(&tr_init, "region-1"))?;
            net.insert_arc(net::Arc::new(&pl1, &tr_init, net::ArcVariant::InOut, "".into()))?;
            net.insert_arc(net::Arc::new(&pl2, &tr_init, net::ArcVariant::In, "".into()))?;
            executors.register::<common::transitions::Move>(&tr1, None);
            executors.register::<common::transitions::Move>(&tr2, None);
            executors
                .register::<common::transitions::Initialize>(&tr_init, Some(Arc::clone(&any_data)));
        }
        let shutdown_token = CancellationToken::new();
        let net = Arc::new(net);
        let config = ETCDConfigBuilder::default()
            .endpoints(["localhost:2379"])
            .prefix("/bench-single-node/")
            .node_name("node1")
            .region("region-1")
            .build()?;

        let etcd = ETCDGate::new(config);
        let run = petri_etcd_runner::runner::run(
            Arc::clone(&net),
            etcd,
            executors,
            shutdown_token.clone(),
        );
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
    warn!(" ## finished single run");
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
            // tracing_subscriber::EnvFilter::try_new("info,petri_etcd_runner=debug").unwrap(),
            tracing_subscriber::EnvFilter::try_new("warn").unwrap(),
            // tracing_subscriber::EnvFilter::try_new("error").unwrap(),
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
