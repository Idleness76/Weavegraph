//! Utilities module for common functionality across the Weavegraph framework.
//!
//! This module provides reusable utilities and common patterns that are used
//! throughout the codebase. These utilities are designed to be generic,
//! type-safe, and ergonomic to maximize applicability and maintain consistency.
//!
//! # Module Organization
//!
//! - [`collections`]: Collection utilities and common patterns for maps and data structures
//! - [`clock`]: Injectable time sources for checkpoints and time-based operations  
//! - [`deterministic_rng`]: Deterministic random number generation for testing
//! - [`id_generator`]: ID generation utilities for runs, steps, nodes, and sessions
//! - [`testing`]: Shared testing utilities and test node implementations
//! - [`json_ext`]: JSON manipulation utilities and extensions
//! - [`merge_inspector`]: State merge debugging and inspection tools *(placeholder)*
//! - [`message_id_helpers`]: Message and tool-call ID generation *(placeholder)*
//! - [`type_guards`]: Type validation and shape checking utilities *(placeholder)*
//!
//! # Design Principles
//!
//! - **Type Safety**: Leverage Rust's type system for compile-time correctness
//! - **Error Handling**: Consistent use of `Result`, `Option`, and `thiserror`/`miette`
//! - **Ergonomics**: Builder patterns, extension traits, and intuitive APIs
//! - **Reusability**: Generic implementations that work across different contexts
//! - **Testing**: Comprehensive test coverage and deterministic behavior
//!
//! # Common Patterns
//!
//! ## Extra Data Maps
//! ```rust
//! use weavegraph::utils::collections::{new_extra_map, ExtraMapExt};
//!
//! let mut extra = new_extra_map();
//! extra.insert_string("key", "value");
//! extra.insert_number("count", 42);
//! ```
//!
//! ## ID Generation
//! ```rust
//! use weavegraph::utils::id_generator::IdGenerator;
//!
//! let generator = IdGenerator::new();
//! let run_id = generator.generate_run_id();
//! let step_id = generator.generate_step_id();
//! ```
//!
//! ## Time Abstraction
//! ```rust
//! use weavegraph::utils::clock::{Clock, SystemClock, MockClock};
//!
//! // Production
//! let clock = SystemClock;
//! let timestamp = clock.now();
//!
//! // Testing  
//! let mut mock_clock = MockClock::new(1000);
//! mock_clock.advance_secs(30);
//! ```

pub mod clock;
pub mod collections;
pub mod deterministic_rng;
pub mod id_generator;
pub mod json_ext;
pub mod merge_inspector;
pub mod message_id_helpers;
pub mod testing;
pub mod type_guards;
