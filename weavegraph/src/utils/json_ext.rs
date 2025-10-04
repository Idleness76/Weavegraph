//! JSON manipulation utilities and extensions for the Weavegraph framework.
//!
//! Provides utilities for deep merging JSON objects, pointer-based access,
//! and common JSON manipulation patterns used throughout the framework.

use miette::{Diagnostic, Result};
use serde_json::{Map, Value};
use std::collections::HashMap;
use thiserror::Error;

/// Errors that can occur during JSON operations.
#[derive(Debug, Error, Diagnostic)]
pub enum JsonError {
    /// Invalid JSON pointer format.
    #[error("Invalid JSON pointer: {pointer}")]
    #[diagnostic(code(weavegraph::json::invalid_pointer))]
    InvalidPointer { pointer: String },

    /// JSON merge conflict that cannot be resolved.
    #[error("Merge conflict at path '{path}': cannot merge {left_type} with {right_type}")]
    #[diagnostic(code(weavegraph::json::merge_conflict))]
    MergeConflict {
        path: String,
        left_type: String,
        right_type: String,
    },

    /// Serialization/deserialization error.
    #[error("JSON serialization error: {source}")]
    #[diagnostic(code(weavegraph::json::serde))]
    Serde {
        #[from]
        source: serde_json::Error,
    },
}

/// Strategy for handling conflicts during JSON merges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeStrategy {
    /// Prefer values from the left operand when conflicts occur.
    PreferLeft,
    /// Prefer values from the right operand when conflicts occur.
    PreferRight,
    /// Fail on any merge conflict.
    FailOnConflict,
    /// Attempt to merge values recursively, failing only on type mismatches.
    DeepMerge,
}

/// Performs deep merge of two JSON values according to the specified strategy.
///
/// # Parameters
/// * `left` - Left operand for the merge
/// * `right` - Right operand for the merge  
/// * `strategy` - Strategy for handling conflicts
///
/// # Returns
/// Merged JSON value or error if merge fails
///
/// # Examples
///
/// ```rust
/// use weavegraph::utils::json_ext::{deep_merge, MergeStrategy};
/// use serde_json::{json, Value};
///
/// let left = json!({"a": 1, "b": {"x": 10}});
/// let right = json!({"b": {"y": 20}, "c": 3});
///
/// let merged = deep_merge(&left, &right, MergeStrategy::DeepMerge).unwrap();
/// let expected = json!({"a": 1, "b": {"x": 10, "y": 20}, "c": 3});
/// assert_eq!(merged, expected);
/// ```
pub fn deep_merge(
    left: &Value,
    right: &Value,
    strategy: MergeStrategy,
) -> Result<Value, JsonError> {
    deep_merge_with_path(left, right, strategy, "")
}

/// Internal function that tracks the current path for better error reporting.
fn deep_merge_with_path(
    left: &Value,
    right: &Value,
    strategy: MergeStrategy,
    path: &str,
) -> Result<Value, JsonError> {
    match (left, right) {
        // Both are objects - merge recursively
        (Value::Object(left_obj), Value::Object(right_obj)) => {
            let mut result = Map::new();

            // Add all keys from left
            for (key, value) in left_obj {
                let current_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", path, key)
                };

                if let Some(right_value) = right_obj.get(key) {
                    // Key exists in both - merge recursively
                    let merged = deep_merge_with_path(value, right_value, strategy, &current_path)?;
                    result.insert(key.clone(), merged);
                } else {
                    // Key only in left
                    result.insert(key.clone(), value.clone());
                }
            }

            // Add keys that only exist in right
            for (key, value) in right_obj {
                if !left_obj.contains_key(key) {
                    result.insert(key.clone(), value.clone());
                }
            }

            Ok(Value::Object(result))
        }

        // Both are arrays - strategy determines behavior
        (Value::Array(left_arr), Value::Array(right_arr)) => match strategy {
            MergeStrategy::PreferLeft => Ok(Value::Array(left_arr.clone())),
            MergeStrategy::PreferRight => Ok(Value::Array(right_arr.clone())),
            MergeStrategy::FailOnConflict => Err(JsonError::MergeConflict {
                path: path.to_string(),
                left_type: "array".to_string(),
                right_type: "array".to_string(),
            }),
            MergeStrategy::DeepMerge => {
                // Concatenate arrays
                let mut result = left_arr.clone();
                result.extend(right_arr.clone());
                Ok(Value::Array(result))
            }
        },

        // Same primitive values
        (left_val, right_val) if left_val == right_val => Ok(left_val.clone()),

        // Different values - strategy determines behavior
        (left_val, right_val) => match strategy {
            MergeStrategy::PreferLeft => Ok(left_val.clone()),
            MergeStrategy::PreferRight => Ok(right_val.clone()),
            MergeStrategy::FailOnConflict => Err(JsonError::MergeConflict {
                path: path.to_string(),
                left_type: get_value_type(left_val).to_string(),
                right_type: get_value_type(right_val).to_string(),
            }),
            MergeStrategy::DeepMerge => {
                // For primitives in deep merge, prefer right
                Ok(right_val.clone())
            }
        },
    }
}

