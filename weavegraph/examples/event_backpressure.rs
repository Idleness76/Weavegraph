//! Demonstrates event bus backpressure handling and lag recovery.
//!
//! This example shows:
//! - How to detect lagged event streams
//! - Metrics for monitoring dropped events
//! - Proper handling of RecvError::Lagged

use std::time::Duration;
use tokio::time::sleep;
use weavegraph::event_bus::Event;
use weavegraph::runtimes::{EventBusConfig, SinkConfig};

#[tokio::main]
async fn main() {
    // Create a bus with tiny capacity to trigger lag
    let bus = EventBusConfig::new(2, vec![SinkConfig::Memory]).build_event_bus();
    let emitter = bus.get_emitter();
    let mut stream = bus.subscribe();

    // Flood the bus
    for i in 0..100 {
        emitter
            .emit(Event::diagnostic("flood", format!("msg {i}")))
            .ok();
    }

    // Attempt to consume - will get Lagged error
    sleep(Duration::from_millis(10)).await;

    // Drain for a short period and then exit. This prevents the example from hanging.
    // `next_timeout` retries on lag and returns None on timeout or channel close.
    loop {
        match stream.next_timeout(Duration::from_millis(50)).await {
            Some(event) => println!("Received: {}", event.message()),
            None => break,
        }
    }

    let metrics = bus.metrics();
    println!("Capacity: {}", metrics.capacity);
    println!("Total dropped: {}", metrics.dropped);
}
