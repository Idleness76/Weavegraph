/*!
Persistence primitives for serializing/deserializing Weavegraph runtime
state and checkpoints (used by the SQLite checkpointer and any future
persistent backends).

Design Goals:
- Provide explicit serde-friendly structs decoupled from internal
  in-memory representations.
- Keep conversion logic localized (From / TryFrom impls) so the
  checkpointer code is lean and declarative.
- Allow forward compatibility (unknown NodeKind encodings round-trip
  as `NodeKind::Custom(encoded_string)`).

This module intentionally does NOT perform I/O. It is pure data
transformation and (de)serialization glue.
*/

use chrono::Utc;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    channels::{Channel, ExtrasChannel, MessagesChannel},
    message::Message,
    runtimes::checkpointer::Checkpoint,
    state::VersionedState,
    types::NodeKind,
    utils::json_ext::JsonSerializable,
};

/// Blanket implementation of JsonSerializable for all suitable types using PersistenceError.
impl<T> JsonSerializable<PersistenceError> for T
where
    T: serde::Serialize + for<'de> serde::de::DeserializeOwned,
{
    fn to_json_string(&self) -> std::result::Result<String, PersistenceError> {
        serde_json::to_string(self).map_err(|e| PersistenceError::Serde { source: e })
    }

    fn from_json_str(s: &str) -> std::result::Result<Self, PersistenceError> {
        serde_json::from_str(s).map_err(|e| PersistenceError::Serde { source: e })
    }
}

/// Channel that stores a vector collection (e.g., messages) with version metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersistedVecChannel<T> {
    pub version: u32,
    #[serde(default)]
    pub items: Vec<T>,
}

impl<T> Default for PersistedVecChannel<T> {
    fn default() -> Self {
        Self {
            version: 1,
            items: Vec::new(),
        }
    }
}

/// Channel that stores a map collection (e.g., extra) with version metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersistedMapChannel<V> {
    pub version: u32,
    #[serde(default)]
    pub map: FxHashMap<String, V>,
}

impl<V> Default for PersistedMapChannel<V> {
    fn default() -> Self {
        Self {
            version: 1,
            map: FxHashMap::default(),
        }
    }
}

/// Complete persisted shape of the in‑memory VersionedState.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersistedState {
    pub messages: PersistedVecChannel<Message>,
    pub extra: PersistedMapChannel<Value>,
    #[serde(default)]
    pub errors: PersistedVecChannel<crate::channels::errors::ErrorEvent>,
}

/// Wrapper for the scheduler versions_seen structure.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedVersionsSeen(pub FxHashMap<String, FxHashMap<String, u64>>);

/// Full persisted checkpoint representation.
/// (Step history tables may store multiple instances of this shape.)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersistedCheckpoint {
    pub session_id: String,
    pub step: u64,
    pub state: PersistedState,
    /// Frontier encoded as string vector using NodeKind::encode().
    pub frontier: Vec<String>,
    pub versions_seen: PersistedVersionsSeen,
    pub concurrency_limit: usize,
    /// RFC3339 string form of creation time (keeps chrono::DateTime out of serialized shape).
    pub created_at: String,
    /// Nodes that executed in this step, encoded as strings
    #[serde(default)]
    pub ran_nodes: Vec<String>,
    /// Nodes that were skipped in this step, encoded as strings
    #[serde(default)]
    pub skipped_nodes: Vec<String>,
    /// Channels that were updated in this step
    #[serde(default)]
    pub updated_channels: Vec<String>,
}

use miette::Diagnostic;
use thiserror::Error;

/// Bidirectional conversion and serialization errors for persistence models.
#[derive(Debug, Error, Diagnostic)]
pub enum PersistenceError {
    #[error("missing field: {0}")]
    #[diagnostic(
        code(weavegraph::persistence::missing_field),
        help("Populate the field in the persisted JSON before conversion.")
    )]
    MissingField(&'static str),

    #[error("JSON serialization/deserialization failed: {source}")]
    #[diagnostic(
        code(weavegraph::persistence::serde),
        help("Ensure the JSON structure matches Persisted* types.")
    )]
    Serde {
        #[source]
        source: serde_json::Error,
    },

    #[error("persistence error: {0}")]
    #[diagnostic(code(weavegraph::persistence::other))]
    Other(String),
}

