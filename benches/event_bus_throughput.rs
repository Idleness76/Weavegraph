use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use tokio::runtime::Runtime;
use weavegraph::event_bus::{Event, EventBus};

const BATCH_SIZES: &[usize] = &[64, 256, 1024];

async fn publish_batch(bus: &EventBus, batch: usize) {
    bus.listen_for_events();
    let emitter = bus.get_emitter();
    for i in 0..batch {
        emitter
            .emit(Event::diagnostic("bench", format!("message-{i}")))
            .expect("emit");
    }
    bus.stop_listener().await;
}

fn event_bus_throughput(c: &mut Criterion) {
    let runtime = Runtime::new().expect("runtime");
    let mut group = c.benchmark_group("event_bus_publish");

    for &batch in BATCH_SIZES {
        group.throughput(Throughput::Elements(batch as u64));
        group.bench_with_input(BenchmarkId::from_parameter(batch), &batch, |b, &size| {
            b.to_async(&runtime).iter(|| async {
                let bus = EventBus::default();
                publish_batch(&bus, size).await;
            });
        });
    }

    group.finish();
}

criterion_group!(benches, event_bus_throughput);
criterion_main!(benches);
