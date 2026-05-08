//! State management for the Weavegraph workflow framework.
//!
//! This module provides versioned state management with multiple channels
//! for different types of workflow data. State is managed through versioned
//! channels that support snapshotting, deep cloning, and persistence.
//!
//! # Core Types
//!
//! - [`VersionedState`]: The main state container with versioned channels
//! - [`StateSnapshot`]: Immutable snapshot of state at a point in time
//!
//! # Channels
//!
//! State is organized into three main channels:
//! - **Messages**: Conversation messages and chat data
//! - **Extra**: Custom metadata and intermediate results
//! - **Errors**: Error events and diagnostic information
//!
//! # Examples
//!
//! ```rust
//! use weavegraph::state::VersionedState;
//! use weavegraph::channels::Channel;
//! use serde_json::json;
//!
//! // Create initial state with user message
//! let mut state = VersionedState::new_with_user_message("Hello, world!");
//!
//! // Add some metadata
//! state.extra.get_mut().insert("user_id".to_string(), json!("user123"));
//!
//! // Take snapshot for processing
//! let snapshot = state.snapshot();
//! assert_eq!(snapshot.messages.len(), 1);
//! assert_eq!(snapshot.extra.get("user_id"), Some(&json!("user123")));
//! ```

use rustc_hash::FxHashMap;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::marker::PhantomData;
use thiserror::Error;

use crate::{
    channels::{Channel, ErrorsChannel, ExtrasChannel, MessagesChannel},
    message::{Message, Role},
};

/// A schema-versioned key for typed values stored in [`VersionedState::extra`].
///
/// `StateKey` is a thin helper over the JSON-compatible `extra` map. Domain
/// crates can define constants and use them from nodes, reducers, tests, and
/// replay code without repeating string literals.
///
/// # Examples
///
/// ```rust
/// use serde::{Deserialize, Serialize};
/// use weavegraph::state::StateKey;
///
/// #[derive(Serialize, Deserialize)]
/// struct PortfolioSnapshot {
///     cash: i64,
/// }
///
/// const PORTFOLIO: StateKey<PortfolioSnapshot> =
///     StateKey::new("wq", "portfolio_snapshot", 1);
///
/// assert_eq!(PORTFOLIO.storage_key(), "wq:portfolio_snapshot:v1");
/// ```
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct StateKey<T> {
    namespace: &'static str,
    name: &'static str,
    schema_version: u32,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Clone for StateKey<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for StateKey<T> {}

impl<T> StateKey<T> {
    /// Create a typed state key.
    pub const fn new(namespace: &'static str, name: &'static str, schema_version: u32) -> Self {
        Self {
            namespace,
            name,
            schema_version,
            _marker: PhantomData,
        }
    }

    /// Return the namespace component.
    #[must_use]
    pub fn namespace(&self) -> &'static str {
        self.namespace
    }

    /// Return the key name component.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Return the schema version component.
    #[must_use]
    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Return the concrete `extra` map key used for storage.
    ///
    /// The format is `namespace:name:v{schema_version}`. Changing the schema
    /// version intentionally writes to a different slot, avoiding silent
    /// collisions between incompatible payload shapes.
    #[must_use]
    pub fn storage_key(&self) -> String {
        format!("{}:{}:v{}", self.namespace, self.name, self.schema_version)
    }
}

/// Errors produced by typed state-slot helpers.
#[derive(Debug, Error)]
#[cfg_attr(feature = "diagnostics", derive(miette::Diagnostic))]
pub enum StateSlotError {
    /// The requested typed slot was not present in the state.
    #[error("state slot not found: {key}")]
    #[cfg_attr(
        feature = "diagnostics",
        diagnostic(code(weavegraph::state::slot_missing))
    )]
    Missing {
        /// The concrete storage key that was not found.
        key: String,
    },

    /// A typed slot value could not be serialized to JSON.
    #[error("failed to serialize state slot {key}: {source}")]
    #[cfg_attr(
        feature = "diagnostics",
        diagnostic(code(weavegraph::state::slot_serialize))
    )]
    Serialize {
        /// The concrete storage key being written.
        key: String,
        /// The underlying serde error.
        #[source]
        source: serde_json::Error,
    },

    /// A typed slot value could not be deserialized from JSON.
    #[error("failed to deserialize state slot {key}: {source}")]
    #[cfg_attr(
        feature = "diagnostics",
        diagnostic(code(weavegraph::state::slot_deserialize))
    )]
    Deserialize {
        /// The concrete storage key being read.
        key: String,
        /// The underlying serde error.
        #[source]
        source: serde_json::Error,
    },
}