pub type Result<T> = std::result::Result<T, PersistenceError>;

/* ---------- VersionedState <-> PersistedState Conversions ---------- */

impl From<&VersionedState> for PersistedState {
    fn from(s: &VersionedState) -> Self {
        PersistedState {
            messages: PersistedVecChannel {
                version: s.messages.version(),
                items: s.messages.snapshot(),
            },
            extra: PersistedMapChannel {
                version: s.extra.version(),
                map: s.extra.snapshot(),
            },
            errors: PersistedVecChannel {
                version: s.errors.version(),
                items: s.errors.snapshot(),
            },
        }
    }
}

impl TryFrom<PersistedState> for VersionedState {
    type Error = PersistenceError;

    fn try_from(p: PersistedState) -> Result<Self> {
        Ok(VersionedState {
            messages: MessagesChannel::new(p.messages.items, p.messages.version),
            extra: ExtrasChannel::new(p.extra.map, p.extra.version),
            errors: crate::channels::ErrorsChannel::new(p.errors.items, p.errors.version),
        })
    }
}

/* ---------- versions_seen conversions ---------- */

impl From<&FxHashMap<String, FxHashMap<String, u64>>> for PersistedVersionsSeen {
    fn from(v: &FxHashMap<String, FxHashMap<String, u64>>) -> Self {
        PersistedVersionsSeen(v.clone())
    }
}

impl From<PersistedVersionsSeen> for FxHashMap<String, FxHashMap<String, u64>> {
    fn from(p: PersistedVersionsSeen) -> Self {
        p.0
    }
}

/* ---------- Checkpoint <-> PersistedCheckpoint Conversions ---------- */

impl From<&Checkpoint> for PersistedCheckpoint {
    fn from(cp: &Checkpoint) -> Self {
        PersistedCheckpoint {
            session_id: cp.session_id.clone(),
            step: cp.step,
            state: PersistedState::from(&cp.state),
            frontier: cp.frontier.iter().map(|k| k.encode()).collect(),
            versions_seen: PersistedVersionsSeen(cp.versions_seen.clone()),
            concurrency_limit: cp.concurrency_limit,
            created_at: cp.created_at.to_rfc3339(),
            ran_nodes: cp.ran_nodes.iter().map(|k| k.encode()).collect(),
            skipped_nodes: cp.skipped_nodes.iter().map(|k| k.encode()).collect(),
            updated_channels: cp.updated_channels.clone(),
        }
    }
}

impl TryFrom<PersistedCheckpoint> for Checkpoint {
    type Error = PersistenceError;

    fn try_from(p: PersistedCheckpoint) -> Result<Self> {
        let state = VersionedState::try_from(p.state)?;
        let frontier: Vec<NodeKind> = p.frontier.iter().map(|s| NodeKind::decode(s)).collect();
        let ran_nodes: Vec<NodeKind> = p.ran_nodes.iter().map(|s| NodeKind::decode(s)).collect();
        let skipped_nodes: Vec<NodeKind> = p
            .skipped_nodes
            .iter()
            .map(|s| NodeKind::decode(s))
            .collect();
        let parsed_dt = chrono::DateTime::parse_from_rfc3339(&p.created_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        Ok(Checkpoint {
            session_id: p.session_id,
            step: p.step,
            state,
            frontier,
            versions_seen: p.versions_seen.0,
            concurrency_limit: p.concurrency_limit,
            created_at: parsed_dt,
            ran_nodes,
            skipped_nodes,
            updated_channels: p.updated_channels,
        })
    }
}

/* ---------- Convenience JSON helpers (using JsonSerializable trait from utils::json_ext) ---------- */

// Both PersistedState and PersistedCheckpoint automatically implement JsonSerializable
// through the blanket implementation above, providing to_json_string() and from_json_str() methods.
