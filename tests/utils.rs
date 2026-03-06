use serde_json::json;
use weavegraph::utils::clock::*;
use weavegraph::utils::collections::*;
use weavegraph::utils::deterministic_rng::*;
use weavegraph::utils::id_generator::*;
use weavegraph::utils::json_ext::*;

#[test]
fn test_collections_helpers() {
    let mut map = new_extra_map();
    map.insert_string("name", "test");
    map.insert_number("count", 42);
    map.insert_bool("enabled", true);

    assert_eq!(map.get_string("name").unwrap(), "test");
    assert_eq!(map.get_number("count").unwrap(), 42.into());
    assert!(map.get_bool("enabled").unwrap());

    let merged = merge_extra_maps([&map, &extra_map_from_pairs([("added", json!(1))])]);
    assert!(merged.contains_key("name"));
    assert_eq!(merged.get("added"), Some(&json!(1)));
}

#[test]
fn test_json_ext_deep_merge_and_path() {
    let left = json!({"a": 1, "b": {"x": 10}});
    let right = json!({"b": {"y": 20}, "c": 3});
    let merged = deep_merge(&left, &right, MergeStrategy::DeepMerge).unwrap();
    assert_eq!(merged, json!({"a":1, "b": {"x":10, "y":20}, "c":3}));

    assert_eq!(get_by_path(&merged, "b.x"), Some(&json!(10)));
    assert!(has_structure(&merged, &["a", "b", "c"]));
}

#[test]
fn test_id_generator_basics() {
    let id_gen = IdGenerator::new();
    let run = id_gen.generate_run_id();
    assert!(run.starts_with("run-"));

    let config = IdConfig {
        seed: Some(7),
        use_counter: true,
        ..Default::default()
    };
    let det = IdGenerator::with_config(config);
    let id1 = det.generate_id();
    let id2 = det.generate_id();
    assert_ne!(id1, id2);
}

#[test]
fn test_deterministic_rng() {
    let mut r1 = DeterministicRng::new(42);
    let mut r2 = DeterministicRng::new(42);
    assert_eq!(r1.random_u64(), r2.random_u64());
    assert_eq!(r1.random_string(6).len(), 6);
}

#[test]
fn test_clock_utils() {
    let mut clock = MockClock::new(1000);
    assert_eq!(clock.now(), 1000);
    clock.advance_secs(10);
    assert!(clock.has_elapsed(1000, std::time::Duration::from_secs(10)));
    let formatted = time_utils::format_timestamp(0);
    assert!(formatted.contains("1970"));
}
