//! ID generation utilities for run, step, node, and session identifiers.
//!
//! Provides utilities for generating unique, deterministic, and contextual IDs
//! throughout the Weavegraph framework. Supports both random UUID-based generation
//! and deterministic seeded generation for testing and reproducibility.

use miette::{Diagnostic, Result};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;
use uuid::Uuid;

/// Errors that can occur during ID generation.
#[derive(Debug, Error, Diagnostic)]
pub enum IdError {
    /// Invalid format for ID parsing or validation.
    #[error("Invalid ID format: {format}")]
    #[diagnostic(code(weavegraph::id::invalid_format))]
    InvalidFormat { format: String },

    /// ID generation failed due to system constraints.
    #[error("ID generation failed: {reason}")]
    #[diagnostic(code(weavegraph::id::generation_failed))]
    GenerationFailed { reason: String },
}

/// Configuration for ID generation behavior.
#[derive(Debug, Clone, Default)]
pub struct IdConfig {
    /// Random seed for deterministic ID generation (optional).
    pub seed: Option<u64>,
    /// Prefix to use for all generated IDs.
    pub prefix: Option<String>,
    /// Whether to include timestamps in IDs.
    pub include_timestamp: bool,
    /// Counter for sequential ID generation.
    pub use_counter: bool,
}

impl fmt::Display for IdConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "IdConfig {{ seed: {:?}, prefix: {:?}, timestamp: {}, counter: {} }}",
            self.seed, self.prefix, self.include_timestamp, self.use_counter
        )
    }
}

/// High-performance ID generator with configurable behavior.
///
/// Supports multiple ID generation strategies including UUID-based random IDs,
/// deterministic seeded IDs, and sequential counter-based IDs. Thread-safe
/// and optimized for high-throughput scenarios.
///
/// # Examples
///
/// ```rust
/// use weavegraph::utils::id_generator::{IdGenerator, IdConfig};
///
/// // Default random ID generation
/// let generator = IdGenerator::new();
/// let id = generator.generate_run_id();
/// assert!(id.starts_with("run-"));
///
/// // Deterministic generation for testing
/// let config = IdConfig {
///     seed: Some(12345),
///     prefix: Some("test".into()),
///     ..Default::default()
/// };
/// let det_generator = IdGenerator::with_config(config);
/// let det_id = det_generator.generate_id();
/// ```
#[derive(Debug)]
pub struct IdGenerator {
    config: IdConfig,
    counter: AtomicU64,
}