/// Get a human-readable type name for a JSON value.
fn get_value_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Merge multiple JSON values using the specified strategy.
///
/// # Parameters
/// * `values` - Iterator of JSON values to merge
/// * `strategy` - Strategy for handling conflicts
///
/// # Returns
/// Merged JSON value or error if merge fails
///
/// # Examples
///
/// ```rust
/// use weavegraph::utils::json_ext::{merge_multiple, MergeStrategy};
/// use serde_json::json;
///
/// let values = vec![
///     json!({"a": 1}),
///     json!({"b": 2}),
///     json!({"c": 3}),
/// ];
///
/// let merged = merge_multiple(values.iter(), MergeStrategy::DeepMerge).unwrap();
/// let expected = json!({"a": 1, "b": 2, "c": 3});
/// assert_eq!(merged, expected);
/// ```
pub fn merge_multiple<'a, I>(values: I, strategy: MergeStrategy) -> Result<Value, JsonError>
where
    I: IntoIterator<Item = &'a Value>,
{
    let mut result = Value::Object(Map::new());
    for value in values {
        result = deep_merge(&result, value, strategy)?;
    }
    Ok(result)
}

/// Get a value using a JSON pointer-like path.
///
/// # Parameters
/// * `value` - JSON value to search in
/// * `path` - Dot-separated path (e.g., "user.profile.name")
///
/// # Returns
/// Reference to the value if found, None otherwise
///
/// # Examples
///
/// ```rust
/// use weavegraph::utils::json_ext::get_by_path;
/// use serde_json::json;
///
/// let data = json!({"user": {"profile": {"name": "Alice"}}});
/// let name = get_by_path(&data, "user.profile.name");
/// assert_eq!(name, Some(&json!("Alice")));
/// ```
#[must_use]
pub fn get_by_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return Some(value);
    }

    let parts: Vec<&str> = path.split('.').collect();
    let mut current = value;

    for part in parts {
        match current {
            Value::Object(obj) => {
                current = obj.get(part)?;
            }
            Value::Array(arr) => {
                let index: usize = part.parse().ok()?;
                current = arr.get(index)?;
            }
            _ => return None,
        }
    }

    Some(current)
}

/// Set a value using a JSON pointer-like path, creating intermediate objects as needed.
///
/// # Parameters
/// * `target` - Mutable JSON value to modify
/// * `path` - Dot-separated path (e.g., "user.profile.name")
/// * `value` - Value to set
///
/// # Returns
/// Result indicating success or failure
///
/// # Examples
///
/// ```rust
/// use weavegraph::utils::json_ext::set_by_path;
/// use serde_json::{json, Value};
///
/// let mut data = json!({});
/// set_by_path(&mut data, "user.profile.name", json!("Alice")).unwrap();
///
/// let expected = json!({"user": {"profile": {"name": "Alice"}}});
/// assert_eq!(data, expected);
/// ```
pub fn set_by_path(target: &mut Value, path: &str, value: Value) -> Result<(), JsonError> {
    if path.is_empty() {
        *target = value;
        return Ok(());
    }

    let parts: Vec<&str> = path.split('.').collect();
    let mut current = target;

    // Navigate to the parent of the target location
    for part in &parts[..parts.len() - 1] {
        match current {
            Value::Object(obj) => {
                current = obj
                    .entry(part.to_string())
                    .or_insert_with(|| Value::Object(Map::new()));
            }
            _ => {
                return Err(JsonError::InvalidPointer {
                    pointer: path.to_string(),
                });
            }
        }
    }

    // Set the final value
    let final_key = parts[parts.len() - 1];
    match current {
        Value::Object(obj) => {
            obj.insert(final_key.to_string(), value);
            Ok(())
        }
        _ => Err(JsonError::InvalidPointer {
            pointer: path.to_string(),
        }),
    }
}

