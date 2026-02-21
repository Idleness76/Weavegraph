//! Prompt protection module — system prompt security, template hardening, and leakage detection.

#[cfg(feature = "heuristics")]
pub mod scanner;

#[cfg(feature = "honeytoken")]
pub mod honeytoken;

#[cfg(feature = "heuristics")]
pub mod template;

pub mod refusal;
