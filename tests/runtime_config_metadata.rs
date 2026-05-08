use async_trait::async_trait;
use proptest::prelude::*;
use std::sync::Arc;
use weavegraph::runtimes::checkpointer::Result as CheckpointerResult;
use weavegraph::runtimes::runtime_config::DiagnosticsConfig;
use weavegraph::runtimes::{Checkpoint, Checkpointer, EventBusConfig, RuntimeConfig, SinkConfig};
use weavegraph::utils::clock::MockClock;

#[derive(Default)]
struct NoopCheckpointer;

#[async_trait]
impl Checkpointer for NoopCheckpointer {
    async fn save(&self, _checkpoint: Checkpoint) -> CheckpointerResult<()> {
        Ok(())
    }

    async fn load_latest(&self, _session_id: &str) -> CheckpointerResult<Option<Checkpoint>> {
        Ok(None)
    }

    async fn list_sessions(&self) -> CheckpointerResult<Vec<String>> {
        Ok(Vec::new())
    }
}

#[test]
fn runtime_config_hash_is_stable_and_changes_for_metadata_boundaries() {
    let base = RuntimeConfig::new(
        Some("session-a".to_string()),
        Some("db-a.sqlite".to_string()),
    )
    .with_memory_event_bus();
    let same = RuntimeConfig::new(
        Some("session-a".to_string()),
        Some("db-a.sqlite".to_string()),
    )
    .with_memory_event_bus();

    assert_eq!(base.config_hash(), same.config_hash());
    assert_eq!(base.clock_mode(), "unset");
    assert!(base.clock().is_none());

    let with_session = RuntimeConfig::new(
        Some("session-b".to_string()),
        Some("db-a.sqlite".to_string()),
    )
    .with_memory_event_bus();
    let with_sqlite = RuntimeConfig::new(
        Some("session-a".to_string()),
        Some("db-b.sqlite".to_string()),
    )
    .with_memory_event_bus();
    let with_clock = base.clone().with_clock(Arc::new(MockClock::new(7)));
    let with_custom_checkpointer = base.clone().checkpointer_custom(Arc::new(NoopCheckpointer));

    assert_ne!(base.config_hash(), with_session.config_hash());
    assert_ne!(base.config_hash(), with_sqlite.config_hash());
    assert_ne!(base.config_hash(), with_clock.config_hash());
    assert_ne!(base.config_hash(), with_custom_checkpointer.config_hash());
    assert_eq!(with_clock.clock_mode(), "configured");
    assert!(with_clock.clock().is_some());
    assert!(with_custom_checkpointer.custom_checkpointer().is_some());
}

#[test]
fn event_bus_config_normalizes_capacity_and_deduplicates_sinks() {
    let config = EventBusConfig::new(0, Vec::new())
        .add_sink(SinkConfig::Memory)
        .add_sink(SinkConfig::Memory)
        .add_sink(SinkConfig::StdOut);

    assert_eq!(
        config.buffer_capacity(),
        EventBusConfig::DEFAULT_BUFFER_CAPACITY
    );
    assert_eq!(config.sinks(), &[SinkConfig::Memory, SinkConfig::StdOut]);

    let signature = config.metadata_signature();
    assert!(signature.contains(&"event_buffer:1024".to_string()));
    assert!(signature.contains(&"event_sink:0:Memory".to_string()));
    assert!(signature.contains(&"event_sink:1:StdOut".to_string()));
}

#[test]
fn diagnostics_config_defaults_and_overrides_are_reflected_in_metadata() {
    let default_for_zero = DiagnosticsConfig::default_with_capacity(0);
    assert_eq!(default_for_zero.effective_capacity(99), 1);

    let diagnostics = DiagnosticsConfig {
        enabled: false,
        buffer_capacity: None,
        emit_to_events: true,
    };
    let config = EventBusConfig::new(64, vec![SinkConfig::Memory]).with_diagnostics(diagnostics);

    assert_eq!(config.buffer_capacity(), 64);
    let signature = config.metadata_signature();
    assert!(signature.contains(&"diagnostics_enabled:false".to_string()));
    assert!(signature.contains(&"diagnostics_capacity:64".to_string()));
    assert!(signature.contains(&"diagnostics_emit_to_events:true".to_string()));

    let _bus = config.build_event_bus();
}

proptest! {
    #[test]
    fn prop_runtime_config_hash_is_deterministic_for_generated_metadata(
        session_id in "[A-Za-z0-9_-]{0,24}",
        sqlite_db_name in "[A-Za-z0-9_.-]{1,24}",
        capacity in 1usize..2048,
        use_memory_sink in any::<bool>(),
        diagnostics_enabled in any::<bool>(),
        emit_diagnostics in any::<bool>(),
    ) {
        let sinks = if use_memory_sink {
            vec![SinkConfig::Memory]
        } else {
            vec![SinkConfig::StdOut]
        };
        let diagnostics = DiagnosticsConfig {
            enabled: diagnostics_enabled,
            buffer_capacity: Some(capacity),
            emit_to_events: emit_diagnostics,
        };
        let config = RuntimeConfig::new(Some(session_id.clone()), Some(sqlite_db_name.clone()))
            .with_event_bus(EventBusConfig::new(capacity, sinks.clone()).with_diagnostics(diagnostics.clone()));
        let same = RuntimeConfig::new(Some(session_id), Some(sqlite_db_name))
            .with_event_bus(EventBusConfig::new(capacity, sinks).with_diagnostics(diagnostics));

        prop_assert_eq!(config.config_hash(), same.config_hash());
    }
}