/// Check if a JSON value has a specific structure.
///
/// # Parameters
/// * `value` - JSON value to validate
/// * `expected_keys` - Expected object keys
///
/// # Returns
/// True if the value is an object containing all expected keys
///
/// # Examples
///
/// ```rust
/// use weavegraph::utils::json_ext::has_structure;
/// use serde_json::json;
///
/// let data = json!({"name": "Alice", "age": 30, "email": "alice@example.com"});
/// assert!(has_structure(&data, &["name", "email"]));
/// assert!(!has_structure(&data, &["name", "phone"]));
/// ```
#[must_use]
pub fn has_structure(value: &Value, expected_keys: &[&str]) -> bool {
    match value {
        Value::Object(obj) => expected_keys.iter().all(|key| obj.contains_key(*key)),
        _ => false,
    }
}

/// Convert a HashMap to a JSON object.
///
/// # Parameters
/// * `map` - HashMap to convert
///
/// # Returns
/// JSON object representation
pub fn hashmap_to_json<V: Into<Value>>(map: HashMap<String, V>) -> Value {
    let json_map: Map<String, Value> = map.into_iter().map(|(k, v)| (k, v.into())).collect();
    Value::Object(json_map)
}

/// Extension trait for JSON Value providing additional utility methods.
pub trait JsonValueExt {
    /// Get a value by path with a default if not found.
    fn get_path_or<'a>(&'a self, path: &str, default: &'a Value) -> &'a Value;

    /// Check if this value is an empty object or array.
    fn is_empty_container(&self) -> bool;

    /// Get the number of elements (for objects/arrays) or 1 (for primitives).
    fn element_count(&self) -> usize;

    /// Get all keys if this is an object.
    fn keys(&self) -> Vec<String>;

    /// Deep clone with type conversion.
    fn deep_clone(&self) -> Value;
}

impl JsonValueExt for Value {
    fn get_path_or<'a>(&'a self, path: &str, default: &'a Value) -> &'a Value {
        get_by_path(self, path).unwrap_or(default)
    }

    fn is_empty_container(&self) -> bool {
        match self {
            Value::Object(obj) => obj.is_empty(),
            Value::Array(arr) => arr.is_empty(),
            _ => false,
        }
    }

    fn element_count(&self) -> usize {
        match self {
            Value::Object(obj) => obj.len(),
            Value::Array(arr) => arr.len(),
            _ => 1,
        }
    }

    fn keys(&self) -> Vec<String> {
        match self {
            Value::Object(obj) => obj.keys().cloned().collect(),
            _ => vec![],
        }
    }

    fn deep_clone(&self) -> Value {
        self.clone()
    }
}

/// Trait for types that can be serialized to/from JSON strings with specific error handling.
///
/// This provides a consistent interface for JSON operations throughout the framework.
/// Unlike the other utilities in this module which work with `JsonError`, this trait
/// is generic over the error type to allow different modules to use their own error types.
pub trait JsonSerializable<E>: serde::Serialize + for<'de> serde::de::DeserializeOwned {
    /// Serialize this object to a JSON string.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    fn to_json_string(&self) -> Result<String, E>;

    /// Deserialize an object from a JSON string.
    ///
    /// # Errors
    ///
    /// Returns an error if deserialization fails.
    fn from_json_str(s: &str) -> Result<Self, E>;
}

/// Helper for JSON serialization with custom error context.
///
/// This utility provides context-aware JSON serialization that can be used
/// by different modules with their own error types.
///
/// # Parameters
/// * `value` - The value to serialize
/// * `context` - Context string for error messages
/// * `error_mapper` - Function to convert serde_json::Error to the target error type
///
/// # Returns
/// JSON string or mapped error
pub fn serialize_with_context<T, E>(
    value: &T,
    context: &str,
    error_mapper: impl FnOnce(serde_json::Error, &str) -> E,
) -> Result<String, E>
where
    T: serde::Serialize,
{
    serde_json::to_string(value).map_err(|e| error_mapper(e, context))
}

