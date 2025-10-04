//! Core types for the Weavegraph workflow framework.
//!
//! This module defines the fundamental types used throughout the Weavegraph system
//! for identifying nodes and channels in workflow graphs. These are the core
//! domain concepts that define what a workflow *is*.
//!
//! For runtime execution types (session IDs, step numbers), see [`crate::runtimes::types`].
//!
//! # Key Types
//!
//! - [`NodeKind`]: Identifies different types of nodes in a workflow graph
//! - [`ChannelType`]: Identifies different types of data channels for state management
//!
//! # Type Organization
//!
//! Weavegraph organizes types by conceptual domain:
//!
//! - **Core types** (this module): Fundamental workflow concepts (`NodeKind`, `ChannelType`)
//! - **Runtime types** ([`crate::runtimes::types`]): Execution infrastructure (`SessionId`, `StepNumber`)
//! - **Utility types**: Domain-specific helpers in their respective modules
//!
//! # Examples
//!
//! ```rust
//! use weavegraph::types::{NodeKind, ChannelType};
//!
//! // Create different types of nodes
//! let start = NodeKind::Start;
//! let custom = NodeKind::Other("ProcessData".to_string());
//! let end = NodeKind::End;
//!
//! // Encode for persistence
//! let encoded = custom.encode();
//! assert_eq!(encoded, "Other:ProcessData");
//!
//! // Work with channels
//! let msg_channel = ChannelType::Message;
//! println!("Channel: {}", msg_channel);
//! ```

use serde::{Deserialize, Serialize};
use std::fmt;

/// Identifies the type of a node within a workflow graph.
///
/// `NodeKind` serves as a unique identifier for nodes in the workflow execution graph.
/// It provides special handling for common workflow patterns (start/end nodes) while
/// allowing arbitrary custom node types through the `Other` variant.
///
/// # Persistence
///
/// `NodeKind` supports serialization for checkpointing and persistence through both
/// serde and the [`encode`](Self::encode)/[`decode`](Self::decode) methods.
///
/// # Examples
///
/// ```rust
/// use weavegraph::types::NodeKind;
///
/// // Special workflow nodes
/// let start = NodeKind::Start;
/// let end = NodeKind::End;
///
/// // Custom application nodes
/// let processor = NodeKind::Other("DataProcessor".to_string());
///
/// // Persistence round-trip
/// let encoded = processor.encode();
/// let decoded = NodeKind::decode(&encoded);
/// assert_eq!(processor, decoded);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeKind {
    /// Entry point node that begins workflow execution.
    ///
    /// Start nodes have no incoming edges and serve as the initial
    /// frontier for workflow execution.
    Start,

    /// Terminal node that completes workflow execution.
    ///
    /// End nodes typically have no outgoing edges and signal
    /// the completion of a workflow branch.
    End,

    /// Custom node type identified by a user-defined string.
    ///
    /// The string should be descriptive and unique within the workflow.
    /// Common patterns include function names, service names, or step descriptions.
    Other(String),
}

impl NodeKind {
    /// Encode a NodeKind into its persisted string form.
    ///
    /// The encoding format is human-readable and forward-compatible:
    /// - `Start` → `"Start"`
    /// - `End` → `"End"`
    /// - `Other("X")` → `"Other:X"`
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use weavegraph::types::NodeKind;
    /// assert_eq!(NodeKind::Start.encode(), "Start");
    /// assert_eq!(NodeKind::Other("Processor".to_string()).encode(), "Other:Processor");
    /// ```
    #[must_use]
    pub fn encode(&self) -> String {
        match self {
            NodeKind::Start => "Start".to_string(),
            NodeKind::End => "End".to_string(),
            NodeKind::Other(s) => format!("Other:{s}"),
        }
    }

    /// Decode a persisted string form back into a NodeKind.
    ///
    /// This method provides forward compatibility by falling back to
    /// `Other(s)` for any unrecognized format.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use weavegraph::types::NodeKind;
    /// assert_eq!(NodeKind::decode("Start"), NodeKind::Start);
    /// assert_eq!(NodeKind::decode("Other:Processor"), NodeKind::Other("Processor".to_string()));
    ///
    /// // Forward compatibility - unknown formats become Other
    /// assert_eq!(NodeKind::decode("Unknown"), NodeKind::Other("Unknown".to_string()));
    /// ```
    pub fn decode(s: &str) -> Self {
        if s == "Start" {
            NodeKind::Start
        } else if s == "End" {
            NodeKind::End
        } else if let Some(rest) = s.strip_prefix("Other:") {
            NodeKind::Other(rest.to_string())
        } else {
            NodeKind::Other(s.to_string())
        }
    }

