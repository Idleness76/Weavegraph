//! Input validation module — injection scanning, normalization, and content analysis.

#[cfg(feature = "heuristics")]
pub mod normalization;
#[cfg(feature = "heuristics")]
pub mod patterns;
#[cfg(feature = "heuristics")]
pub mod injection;
#[cfg(feature = "heuristics")]
pub mod structural;