/// The main state container for workflow execution.
///
/// `VersionedState` manages three independent channels of versioned data:
/// messages, custom extras, and error events. Each channel maintains its own
/// version number for optimistic concurrency control and change detection.
///
/// # Channels
///
/// - **messages**: Chat messages and conversation data ([`MessagesChannel`])
/// - **extra**: Custom metadata and intermediate results ([`ExtrasChannel`])
/// - **errors**: Error events and diagnostics ([`ErrorsChannel`])
///
/// # Examples
///
/// ```rust
/// use weavegraph::state::VersionedState;
/// use weavegraph::message::{Message, Role};
/// use weavegraph::channels::Channel;
/// use serde_json::json;
///
/// // Initialize with user message
/// let mut state = VersionedState::new_with_user_message("Process this data");
///
/// // Add metadata
/// state.extra.get_mut().insert("session_id".to_string(), json!("sess_123"));
/// state.extra.get_mut().insert("priority".to_string(), json!("high"));
///
/// // Add assistant response
/// state
///     .messages
///     .get_mut()
///     .push(Message::with_role(Role::Assistant, "Processing your request..."));
///
/// // Take snapshot
/// let snapshot = state.snapshot();
/// assert_eq!(snapshot.messages.len(), 2);
/// assert_eq!(snapshot.extra.len(), 2);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionedState {
    /// Message channel containing conversation data
    pub messages: MessagesChannel,
    /// Extra channel for custom metadata and intermediate results
    pub extra: ExtrasChannel,
    /// Error channel for diagnostic information
    pub errors: ErrorsChannel,
}

/// Immutable snapshot of workflow state at a specific point in time.
///
/// `StateSnapshot` provides a read-only view of the state that nodes can
/// safely access during execution without affecting the underlying state.
/// It contains cloned data from the messages and extra channels along with
/// their version numbers.
///
/// # Fields
///
/// - `messages`: Cloned message data at snapshot time
/// - `messages_version`: Version of messages channel when snapshot was taken
/// - `extra`: Cloned extra data at snapshot time
/// - `extra_version`: Version of extra channel when snapshot was taken
/// - `errors`: Cloned error events at snapshot time
/// - `errors_version`: Version of errors channel when snapshot was taken
///
/// # Usage
///
/// Snapshots are automatically created by [`VersionedState::snapshot()`] and
/// passed to nodes during workflow execution. Nodes should treat snapshots
/// as immutable input data.
///
/// # Examples
///
/// ```rust
/// use weavegraph::state::VersionedState;
/// use weavegraph::channels::Channel;
/// use serde_json::json;
///
/// let mut state = VersionedState::new_with_user_message("Hello");
/// state.extra.get_mut().insert("key".to_string(), json!("value"));
///
/// let snapshot = state.snapshot();
///
/// // Snapshot is independent of original state
/// state.extra.get_mut().clear();
/// assert_eq!(snapshot.extra.get("key"), Some(&json!("value")));
/// assert!(state.extra.snapshot().is_empty());
/// ```
#[derive(Clone, Debug)]
pub struct StateSnapshot {
    /// Messages at the time of snapshot
    pub messages: Vec<Message>,
    /// Version of messages channel when snapshot was taken
    pub messages_version: u32,
    /// Extra data at the time of snapshot
    pub extra: FxHashMap<String, Value>,
    /// Version of extra channel when snapshot was taken
    pub extra_version: u32,
    /// Error events at the time of snapshot
    pub errors: Vec<crate::channels::errors::ErrorEvent>,
    /// Version of errors channel when snapshot was taken
    pub errors_version: u32,
}

impl VersionedState {
    /// Creates a new versioned state initialized with a user message.
    ///
    /// This is the primary constructor for starting workflow execution.
    /// text as the first user message.
    ///
    /// # Parameters
    ///
    /// - `user_text`: The initial user message content
    ///
    /// # Returns
    ///
    /// A new `VersionedState` with:
    /// - One user message in the messages channel
    /// - Empty extra and error channels
    /// - All channels initialized to version 1
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::state::VersionedState;
    ///
    /// let state = VersionedState::new_with_user_message("Analyze this data");
    /// let snapshot = state.snapshot();
    ///
    /// assert_eq!(snapshot.messages.len(), 1);
    /// assert_eq!(snapshot.messages[0].role, weavegraph::message::Role::User);
    /// assert_eq!(snapshot.messages[0].content, "Analyze this data");
    /// assert_eq!(snapshot.messages_version, 1);
    /// assert!(snapshot.extra.is_empty());
    /// ```
    pub fn new_with_user_message(user_text: &str) -> Self {
        let messages = vec![Message::with_role(Role::User, user_text)];
        Self {
            messages: MessagesChannel::new(messages, 1),
            extra: ExtrasChannel::default(),
            errors: ErrorsChannel::default(),
        }
    }