    /// Returns `true` if this is a [`Start`](Self::Start) node.
    #[must_use]
    pub fn is_start(&self) -> bool {
        matches!(self, Self::Start)
    }

    /// Returns `true` if this is an [`End`](Self::End) node.
    #[must_use]
    pub fn is_end(&self) -> bool {
        matches!(self, Self::End)
    }

    /// Returns `true` if this is a custom [`Other`](Self::Other) node.
    #[must_use]
    pub fn is_custom(&self) -> bool {
        matches!(self, Self::Other(_))
    }
}

impl fmt::Display for NodeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start => write!(f, "Start"),
            Self::End => write!(f, "End"),
            Self::Other(name) => write!(f, "{}", name),
        }
    }
}

/// Identifies the type of data channel used for state management.
///
/// `ChannelType` represents the different categories of state data that
/// can be managed within the workflow system. Each channel type has
/// its own reducer and update semantics.
///
/// # Examples
///
/// ```rust
/// use weavegraph::types::ChannelType;
///
/// let msg_channel = ChannelType::Message;
/// let err_channel = ChannelType::Error;
/// let meta_channel = ChannelType::Extra;
///
/// println!("Processing {} channel", msg_channel);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChannelType {
    /// Channel for chat messages and conversation data.
    ///
    /// Manages the sequence of messages that flow through the workflow,
    /// including user inputs, assistant responses, and system notifications.
    Message,

    /// Channel for error events and diagnostic information.
    ///
    /// Collects both fatal errors that halt execution and non-fatal
    /// errors that should be tracked for debugging and monitoring.
    Error,

    /// Channel for custom metadata and intermediate results.
    ///
    /// Provides a flexible key-value store for custom data that nodes
    /// need to share, including configuration and intermediate computations.
    Extra,
}

impl fmt::Display for ChannelType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Message => write!(f, "message"),
            Self::Error => write!(f, "error"),
            Self::Extra => write!(f, "extra"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nodekind_predicates() {
        assert!(NodeKind::Start.is_start());
        assert!(!NodeKind::Start.is_end());
        assert!(!NodeKind::Start.is_custom());

        assert!(!NodeKind::End.is_start());
        assert!(NodeKind::End.is_end());
        assert!(!NodeKind::End.is_custom());

        let custom = NodeKind::Other("Test".to_string());
        assert!(!custom.is_start());
        assert!(!custom.is_end());
        assert!(custom.is_custom());
    }

    #[test]
    fn test_nodekind_encode_decode() {
        let test_cases = vec![
            (NodeKind::Start, "Start"),
            (NodeKind::End, "End"),
            (NodeKind::Other("Processor".to_string()), "Other:Processor"),
        ];

        for (node, expected) in test_cases {
            let encoded = node.encode();
            assert_eq!(encoded, expected);

            let decoded = NodeKind::decode(&encoded);
            assert_eq!(decoded, node);
        }
    }

    #[test]
    fn test_display() {
        assert_eq!(NodeKind::Start.to_string(), "Start");
        assert_eq!(NodeKind::End.to_string(), "End");
        assert_eq!(
            NodeKind::Other("DataProcessor".to_string()).to_string(),
            "DataProcessor"
        );

        assert_eq!(ChannelType::Message.to_string(), "message");
        assert_eq!(ChannelType::Error.to_string(), "error");
        assert_eq!(ChannelType::Extra.to_string(), "extra");
    }

    #[test]
    fn test_serde_support() {
        // Test NodeKind serialization
        let nodes = vec![
            NodeKind::Start,
            NodeKind::End,
            NodeKind::Other("TestNode".to_string()),
        ];
        for node in nodes {
            let serialized = serde_json::to_string(&node).unwrap();
            let deserialized: NodeKind = serde_json::from_str(&serialized).unwrap();
            assert_eq!(node, deserialized);
        }

        // Test ChannelType serialization
        let channels = vec![ChannelType::Message, ChannelType::Error, ChannelType::Extra];
        for channel in channels {
            let serialized = serde_json::to_string(&channel).unwrap();
            let deserialized: ChannelType = serde_json::from_str(&serialized).unwrap();
            assert_eq!(channel, deserialized);
        }
    }
}