/// Helper for JSON deserialization with custom error context.
///
/// This utility provides context-aware JSON deserialization that can be used
/// by different modules with their own error types.
///
/// # Parameters
/// * `json` - The JSON string to deserialize
/// * `context` - Context string for error messages
/// * `error_mapper` - Function to convert serde_json::Error to the target error type
///
/// # Returns
/// Deserialized value or mapped error
pub fn deserialize_with_context<T, E>(
    json: &str,
    context: &str,
    error_mapper: impl FnOnce(serde_json::Error, &str) -> E,
) -> Result<T, E>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_str(json).map_err(|e| error_mapper(e, context))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_deep_merge_objects() {
        let left = json!({"a": 1, "b": {"x": 10}});
        let right = json!({"b": {"y": 20}, "c": 3});

        let merged = deep_merge(&left, &right, MergeStrategy::DeepMerge).unwrap();
        let expected = json!({"a": 1, "b": {"x": 10, "y": 20}, "c": 3});
        assert_eq!(merged, expected);
    }

    #[test]
    fn test_merge_strategy_prefer_left() {
        let left = json!({"a": 1});
        let right = json!({"a": 2});

        let merged = deep_merge(&left, &right, MergeStrategy::PreferLeft).unwrap();
        assert_eq!(merged, json!({"a": 1}));
    }

    #[test]
    fn test_merge_strategy_prefer_right() {
        let left = json!({"a": 1});
        let right = json!({"a": 2});

        let merged = deep_merge(&left, &right, MergeStrategy::PreferRight).unwrap();
        assert_eq!(merged, json!({"a": 2}));
    }

    #[test]
    fn test_merge_arrays() {
        let left = json!([1, 2]);
        let right = json!([3, 4]);

        let merged = deep_merge(&left, &right, MergeStrategy::DeepMerge).unwrap();
        assert_eq!(merged, json!([1, 2, 3, 4]));
    }

    #[test]
    fn test_get_by_path() {
        let data = json!({"user": {"profile": {"name": "Alice"}}});

        assert_eq!(
            get_by_path(&data, "user.profile.name"),
            Some(&json!("Alice"))
        );
        assert_eq!(
            get_by_path(&data, "user.profile"),
            Some(&json!({"name": "Alice"}))
        );
        assert_eq!(get_by_path(&data, "user.missing"), None);
    }

    #[test]
    fn test_set_by_path() {
        let mut data = json!({});
        set_by_path(&mut data, "user.profile.name", json!("Alice")).unwrap();

        let expected = json!({"user": {"profile": {"name": "Alice"}}});
        assert_eq!(data, expected);
    }

    #[test]
    fn test_has_structure() {
        let data = json!({"name": "Alice", "age": 30});

        assert!(has_structure(&data, &["name"]));
        assert!(has_structure(&data, &["name", "age"]));
        assert!(!has_structure(&data, &["name", "email"]));
        assert!(!has_structure(&json!("not an object"), &["name"]));
    }

    #[test]
    fn test_json_value_ext() {
        let data = json!({"a": 1, "b": [1, 2, 3]});

        assert_eq!(data.get_path_or("a", &json!(0)), &json!(1));
        assert_eq!(data.get_path_or("missing", &json!(0)), &json!(0));
        assert_eq!(data.element_count(), 2);
        assert_eq!(data.keys(), vec!["a", "b"]);
        assert!(!data.is_empty_container());

        let empty_obj = json!({});
        assert!(empty_obj.is_empty_container());
    }

    #[test]
    fn test_merge_multiple() {
        let values = [json!({"a": 1}), json!({"b": 2}), json!({"c": 3})];

        let merged = merge_multiple(values.iter(), MergeStrategy::DeepMerge).unwrap();
        let expected = json!({"a": 1, "b": 2, "c": 3});
        assert_eq!(merged, expected);
    }

    #[test]
    fn test_serialize_with_context() {
        #[derive(Debug, PartialEq)]
        struct TestError(String);

        let data = json!({"key": "value"});
        let result =
            serialize_with_context(&data, "test", |e, ctx| TestError(format!("{ctx}: {e}")));

        assert!(result.is_ok());
        assert!(result.unwrap().contains("\"key\":\"value\""));

        // Test error case
        struct BadSerialize;
        impl serde::Serialize for BadSerialize {
            fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                Err(serde::ser::Error::custom("test error"))
            }
        }

        let bad_data = BadSerialize;
        let error_result = serialize_with_context(&bad_data, "test_context", |e, ctx| {
            TestError(format!("{ctx}: {e}"))
        });

        assert!(error_result.is_err());
        assert_eq!(
            error_result.unwrap_err(),
            TestError("test_context: test error".to_string())
        );
    }

    #[test]
    fn test_deserialize_with_context() {
        #[derive(Debug, PartialEq)]
        struct TestError(String);

        use serde_json::Value;
        let json_str = r#"{"key": "value"}"#;
        let result: Result<Value, TestError> =
            deserialize_with_context(json_str, "test", |e, ctx| TestError(format!("{ctx}: {e}")));

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), json!({"key": "value"}));

        // Test error case
        let bad_json = r#"{invalid json}"#;
        let error_result: Result<Value, TestError> =
            deserialize_with_context(bad_json, "test_context", |e, ctx| {
                TestError(format!("{ctx}: {e}"))
            });

        assert!(error_result.is_err());
        let error = error_result.unwrap_err();
        assert!(error.0.starts_with("test_context:"));
        // Just check that we got some error message about parsing
        assert!(error.0.len() > "test_context:".len());
    }
}
