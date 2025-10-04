//! Collection utilities and common patterns for the Weavegraph framework.
//!
//! This module provides type-safe, ergonomic utilities for working with
//! common collection patterns throughout the codebase, particularly for
//! extra data maps, state snapshots, and channel operations.

use miette::{Diagnostic, Result};
use rustc_hash::FxHashMap;
use serde_json::Value;
use std::collections::HashMap;
use thiserror::Error;

/// Errors that can occur during collection operations.
#[derive(Debug, Error, Diagnostic)]
pub enum CollectionError {
    /// Attempted to access a key that doesn't exist.
    #[error("Key '{key}' not found in collection")]
    #[diagnostic(code(weavegraph::collections::missing_key))]
    MissingKey {
        /// The key that was not found
        key: String,
    },

    /// Invalid type conversion during value extraction.
    #[error("Invalid type conversion for key '{key}': expected {expected}, found {found}")]
    #[diagnostic(code(weavegraph::collections::type_mismatch))]
    TypeMismatch {
        /// The key where the type mismatch occurred
        key: String,
        /// The expected type as a string
        expected: String,
        /// The actual type that was found
        found: String,
    },

    /// JSON serialization/deserialization error.
    #[error("JSON operation failed: {source}")]
    #[diagnostic(code(weavegraph::collections::json))]
    Json {
        /// The underlying JSON error
        #[from]
        source: serde_json::Error,
    },
}

/// Creates a new `FxHashMap` for string keys and JSON values.
///
/// This is the standard pattern throughout the codebase for extra data storage.
/// Uses `FxHashMap` for better performance with string keys.
///
/// # Examples
///
/// ```rust
/// use weavegraph::utils::collections::new_extra_map;
/// use serde_json::json;
///
/// let mut extra = new_extra_map();
/// extra.insert("key".to_string(), json!("value"));
/// extra.insert("count".to_string(), json!(42));
/// ```
#[must_use]
#[inline]
pub fn new_extra_map() -> FxHashMap<String, Value> {
    FxHashMap::default()
}

/// Creates a new `FxHashMap` with the specified capacity.
///
/// Useful when you know the approximate size of the map ahead of time
/// to avoid reallocations during insertion.
///
/// # Parameters
/// * `capacity` - Initial capacity hint for the map
///
/// # Returns
/// A new `FxHashMap` with the specified capacity.
///
/// # Examples
///
/// ```rust
/// use weavegraph::utils::collections::new_extra_map_with_capacity;
///
/// // Pre-allocate for known size
/// let mut extra = new_extra_map_with_capacity(10);
/// ```
#[must_use]
#[inline]
pub fn new_extra_map_with_capacity(capacity: usize) -> FxHashMap<String, Value> {
    FxHashMap::with_capacity_and_hasher(capacity, Default::default())
}

/// Creates a new `FxHashMap` from key-value pairs.
///
/// Convenience function for creating extra maps with initial data.
///
/// # Parameters
/// * `pairs` - Iterator of (key, value) pairs
///
/// # Returns
/// A new `FxHashMap` populated with the given pairs.
///
/// # Examples
///
/// ```rust
/// use weavegraph::utils::collections::extra_map_from_pairs;
/// use serde_json::json;
///
/// let extra = extra_map_from_pairs([
///     ("name", json!("test")),
///     ("count", json!(42)),
/// ]);
/// ```
#[must_use]
pub fn extra_map_from_pairs<I, K, V>(pairs: I) -> FxHashMap<String, Value>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<Value>,
{
    pairs
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect()
}

