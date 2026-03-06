/*!
Helper utilities for SQLite checkpointer operations.

This module provides consistent JSON serialization/deserialization helpers
that standardize error handling and context reporting across the SQLite
checkpointer implementation. All functions convert serde errors to appropriate
`CheckpointerError` variants with contextual information.

## Usage

These helpers should be used for all JSON operations in the SQLite checkpointer
to ensure consistent error handling and debugging information.

```rust,ignore
// Serialization
let json_str = serialize_json(&data, "user_data")?;

// Deserialization
let data: MyType = deserialize_json(&json_str, "user_data")?;

// Value deserialization
let data: MyType = deserialize_json_value(json_value, "user_data")?;

// Field validation
let required_field = require_json_field(optional_field, "state_json")?;
```
*/

use crate::runtimes::checkpointer::CheckpointerError;
use crate::utils::json_ext::{deserialize_with_context, serialize_with_context};
use serde_json::Value;

/// Helper for JSON serialization with consistent error formatting.
pub(super) fn serialize_json<T: serde::Serialize>(
    value: &T,
    context: &'static str,
) -> Result<String, CheckpointerError> {
    serialize_with_context(value, context, |e, ctx| CheckpointerError::Other {
        message: format!("{ctx} serialize: {e}"),
    })
}

/// Helper for JSON deserialization with consistent error formatting.
pub(super) fn deserialize_json<T: serde::de::DeserializeOwned>(
    json: &str,
    context: &'static str,
) -> Result<T, CheckpointerError> {
    deserialize_with_context(json, context, |e, ctx| CheckpointerError::Other {
        message: format!("{ctx} parse: {e}"),
    })
}

/// Helper for JSON value deserialization with consistent error formatting.
pub(super) fn deserialize_json_value<T: serde::de::DeserializeOwned>(
    value: Value,
    context: &'static str,
) -> Result<T, CheckpointerError> {
    serde_json::from_value(value).map_err(|e| CheckpointerError::Other {
        message: format!("{context} parse (serde): {e}"),
    })
}

/// Helper for extracting required JSON fields with consistent error formatting.
pub(super) fn require_json_field(
    field: Option<String>,
    field_name: &'static str,
) -> Result<String, CheckpointerError> {
    field.ok_or_else(|| CheckpointerError::Other {
        message: format!("missing {field_name} for persisted checkpoint"),
    })
}