    /// Creates a new versioned state initialized with a vector of messages.
    ///
    /// This constructor is useful for starting a workflow with an existing chat history.
    ///
    /// # Parameters
    ///
    /// - `messages`: The initial messages content
    ///
    /// # Returns
    ///
    /// A new `VersionedState` with:
    /// - Multiple messages in the messages channel
    /// - Empty extra and error channels
    /// - All channels initialized to version 1
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::state::VersionedState;
    /// use weavegraph::message::{Message, Role};
    ///
    /// let messages = vec![
    ///     Message::with_role(Role::User, "Explain error handling in Rust"),
    ///     Message::with_role(
    ///         Role::Assistant,
    ///         "Use Result and the ? operator to propagate errors cleanly.",
    ///     ),
    /// ];
    /// let state = VersionedState::new_with_messages(messages);
    /// let snapshot = state.snapshot();
    ///
    /// assert_eq!(snapshot.messages.len(), 2);
    /// assert_eq!(snapshot.messages_version, 1);
    /// assert!(snapshot.extra.is_empty());
    /// ```
    pub fn new_with_messages(messages: Vec<Message>) -> Self {
        Self {
            messages: MessagesChannel::new(messages, 1),
            extra: ExtrasChannel::default(),
            errors: ErrorsChannel::default(),
        }
    }

    /// Creates a builder for constructing VersionedState with fluent API.
    ///
    /// The builder pattern provides an ergonomic way to construct state
    /// with custom initial data, versions, and multiple messages.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::state::VersionedState;
    /// use weavegraph::channels::Channel;
    /// use serde_json::json;
    ///
    /// let state = VersionedState::builder()
    ///     .with_user_message("Hello, assistant!")
    ///     .with_assistant_message("Hello! How can I help you?")
    ///     .with_extra("session_id", json!("session_123"))
    ///     .with_extra("priority", json!("high"))
    ///     .build();
    ///
    /// let snapshot = state.snapshot();
    /// assert_eq!(snapshot.messages.len(), 2);
    /// assert_eq!(snapshot.extra.len(), 2);
    /// ```
    pub fn builder() -> VersionedStateBuilder {
        VersionedStateBuilder::new()
    }

    /// Convenience method for adding a message to the state.
    ///
    /// This method adds a message with the specified role and content
    /// to the messages channel. The version is not automatically incremented
    /// as that's handled by the barrier system.
    ///
    /// # Parameters
    ///
    /// - `role`: The role of the message sender (e.g., "user", "assistant", "system")
    /// - `content`: The message content
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::state::VersionedState;
    ///
    /// let mut state = VersionedState::new_with_user_message("Initial message");
    /// state.add_message(
    ///     weavegraph::message::Role::Assistant.as_str(),
    ///     "I understand your request.",
    /// );
    ///
    /// let snapshot = state.snapshot();
    /// assert_eq!(snapshot.messages.len(), 2);
    /// assert_eq!(snapshot.messages[1].role, weavegraph::message::Role::Assistant);
    /// ```
    #[must_use = "consider using the returned self for method chaining"]
    pub fn add_message(&mut self, role: &str, content: &str) -> &mut Self {
        self.messages
            .get_mut()
            .push(Message::with_role(Role::from(role), content));
        self
    }