impl IdGenerator {
    /// Create a new ID generator with default configuration.
    ///
    /// Uses random UUID generation without prefixes or deterministic behavior.
    ///
    /// # Returns
    /// A new IdGenerator instance with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: IdConfig::default(),
            counter: AtomicU64::new(0),
        }
    }

    /// Create an ID generator with custom configuration.
    ///
    /// # Parameters
    /// * `config` - Configuration specifying ID generation behavior
    ///
    /// # Returns
    /// A new IdGenerator instance with the specified configuration.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::utils::id_generator::{IdGenerator, IdConfig};
    ///
    /// let config = IdConfig {
    ///     seed: Some(42),
    ///     prefix: Some("weavegraph".into()),
    ///     include_timestamp: true,
    ///     use_counter: false,
    /// };
    /// let generator = IdGenerator::with_config(config);
    /// ```
    #[must_use]
    pub fn with_config(config: IdConfig) -> Self {
        Self {
            config,
            counter: AtomicU64::new(0),
        }
    }

    /// Generate a generic ID using the configured strategy.
    ///
    /// This is the core ID generation method that respects all configuration
    /// options including seeds, prefixes, timestamps, and counters.
    ///
    /// # Returns
    /// A newly generated ID string.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::utils::id_generator::IdGenerator;
    ///
    /// let generator = IdGenerator::new();
    /// let id = generator.generate_id();
    /// assert!(!id.is_empty());
    /// ```
    #[must_use]
    pub fn generate_id(&self) -> String {
        let base_id = if let Some(seed) = self.config.seed {
            if self.config.use_counter {
                let counter = self.counter.fetch_add(1, Ordering::Relaxed);
                format!("seeded-{}-{}", seed, counter)
            } else {
                format!("seeded-{}", seed)
            }
        } else if self.config.use_counter {
            let counter = self.counter.fetch_add(1, Ordering::Relaxed);
            format!("counter-{}", counter)
        } else {
            self.generate_uuid()
        };

        let mut final_id = base_id;

        if self.config.include_timestamp {
            let timestamp = chrono::Utc::now().timestamp();
            final_id = format!("{}-t{}", final_id, timestamp);
        }

        if let Some(prefix) = &self.config.prefix {
            final_id = format!("{}-{}", prefix, final_id);
        }

        final_id
    }

    /// Generate a UUID v4 as a string.
    ///
    /// Provides direct access to UUID generation regardless of configuration.
    ///
    /// # Returns
    /// A new UUID v4 formatted as a string.
    #[must_use]
    pub fn generate_uuid(&self) -> String {
        Uuid::new_v4().to_string()
    }

    /// Generate an ID with a specific prefix.
    ///
    /// This method combines the configured generation strategy with a
    /// custom prefix, useful for creating typed IDs.
    ///
    /// # Parameters
    /// * `prefix` - Prefix to prepend to the generated ID
    ///
    /// # Returns
    /// A new ID with the specified prefix.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::utils::id_generator::IdGenerator;
    ///
    /// let generator = IdGenerator::new();
    /// let session_id = generator.generate_id_with_prefix("session");
    /// assert!(session_id.starts_with("session-"));
    /// ```
    #[must_use]
    pub fn generate_id_with_prefix(&self, prefix: &str) -> String {
        format!("{}-{}", prefix, self.generate_base_id())
    }

    /// Generate a run ID for workflow execution tracking.
    ///
    /// Run IDs are used to track complete workflow executions from start to finish.
    ///
    /// # Returns
    /// A new run ID with "run" prefix.
    #[must_use]
    pub fn generate_run_id(&self) -> String {
        self.generate_id_with_prefix("run")
    }

    /// Generate a step ID for individual workflow steps.
    ///
    /// Step IDs track individual execution steps within a workflow run.
    ///
    /// # Returns
    /// A new step ID with "step" prefix.
    #[must_use]
    pub fn generate_step_id(&self) -> String {
        self.generate_id_with_prefix("step")
    }

    /// Generate a node ID for graph node instances.
    ///
    /// Node IDs identify specific instances of nodes in the execution graph.
    ///
    /// # Returns
    /// A new node ID with "node" prefix.
    #[must_use]
    pub fn generate_node_id(&self) -> String {
        self.generate_id_with_prefix("node")
    }

    /// Generate a session ID for checkpoint and persistence tracking.
    ///
    /// Session IDs are used for checkpoint management and state persistence.
    ///
    /// # Returns
    /// A new session ID with "session" prefix.
    #[must_use]
    pub fn generate_session_id(&self) -> String {
        self.generate_id_with_prefix("session")
    }

    /// Generate a random ID without any configuration-based modifications.
    ///
    /// This bypasses all configuration and always generates a random UUID.
    ///
    /// # Returns
    /// A random UUID string.
    #[must_use]
    pub fn generate_random_id(&self) -> String {
        self.generate_uuid()
    }

    /// Parse and validate an ID format.
    ///
    /// Checks if an ID follows expected patterns and extracts components.
    ///
    /// # Parameters
    /// * `id` - ID string to validate
    ///
    /// # Returns
    /// Ok(ParsedId) if valid, Err(IdError) if invalid.
    pub fn parse_id(&self, id: &str) -> Result<ParsedId, IdError> {
        if id.is_empty() {
            return Err(IdError::InvalidFormat {
                format: "empty string".into(),
            });
        }

        let parts: Vec<&str> = id.split('-').collect();
        if parts.len() < 2 {
            return Err(IdError::InvalidFormat {
                format: "missing separator".into(),
            });
        }

        Ok(ParsedId {
            prefix: parts[0].to_string(),
            base: parts[1..].join("-"),
            original: id.to_string(),
        })
    }

    /// Get the current counter value.
    ///
    /// Useful for testing and debugging sequential ID generation.
    ///
    /// # Returns
    /// Current counter value.
    #[must_use]
    pub fn current_counter(&self) -> u64 {
        self.counter.load(Ordering::Relaxed)
    }

    /// Reset the counter to zero.
    ///
    /// Useful for testing scenarios that require predictable counter values.
    pub fn reset_counter(&self) {
        self.counter.store(0, Ordering::Relaxed);
    }

    /// Private helper method for base ID generation
    fn generate_base_id(&self) -> String {
        if let Some(seed) = self.config.seed {
            if self.config.use_counter {
                let counter = self.counter.fetch_add(1, Ordering::Relaxed);
                format!("seeded-{}-{}", seed, counter)
            } else {
                format!("seeded-{}", seed)
            }
        } else if self.config.use_counter {
            let counter = self.counter.fetch_add(1, Ordering::Relaxed);
            format!("counter-{}", counter)
        } else {
            self.generate_uuid()
        }
    }
}

impl Default for IdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// Parsed ID components for analysis and validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedId {
    /// The prefix part of the ID (e.g., "run", "step", "node").
    pub prefix: String,
    /// The base identifier part after the prefix.
    pub base: String,
    /// The original complete ID string.
    pub original: String,
}

impl ParsedId {
    /// Check if this ID has a specific prefix.
    ///
    /// # Parameters
    /// * `expected_prefix` - Prefix to check for
    ///
    /// # Returns
    /// True if the ID has the expected prefix.
    #[must_use]
    pub fn has_prefix(&self, expected_prefix: &str) -> bool {
        self.prefix == expected_prefix
    }

