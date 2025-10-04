/*!
Type-safe wrappers for runtime execution identifiers and values.

This module provides newtype patterns to improve type safety and prevent
common errors like passing session IDs where step numbers are expected.
These types are specific to the runtime execution layer.

For core workflow types (node kinds, channel types), see [`crate::types`].

# Design Philosophy

Runtime types are **execution infrastructure** - they manage how workflows
are executed, tracked, and persisted, but don't define what workflows *are*.

- **Type Safety**: Prevent mixing up session IDs, step numbers, etc.
- **Domain Modeling**: Make execution concepts explicit in the type system
- **Evolution**: Allow runtime infrastructure to evolve independently

# Examples

```rust
use weavegraph::runtimes::types::{SessionId, StepNumber};

// Type-safe session management
let session = SessionId::generate();
let step = StepNumber::zero();

// The compiler prevents mixing these up
// process_session(step);  // ✗ Compile error!
// process_step(session);  // ✗ Compile error!
```
*/

use serde::{Deserialize, Serialize};
use std::fmt;

/// Type-safe wrapper for session identifiers.
///
/// This prevents accidentally passing arbitrary strings where session IDs
/// are expected and provides utilities for generating valid session IDs.
///
/// # Examples
///
/// ```rust
/// use weavegraph::runtimes::types::SessionId;
///
/// let session_id = SessionId::new("my_session");
/// let generated_id = SessionId::generate();
///
/// // Type safety - can't accidentally pass a string
/// // process_session(session_id); // ✓ OK
/// // process_session("my_session"); // ✗ Compile error
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(String);

impl SessionId {
    /// Create a new session ID from a string.
    ///
    /// # Parameters
    ///
    /// * `id` - The session identifier string
    ///
    /// # Returns
    ///
    /// A type-safe `SessionId` wrapper
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Generate a new unique session ID.
    ///
    /// Uses the ID generator utilities to create a unique identifier
    /// suitable for session tracking.
    ///
    /// # Returns
    ///
    /// A newly generated unique `SessionId`
    #[must_use]
    pub fn generate() -> Self {
        use crate::utils::id_generator::IdGenerator;
        Self(IdGenerator::new().generate_session_id())
    }

    /// Get the inner string value.
    ///
    /// # Returns
    ///
    /// A reference to the underlying string
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Convert into the inner string value.
    ///
    /// # Returns
    ///
    /// The underlying string
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for SessionId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for SessionId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl AsRef<str> for SessionId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Type-safe wrapper for workflow step numbers.
///
/// Prevents confusion between step numbers and other numeric values,
/// and provides utilities for step arithmetic.
///
/// # Examples
///
/// ```rust
/// use weavegraph::runtimes::types::StepNumber;
///
/// let step = StepNumber::new(5);
/// let next_step = step.next();
/// assert_eq!(next_step.value(), 6);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct StepNumber(u64);

impl StepNumber {
    /// Create a new step number.
    ///
    /// # Parameters
    ///
    /// * `step` - The step number value
    ///
    /// # Returns
    ///
    /// A type-safe `StepNumber` wrapper
    #[must_use]
    pub fn new(step: u64) -> Self {
        Self(step)
    }

    /// Create step number zero (initial step).
    ///
    /// # Returns
    ///
    /// A `StepNumber` representing step 0
    #[must_use]
    pub fn zero() -> Self {
        Self(0)
    }

    /// Get the numeric value of this step.
    ///
    /// # Returns
    ///
    /// The underlying u64 value
    #[must_use]
    pub fn value(self) -> u64 {
        self.0
    }

    /// Get the next step number.
    ///
    /// # Returns
    ///
    /// A new `StepNumber` incremented by 1
    #[must_use]
    pub fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }

    /// Check if this is the initial step (step 0).
    ///
    /// # Returns
    ///
    /// `true` if this is step 0
    #[must_use]
    pub fn is_initial(self) -> bool {
        self.0 == 0
    }
}

impl fmt::Display for StepNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for StepNumber {
    fn from(step: u64) -> Self {
        Self(step)
    }
}

impl From<StepNumber> for u64 {
    fn from(step: StepNumber) -> u64 {
        step.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_id_creation() {
        let id = SessionId::new("test_session");
        assert_eq!(id.as_str(), "test_session");
        assert_eq!(id.to_string(), "test_session");
    }

    #[test]
    fn test_session_id_generation() {
        let id1 = SessionId::generate();
        let id2 = SessionId::generate();
        // Generated IDs should be different
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_step_number_arithmetic() {
        let step = StepNumber::new(5);
        assert_eq!(step.value(), 5);
        assert_eq!(step.next().value(), 6);
        assert!(!step.is_initial());

        let initial = StepNumber::zero();
        assert!(initial.is_initial());
        assert_eq!(initial.value(), 0);
    }

    #[test]
    fn test_step_number_saturation() {
        let max_step = StepNumber::new(u64::MAX);
        let next = max_step.next();
        assert_eq!(next.value(), u64::MAX); // Should saturate, not overflow
    }
}