    /// Convenience method for adding metadata to the extra channel.
    ///
    /// This method adds a key-value pair to the extra channel for custom
    /// metadata and intermediate results. The version is not automatically
    /// incremented as that's handled by the barrier system.
    ///
    /// # Parameters
    ///
    /// - `key`: The metadata key
    /// - `value`: The metadata value as a serde_json::Value
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::state::VersionedState;
    /// use weavegraph::channels::Channel;
    /// use serde_json::json;
    ///
    /// let mut state = VersionedState::new_with_user_message("Test");
    /// state.add_extra("user_id", json!("user_123"))
    ///      .add_extra("timestamp", json!(1234567890));
    ///
    /// let snapshot = state.snapshot();
    /// assert_eq!(snapshot.extra.len(), 2);
    /// assert_eq!(snapshot.extra.get("user_id"), Some(&json!("user_123")));
    /// ```
    #[must_use = "consider using the returned self for method chaining"]
    pub fn add_extra(&mut self, key: &str, value: Value) -> &mut Self {
        self.extra.get_mut().insert(key.to_string(), value);
        self
    }

    /// Adds a typed value to the extra channel using a schema-versioned key.
    ///
    /// The value is serialized to JSON and stored under
    /// [`StateKey::storage_key`]. The channel version is still advanced by the
    /// normal barrier system during graph execution.
    pub fn add_typed_extra<T: Serialize>(
        &mut self,
        key: StateKey<T>,
        value: T,
    ) -> Result<&mut Self, StateSlotError> {
        let storage_key = key.storage_key();
        let json_value =
            serde_json::to_value(value).map_err(|source| StateSlotError::Serialize {
                key: storage_key.clone(),
                source,
            })?;
        self.extra.get_mut().insert(storage_key, json_value);
        Ok(self)
    }

    /// Creates an immutable snapshot of the current state.
    ///
    /// This method clones the current channel data and version numbers,
    /// creating a point-in-time view that is safe to access concurrently
    /// while the original state may be modified.
    ///
    /// # Returns
    ///
    /// A [`StateSnapshot`] containing cloned data from messages and extra
    /// channels along with their current version numbers.
    ///
    /// # Performance
    ///
    /// This operation clones all channel data, so it has O(n) complexity
    /// relative to the amount of data in the channels. For large states,
    /// consider whether the snapshot is necessary.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::state::VersionedState;
    /// use weavegraph::channels::Channel;
    /// use serde_json::json;
    ///
    /// let mut state = VersionedState::new_with_user_message("Test");
    /// state.extra.get_mut().insert("status".to_string(), json!("processing"));
    ///
    /// let snapshot = state.snapshot();
    ///
    /// // Snapshot is independent - mutations don't affect it
    /// state.extra.get_mut().insert("status".to_string(), json!("complete"));
    ///
    /// assert_eq!(snapshot.extra.get("status"), Some(&json!("processing")));
    /// assert_eq!(state.extra.snapshot().get("status"), Some(&json!("complete")));
    /// ```
    pub fn snapshot(&self) -> StateSnapshot {
        StateSnapshot {
            messages: self.messages.snapshot(),
            messages_version: self.messages.version(),
            extra: self.extra.snapshot(),
            extra_version: self.extra.version(),
            errors: self.errors.snapshot(),
            errors_version: self.errors.version(),
        }
    }
}

impl StateSnapshot {
    /// Read an optional typed value from the extra channel.
    ///
    /// Returns `Ok(None)` when the slot is absent. Deserialization errors are
    /// reported with the concrete storage key.
    pub fn get_typed<T: DeserializeOwned>(
        &self,
        key: StateKey<T>,
    ) -> Result<Option<T>, StateSlotError> {
        let storage_key = key.storage_key();
        self.extra
            .get(&storage_key)
            .cloned()
            .map(|value| {
                serde_json::from_value(value).map_err(|source| StateSlotError::Deserialize {
                    key: storage_key,
                    source,
                })
            })
            .transpose()
    }

    /// Read a required typed value from the extra channel.
    ///
    /// Use this when a node cannot proceed without a specific typed slot.
    pub fn require_typed<T: DeserializeOwned>(
        &self,
        key: StateKey<T>,
    ) -> Result<T, StateSlotError> {
        let storage_key = key.storage_key();
        self.get_typed(key)?
            .ok_or(StateSlotError::Missing { key: storage_key })
    }
}

/// Builder for constructing VersionedState with fluent API.
///
/// `VersionedStateBuilder` provides an ergonomic way to construct workflow state
/// with custom initial data, multiple messages, and metadata. This is particularly
/// useful when setting up complex initial states for testing or when restoring
/// state from persistence.
///
/// # Examples
///
/// ```rust
/// use weavegraph::state::VersionedState;
/// use weavegraph::channels::Channel;
/// use serde_json::json;
///
/// let state = VersionedState::builder()
///     .with_user_message("What's the weather like?")
///     .with_assistant_message("I'll help you check the weather.")
///     .with_system_message("Weather API access enabled")
///     .with_extra("location", json!("New York"))
///     .with_extra("units", json!("celsius"))
///     .build();
///
/// let snapshot = state.snapshot();
/// assert_eq!(snapshot.messages.len(), 3);
/// assert_eq!(snapshot.extra.len(), 2);
/// ```
#[derive(Debug, Default)]
pub struct VersionedStateBuilder {
    messages: Vec<Message>,
    extra: FxHashMap<String, Value>,
}

