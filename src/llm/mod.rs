//! Framework-agnostic LLM abstractions and optional adapters.
//!
//! This module defines provider traits that are independent of any specific
//! LLM SDK. The Rig adapter is available behind the `rig` feature.

pub mod traits;

#[cfg(feature = "rig")]
pub mod rig_adapter;

pub use traits::{LlmError, LlmProvider, LlmResponse, LlmStreamProvider};