    /// Extract timestamp from the ID if present.
    ///
    /// # Returns
    /// Timestamp if found and valid, None otherwise.
    #[must_use]
    pub fn extract_timestamp(&self) -> Option<i64> {
        if let Some(timestamp_part) = self.base.split('-').find(|part| part.starts_with('t')) {
            timestamp_part[1..].parse().ok()
        } else {
            None
        }
    }
}

/// Utility functions for common ID operations.
pub mod id_utils {
    use super::*;

    /// Create a deterministic ID generator for testing.
    ///
    /// # Parameters
    /// * `seed` - Seed value for deterministic generation
    ///
    /// # Returns
    /// IdGenerator configured for deterministic behavior.
    #[must_use]
    pub fn create_test_generator(seed: u64) -> IdGenerator {
        IdGenerator::with_config(IdConfig {
            seed: Some(seed),
            use_counter: true,
            ..Default::default()
        })
    }

    /// Create a production ID generator with timestamps.
    ///
    /// # Returns
    /// IdGenerator configured for production use with timestamps.
    #[must_use]
    pub fn create_production_generator() -> IdGenerator {
        IdGenerator::with_config(IdConfig {
            include_timestamp: true,
            ..Default::default()
        })
    }

    /// Validate that an ID follows expected patterns.
    ///
    /// # Parameters
    /// * `id` - ID to validate
    /// * `expected_prefix` - Expected prefix (optional)
    ///
    /// # Returns
    /// True if the ID is valid.
    pub fn is_valid_id(id: &str, expected_prefix: Option<&str>) -> bool {
        if id.is_empty() {
            return false;
        }

        let parts: Vec<&str> = id.split('-').collect();
        if parts.len() < 2 {
            return false;
        }

        if let Some(prefix) = expected_prefix {
            parts[0] == prefix
        } else {
            true
        }
    }

    /// Extract the type of an ID from its prefix.
    ///
    /// # Parameters
    /// * `id` - ID to analyze
    ///
    /// # Returns
    /// ID type based on prefix, or "unknown" if unrecognized.
    #[must_use]
    pub fn get_id_type(id: &str) -> String {
        id.split('-')
            .next()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_generator() {
        let generator = IdGenerator::new();
        let id = generator.generate_id();
        assert!(!id.is_empty());

        let run_id = generator.generate_run_id();
        assert!(run_id.starts_with("run-"));

        let step_id = generator.generate_step_id();
        assert!(step_id.starts_with("step-"));
    }

    #[test]
    fn test_deterministic_generator() {
        let config = IdConfig {
            seed: Some(42),
            use_counter: true,
            ..Default::default()
        };
        let generator = IdGenerator::with_config(config);

        let id1 = generator.generate_id();
        let id2 = generator.generate_id();

        assert!(id1.contains("seeded-42-0"));
        assert!(id2.contains("seeded-42-1"));
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_prefixed_generator() {
        let config = IdConfig {
            prefix: Some("test".into()),
            ..Default::default()
        };
        let generator = IdGenerator::with_config(config);

        let id = generator.generate_id();
        assert!(id.starts_with("test-"));
    }

    #[test]
    fn test_timestamp_generator() {
        let config = IdConfig {
            include_timestamp: true,
            ..Default::default()
        };
        let generator = IdGenerator::with_config(config);

        let id = generator.generate_id();
        assert!(id.contains("-t"));
    }

    #[test]
    fn test_id_parsing() {
        let generator = IdGenerator::new();
        let parsed = generator.parse_id("run-abc123").unwrap();

        assert_eq!(parsed.prefix, "run");
        assert_eq!(parsed.base, "abc123");
        assert_eq!(parsed.original, "run-abc123");
        assert!(parsed.has_prefix("run"));
        assert!(!parsed.has_prefix("step"));
    }

    #[test]
    fn test_id_validation() {
        assert!(id_utils::is_valid_id("run-abc123", Some("run")));
        assert!(!id_utils::is_valid_id("step-abc123", Some("run")));
        assert!(!id_utils::is_valid_id("invalid", None));
        assert!(!id_utils::is_valid_id("", None));

        assert_eq!(id_utils::get_id_type("run-abc123"), "run");
        assert_eq!(id_utils::get_id_type("invalid"), "invalid");
    }

    #[test]
    fn test_counter_operations() {
        let generator = IdGenerator::new();
        assert_eq!(generator.current_counter(), 0);

        generator.reset_counter();
        assert_eq!(generator.current_counter(), 0);
    }

    #[test]
    fn test_utility_functions() {
        let test_gen = id_utils::create_test_generator(123);
        let test_id = test_gen.generate_id();
        assert!(test_id.contains("seeded-123"));

        let prod_gen = id_utils::create_production_generator();
        let prod_id = prod_gen.generate_id();
        assert!(prod_id.contains("-t"));
    }
}