impl VersionedStateBuilder {
    /// Creates a new empty builder.
    fn new() -> Self {
        Self::default()
    }

    /// Adds a user message to the builder.
    ///
    /// # Parameters
    ///
    /// - `content`: The user message content
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::state::VersionedState;
    ///
    /// let state = VersionedState::builder()
    ///     .with_user_message("Hello")
    ///     .build();
    /// ```
    pub fn with_user_message(mut self, content: &str) -> Self {
        self.messages.push(Message::with_role(Role::User, content));
        self
    }

    /// Adds an assistant message to the builder.
    ///
    /// # Parameters
    ///
    /// - `content`: The assistant message content
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::state::VersionedState;
    ///
    /// let state = VersionedState::builder()
    ///     .with_user_message("Hello")
    ///     .with_assistant_message("Hi there!")
    ///     .build();
    /// ```
    pub fn with_assistant_message(mut self, content: &str) -> Self {
        self.messages
            .push(Message::with_role(Role::Assistant, content));
        self
    }

    /// Adds a system message to the builder.
    ///
    /// # Parameters
    ///
    /// - `content`: The system message content
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::state::VersionedState;
    ///
    /// let state = VersionedState::builder()
    ///     .with_system_message("Session started")
    ///     .with_user_message("Hello")
    ///     .build();
    /// ```
    pub fn with_system_message(mut self, content: &str) -> Self {
        self.messages
            .push(Message::with_role(Role::System, content));
        self
    }

    /// Adds a custom message with specified role to the builder.
    ///
    /// # Parameters
    ///
    /// - `role`: The message role
    /// - `content`: The message content
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::state::VersionedState;
    ///
    /// let state = VersionedState::builder()
    ///     .with_message("function", "API call result")
    ///     .build();
    /// ```
    pub fn with_message(mut self, role: &str, content: &str) -> Self {
        self.messages
            .push(Message::with_role(Role::from(role), content));
        self
    }

    /// Adds metadata to the extra channel.
    ///
    /// # Parameters
    ///
    /// - `key`: The metadata key
    /// - `value`: The metadata value
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::state::VersionedState;
    /// use serde_json::json;
    ///
    /// let state = VersionedState::builder()
    ///     .with_user_message("Hello")
    ///     .with_extra("session_id", json!("sess_123"))
    ///     .build();
    /// ```
    pub fn with_extra(mut self, key: &str, value: Value) -> Self {
        self.extra.insert(key.to_string(), value);
        self
    }

    /// Adds a typed value to the extra channel using a schema-versioned key.
    pub fn with_typed_extra<T: Serialize>(
        mut self,
        key: StateKey<T>,
        value: T,
    ) -> Result<Self, StateSlotError> {
        let storage_key = key.storage_key();
        let json_value =
            serde_json::to_value(value).map_err(|source| StateSlotError::Serialize {
                key: storage_key.clone(),
                source,
            })?;
        self.extra.insert(storage_key, json_value);
        Ok(self)
    }

    /// Builds the final VersionedState.
    ///
    /// Creates a new VersionedState with all the configured messages and metadata.
    /// All channels are initialized with version 1. If no messages were added,
    /// the messages channel will be empty.
    ///
    /// # Returns
    ///
    /// A fully constructed `VersionedState`
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::state::VersionedState;
    /// use serde_json::json;
    ///
    /// let state = VersionedState::builder()
    ///     .with_user_message("Hello")
    ///     .with_extra("key", json!("value"))
    ///     .build();
    ///
    /// let snapshot = state.snapshot();
    /// assert_eq!(snapshot.messages.len(), 1);
    /// assert_eq!(snapshot.extra.len(), 1);
    /// ```
    pub fn build(self) -> VersionedState {
        VersionedState {
            messages: MessagesChannel::new(self.messages, 1),
            extra: ExtrasChannel::new(self.extra, 1),
            errors: ErrorsChannel::default(),
        }
    }
}