/// Merges multiple extra maps into one, with later maps overriding earlier ones.
///
/// This is useful for combining extra data from multiple sources in a
/// predictable way.
///
/// # Parameters
/// * `maps` - Iterator of maps to merge
///
/// # Returns
/// A new map containing all entries, with conflicts resolved by last-wins.
///
/// # Examples
///
/// ```rust
/// use weavegraph::utils::collections::{new_extra_map, merge_extra_maps};
/// use serde_json::json;
///
/// let mut map1 = new_extra_map();
/// map1.insert("a".into(), json!(1));
/// map1.insert("b".into(), json!(2));
///
/// let mut map2 = new_extra_map();
/// map2.insert("b".into(), json!(3)); // Overrides map1
/// map2.insert("c".into(), json!(4));
///
/// let merged = merge_extra_maps([&map1, &map2]);
/// assert_eq!(merged["b"], json!(3)); // Last wins
/// ```
#[must_use]
pub fn merge_extra_maps<'a, I>(maps: I) -> FxHashMap<String, Value>
where
    I: IntoIterator<Item = &'a FxHashMap<String, Value>>,
{
    let mut result = new_extra_map();
    for map in maps {
        result.extend(map.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    result
}

/// Extension trait for working with extra data maps in a type-safe manner.
///
/// This trait provides convenient methods for inserting and retrieving typed values
/// from JSON-based extra data maps. It's designed specifically for the common pattern
/// of storing heterogeneous data in `FxHashMap<String, Value>` structures throughout
/// the Weavegraph framework.
///
/// # Design Principles
///
/// - **Type Safety**: Methods ensure type correctness at compile time where possible
/// - **Error Handling**: Retrieval methods return `Result` for proper error handling
/// - **Ergonomics**: Convenient insertion methods that accept `Into` traits
/// - **JSON Compatibility**: Full support for JSON serialization/deserialization
///
/// # Examples
///
/// ```rust
/// use weavegraph::utils::collections::{new_extra_map, ExtraMapExt};
///
/// let mut extra = new_extra_map();
///
/// // Insert different types of data
/// extra.insert_string("name", "Alice");
/// extra.insert_number("age", 30);
/// extra.insert_bool("active", true);
///
/// // Retrieve with type checking
/// assert_eq!(extra.get_string("name").unwrap(), "Alice");
/// assert_eq!(extra.get_number("age").unwrap(), 30.into());
/// assert_eq!(extra.get_bool("active").unwrap(), true);
///
/// // Type validation
/// assert!(extra.has_typed("name", "string"));
/// assert!(!extra.has_typed("name", "number"));
/// ```
pub trait ExtraMapExt {
    /// Insert a string value into the extra map.
    ///
    /// This method provides a convenient way to insert string values without
    /// manually wrapping them in `Value::String`.
    ///
    /// # Parameters
    /// * `key` - Map key (will be converted to String)
    /// * `value` - String value (will be converted to String)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::utils::collections::{new_extra_map, ExtraMapExt};
    ///
    /// let mut extra = new_extra_map();
    /// extra.insert_string("username", "alice123");
    /// extra.insert_string("display_name", String::from("Alice Smith"));
    /// ```
    fn insert_string(&mut self, key: impl Into<String>, value: impl Into<String>);

    /// Insert a numeric value into the extra map.
    ///
    /// Accepts any value that can be converted to `serde_json::Number`,
    /// including integers and floats.
    ///
    /// # Parameters
    /// * `key` - Map key (will be converted to String)
    /// * `value` - Numeric value (will be converted to serde_json::Number)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::utils::collections::{new_extra_map, ExtraMapExt};
    /// use serde_json::Number;
    ///
    /// let mut extra = new_extra_map();
    /// extra.insert_number("count", 42);
    ///
    /// // For floats, create Number explicitly
    /// let price = Number::from_f64(19.99).unwrap();
    /// extra.insert_number("price", price);
    /// ```
    fn insert_number(&mut self, key: impl Into<String>, value: impl Into<serde_json::Number>);

    /// Insert a boolean value into the extra map.
    ///
    /// # Parameters
    /// * `key` - Map key (will be converted to String)
    /// * `value` - Boolean value
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::utils::collections::{new_extra_map, ExtraMapExt};
    ///
    /// let mut extra = new_extra_map();
    /// extra.insert_bool("enabled", true);
    /// extra.insert_bool("verified", false);
    /// ```
    fn insert_bool(&mut self, key: impl Into<String>, value: bool);

    /// Insert any serializable value into the extra map.
    ///
    /// This method can handle complex types by serializing them to JSON.
    /// It's useful for storing structured data that needs to be preserved
    /// exactly as it was inserted.
    ///
    /// # Parameters
    /// * `key` - Map key (will be converted to String)
    /// * `value` - Any value that implements `serde::Serialize`
    ///
    /// # Returns
    /// * `Ok(())` - Value was successfully serialized and inserted
    /// * `Err(CollectionError::Json)` - Serialization failed
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::utils::collections::{new_extra_map, ExtraMapExt};
    /// use serde::Serialize;
    ///
    /// #[derive(Serialize)]
    /// struct UserProfile {
    ///     id: u64,
    ///     email: String,
    /// }
    ///
    /// let mut extra = new_extra_map();
    /// let profile = UserProfile {
    ///     id: 123,
    ///     email: "alice@example.com".to_string(),
    /// };
    ///
    /// extra.insert_json("profile", &profile).unwrap();
    /// ```
    fn insert_json<T: serde::Serialize>(
        &mut self,
        key: impl Into<String>,
        value: T,
    ) -> Result<(), CollectionError>;

    /// Get a string value from the map with type validation.
    ///
    /// Returns the string value if the key exists and contains a string,
    /// otherwise returns an appropriate error.
    ///
    /// # Parameters
    /// * `key` - Map key to look up
    ///
    /// # Returns
    /// * `Ok(String)` - The string value
    /// * `Err(CollectionError::MissingKey)` - Key doesn't exist
    /// * `Err(CollectionError::TypeMismatch)` - Key exists but value is not a string
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::utils::collections::{new_extra_map, ExtraMapExt};
    ///
    /// let mut extra = new_extra_map();
    /// extra.insert_string("name", "Alice");
    /// extra.insert_number("count", 42);
    ///
    /// assert_eq!(extra.get_string("name").unwrap(), "Alice");
    /// assert!(extra.get_string("count").is_err()); // Type mismatch
    /// assert!(extra.get_string("missing").is_err()); // Missing key
    /// ```
    fn get_string(&self, key: &str) -> Result<String, CollectionError>;

    /// Get a numeric value from the map with type validation.
    ///
    /// Returns the numeric value if the key exists and contains a number,
    /// otherwise returns an appropriate error.
    ///
    /// # Parameters
    /// * `key` - Map key to look up
    ///
    /// # Returns
    /// * `Ok(serde_json::Number)` - The numeric value
    /// * `Err(CollectionError::MissingKey)` - Key doesn't exist
    /// * `Err(CollectionError::TypeMismatch)` - Key exists but value is not a number
    fn get_number(&self, key: &str) -> Result<serde_json::Number, CollectionError>;

    /// Get a boolean value from the map with type validation.
    ///
    /// Returns the boolean value if the key exists and contains a boolean,
    /// otherwise returns an appropriate error.
    ///
    /// # Parameters
    /// * `key` - Map key to look up
    ///
    /// # Returns
    /// * `Ok(bool)` - The boolean value
    /// * `Err(CollectionError::MissingKey)` - Key doesn't exist
    /// * `Err(CollectionError::TypeMismatch)` - Key exists but value is not a boolean
    fn get_bool(&self, key: &str) -> Result<bool, CollectionError>;

    /// Get a value and deserialize it to the specified type.
    ///
    /// This method attempts to deserialize the JSON value at the given key
    /// to the requested type. It's useful for retrieving complex structured
    /// data that was stored using `insert_json`.
    ///
    /// # Parameters
    /// * `key` - Map key to look up
    ///
    /// # Returns
    /// * `Ok(T)` - Successfully deserialized value
    /// * `Err(CollectionError::MissingKey)` - Key doesn't exist
    /// * `Err(CollectionError::Json)` - Deserialization failed
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::utils::collections::{new_extra_map, ExtraMapExt};
    /// use serde::{Deserialize, Serialize};
    ///
    /// #[derive(Serialize, Deserialize, PartialEq, Debug)]
    /// struct Config {
    ///     timeout: u64,
    ///     retries: u32,
    /// }
    ///
    /// let mut extra = new_extra_map();
    /// let config = Config { timeout: 5000, retries: 3 };
    /// extra.insert_json("config", &config).unwrap();
    ///
    /// let retrieved: Config = extra.get_typed("config").unwrap();
    /// assert_eq!(retrieved, config);
    /// ```
    fn get_typed<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<T, CollectionError>;

    /// Check if a key exists and has the expected type.
    ///
    /// This method provides a quick way to validate the presence and type
    /// of a value without retrieving it. Useful for conditional logic
    /// and validation scenarios.
    ///
    /// # Parameters
    /// * `key` - Map key to check
    /// * `expected_type` - Expected type as string ("string", "number", "bool", "array", "object", "null")
    ///
    /// # Returns
    /// `true` if the key exists and has the expected type, `false` otherwise
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::utils::collections::{new_extra_map, ExtraMapExt};
    ///
    /// let mut extra = new_extra_map();
    /// extra.insert_string("name", "Alice");
    /// extra.insert_number("age", 30);
    ///
    /// assert!(extra.has_typed("name", "string"));
    /// assert!(extra.has_typed("age", "number"));
    /// assert!(!extra.has_typed("name", "number"));
    /// assert!(!extra.has_typed("missing", "string"));
    /// ```
    fn has_typed(&self, key: &str, expected_type: &str) -> bool;
}

impl ExtraMapExt for FxHashMap<String, Value> {
    /// Inserts a string value into the FxHashMap as a JSON String value.
    ///
    /// This implementation converts both the key and value to their owned forms
    /// and wraps the value in `Value::String` for JSON compatibility.
    fn insert_string(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.insert(key.into(), Value::String(value.into()));
    }

    /// Inserts a numeric value into the FxHashMap as a JSON Number value.
    ///
    /// This implementation accepts any type that can be converted to `serde_json::Number`
    /// and wraps it in `Value::Number`.
    fn insert_number(&mut self, key: impl Into<String>, value: impl Into<serde_json::Number>) {
        self.insert(key.into(), Value::Number(value.into()));
    }

    /// Inserts a boolean value into the FxHashMap as a JSON Bool value.
    ///
    /// This implementation directly wraps the boolean in `Value::Bool`.
    fn insert_bool(&mut self, key: impl Into<String>, value: bool) {
        self.insert(key.into(), Value::Bool(value));
    }

    /// Serializes and inserts any serializable value into the FxHashMap.
    ///
    /// This implementation uses `serde_json::to_value` to convert the input
    /// to a JSON value, which may fail if serialization is not possible.
    /// The serialized value is then inserted into the map.
    fn insert_json<T: serde::Serialize>(
        &mut self,
        key: impl Into<String>,
        value: T,
    ) -> Result<(), CollectionError> {
        let json_value = serde_json::to_value(value)?;
        self.insert(key.into(), json_value);
        Ok(())
    }

    /// Retrieves a string value from the FxHashMap with type validation.
    ///
    /// This implementation checks that the value exists and is of type `Value::String`,
    /// returning appropriate errors for missing keys or type mismatches.
    /// The string is cloned to return an owned value.
    fn get_string(&self, key: &str) -> Result<String, CollectionError> {
        match self.get(key) {
            Some(Value::String(s)) => Ok(s.clone()),
            Some(other) => Err(CollectionError::TypeMismatch {
                key: key.to_string(),
                expected: "string".to_string(),
                found: format!("{:?}", other),
            }),
            None => Err(CollectionError::MissingKey {
                key: key.to_string(),
            }),
        }
    }

    /// Retrieves a numeric value from the FxHashMap with type validation.
    ///
    /// This implementation checks that the value exists and is of type `Value::Number`,
    /// returning appropriate errors for missing keys or type mismatches.
    /// The number is cloned to return an owned value.
    fn get_number(&self, key: &str) -> Result<serde_json::Number, CollectionError> {
        match self.get(key) {
            Some(Value::Number(n)) => Ok(n.clone()),
            Some(other) => Err(CollectionError::TypeMismatch {
                key: key.to_string(),
                expected: "number".to_string(),
                found: format!("{:?}", other),
            }),
            None => Err(CollectionError::MissingKey {
                key: key.to_string(),
            }),
        }
    }

    /// Retrieves a boolean value from the FxHashMap with type validation.
    ///
    /// This implementation checks that the value exists and is of type `Value::Bool`,
    /// returning appropriate errors for missing keys or type mismatches.
    /// The boolean is dereferenced to return the primitive value.
    fn get_bool(&self, key: &str) -> Result<bool, CollectionError> {
        match self.get(key) {
            Some(Value::Bool(b)) => Ok(*b),
            Some(other) => Err(CollectionError::TypeMismatch {
                key: key.to_string(),
                expected: "boolean".to_string(),
                found: format!("{:?}", other),
            }),
            None => Err(CollectionError::MissingKey {
                key: key.to_string(),
            }),
        }
    }

    /// Deserializes a JSON value to the specified type.
    ///
    /// This implementation uses `serde_json::from_value` to convert the stored
    /// JSON value to the requested type. The value is cloned before deserialization.
    /// Returns JSON errors if deserialization fails.
    fn get_typed<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<T, CollectionError> {
        match self.get(key) {
            Some(value) => serde_json::from_value(value.clone())
                .map_err(|e| CollectionError::Json { source: e }),
            None => Err(CollectionError::MissingKey {
                key: key.to_string(),
            }),
        }
    }

    /// Checks if a key exists and matches the expected JSON type.
    ///
    /// This implementation performs pattern matching against the JSON value variants
    /// and the expected type string. It's optimized to avoid cloning or deserializing
    /// the actual value, making it suitable for validation scenarios.
    fn has_typed(&self, key: &str, expected_type: &str) -> bool {
        matches!(
            (self.get(key), expected_type),
            (Some(Value::String(_)), "string")
                | (Some(Value::Number(_)), "number")
                | (Some(Value::Bool(_)), "bool")
                | (Some(Value::Array(_)), "array")
                | (Some(Value::Object(_)), "object")
                | (Some(Value::Null), "null")
        )
    }
}

/// Extension trait for `HashMap` with string keys to provide common operations.
///
/// This trait extends both `HashMap<String, V>` and `FxHashMap<String, V>` with
/// convenient methods for common access patterns. These methods reduce boilerplate
/// code when working with string-keyed maps throughout the codebase.
///
/// # Design Goals
///
/// - **Ergonomics**: Reduce common boilerplate patterns
/// - **Flexibility**: Support both standard HashMap and FxHashMap
/// - **Performance**: Minimize allocations and clones where possible
/// - **Safety**: Provide safe alternatives to unwrap-heavy code
///
/// # Examples
///
/// ```rust
/// use weavegraph::utils::collections::StringMapExt;
/// use rustc_hash::FxHashMap;
///
/// let mut config: FxHashMap<String, i32> = FxHashMap::default();
/// config.insert("max_connections".to_string(), 100);
///
/// // Get with default - no panic if key missing
/// let connections = config.get_or_default("max_connections", 50); // Returns 100
/// let timeout = config.get_or_default("timeout", 30); // Returns 30 (default)
///
/// // Insert or update pattern
/// config.insert_or_update(
///     "retry_count".to_string(),
///     1,                    // Initial value if key doesn't exist
///     |count| *count += 1,  // Update function if key exists
/// );
/// ```
pub trait StringMapExt<V> {
    /// Get a value with a default if the key doesn't exist.
    ///
    /// This method provides a safe way to retrieve values from a map with
    /// a fallback default, avoiding the need for `unwrap()` or complex
    /// match statements.
    ///
    /// # Parameters
    /// * `key` - Key to look up in the map
    /// * `default` - Default value to return if key is not found
    ///
    /// # Returns
    /// The value associated with the key, or the default if key doesn't exist
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::utils::collections::StringMapExt;
    /// use std::collections::HashMap;
    ///
    /// let mut settings = HashMap::new();
    /// settings.insert("debug".to_string(), true);
    ///
    /// assert_eq!(settings.get_or_default("debug", false), true);
    /// assert_eq!(settings.get_or_default("verbose", false), false);
    /// ```
    fn get_or_default(&self, key: &str, default: V) -> V
    where
        V: Clone;

    /// Insert if the key doesn't exist, otherwise update with a function.
    ///
    /// This method implements the common "insert or update" pattern efficiently.
    /// If the key exists, the update function is called with a mutable reference
    /// to the existing value. If the key doesn't exist, the provided value is inserted.
    ///
    /// This is particularly useful for counters, accumulators, and other scenarios
    /// where you need to modify existing values or insert new ones.
    ///
    /// # Parameters
    /// * `key` - Key to insert or update
    /// * `value` - Value to insert if key doesn't exist
    /// * `update_fn` - Function to call with existing value if key exists
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::utils::collections::StringMapExt;
    /// use rustc_hash::FxHashMap;
    ///
    /// let mut counters = FxHashMap::default();
    ///
    /// // First call inserts the initial value
    /// counters.insert_or_update(
    ///     "page_views".to_string(),
    ///     1,
    ///     |count| *count += 1,
    /// );
    /// assert_eq!(counters["page_views"], 1);
    ///
    /// // Subsequent calls update the existing value
    /// counters.insert_or_update(
    ///     "page_views".to_string(),
    ///     1,
    ///     |count| *count += 1,
    /// );
    /// assert_eq!(counters["page_views"], 2);
    /// ```
    fn insert_or_update<F>(&mut self, key: String, value: V, update_fn: F)
    where
        F: FnOnce(&mut V);
}

impl<V> StringMapExt<V> for HashMap<String, V> {
    /// Gets a value from the HashMap with a default fallback.
    ///
    /// This implementation uses the standard HashMap's `get` method with `cloned()`
    /// to return an owned value, falling back to the provided default if the key
    /// is not found. The standard HashMap uses SipHash for better security against
    /// hash collision attacks.
    fn get_or_default(&self, key: &str, default: V) -> V
    where
        V: Clone,
    {
        self.get(key).cloned().unwrap_or(default)
    }

    /// Inserts a new value or updates an existing one using the entry API.
    ///
    /// This implementation leverages HashMap's entry API for efficient
    /// insert-or-update operations. If the key exists, `and_modify` calls the
    /// update function. If not, `or_insert` adds the initial value.
    /// This avoids double-lookup that would occur with separate contains/get/insert calls.
    fn insert_or_update<F>(&mut self, key: String, value: V, update_fn: F)
    where
        F: FnOnce(&mut V),
    {
        self.entry(key).and_modify(update_fn).or_insert(value);
    }
}

impl<V> StringMapExt<V> for FxHashMap<String, V> {
    /// Gets a value from the FxHashMap with a default fallback.
    ///
    /// This implementation uses FxHashMap's `get` method with `cloned()`
    /// to return an owned value, falling back to the provided default if the key
    /// is not found. FxHashMap uses a faster hash function (FxHash) that's suitable
    /// for trusted input and provides better performance than the standard HashMap.
    fn get_or_default(&self, key: &str, default: V) -> V
    where
        V: Clone,
    {
        self.get(key).cloned().unwrap_or(default)
    }

    /// Inserts a new value or updates an existing one using the entry API.
    ///
    /// This implementation leverages FxHashMap's entry API for efficient
    /// insert-or-update operations. The behavior is identical to HashMap's
    /// implementation but benefits from FxHashMap's faster hashing for string keys.
    /// This is particularly beneficial in the Weavegraph framework where string keys are common.
    fn insert_or_update<F>(&mut self, key: String, value: V, update_fn: F)
    where
        F: FnOnce(&mut V),
    {
        self.entry(key).and_modify(update_fn).or_insert(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_new_extra_map() {
        let map = new_extra_map();
        assert!(map.is_empty());
    }

    #[test]
    fn test_extra_map_from_pairs() {
        let map = extra_map_from_pairs([
            ("name", json!("test")),
            ("count", json!(42)),
            ("enabled", json!(true)),
        ]);

        assert_eq!(map.len(), 3);
        assert_eq!(map["name"], json!("test"));
        assert_eq!(map["count"], json!(42));
        assert_eq!(map["enabled"], json!(true));
    }

    #[test]
    fn test_merge_extra_maps() {
        let mut map1 = new_extra_map();
        map1.insert("a".into(), json!(1));
        map1.insert("b".into(), json!(2));

        let mut map2 = new_extra_map();
        map2.insert("b".into(), json!(3));
        map2.insert("c".into(), json!(4));

        let merged = merge_extra_maps([&map1, &map2]);
        assert_eq!(merged["a"], json!(1));
        assert_eq!(merged["b"], json!(3)); // Last wins
        assert_eq!(merged["c"], json!(4));
    }

    #[test]
    fn test_extra_map_ext() {
        let mut map = new_extra_map();
        map.insert_string("name", "test");
        map.insert_number("count", 42);
        map.insert_bool("enabled", true);

        assert_eq!(map.len(), 3);
        assert_eq!(map.get_string("name").unwrap(), "test");
        assert_eq!(map.get_number("count").unwrap(), 42.into());
        assert!(map.get_bool("enabled").unwrap());
    }

    #[test]
    fn test_extra_map_ext_errors() {
        let mut map = new_extra_map();
        map.insert_string("name", "test");

        // Test missing key
        assert!(matches!(
            map.get_string("missing"),
            Err(CollectionError::MissingKey { .. })
        ));

        // Test type mismatch
        assert!(matches!(
            map.get_number("name"),
            Err(CollectionError::TypeMismatch { .. })
        ));
    }

    #[test]
    fn test_has_typed() {
        let mut map = new_extra_map();
        map.insert_string("name", "test");
        map.insert_number("count", 42);
        map.insert_bool("enabled", true);

        assert!(map.has_typed("name", "string"));
        assert!(map.has_typed("count", "number"));
        assert!(map.has_typed("enabled", "bool"));
        assert!(!map.has_typed("name", "number"));
        assert!(!map.has_typed("missing", "string"));
    }

    #[test]
    fn test_string_map_ext() {
        let mut map: FxHashMap<String, i32> = FxHashMap::default();
        map.insert("existing".into(), 10);

        assert_eq!(map.get_or_default("existing", 0), 10);
        assert_eq!(map.get_or_default("missing", 42), 42);

        map.insert_or_update("new".into(), 5, |v| *v += 1);
        assert_eq!(map["new"], 5);

        map.insert_or_update("new".into(), 999, |v| *v += 1);
        assert_eq!(map["new"], 6); // Updated, not replaced
    }
}
