//! Configuration management for security policies.
//!
//! This module provides the core configuration infrastructure for wg-bastion, including:
//!
//! - [`SecurityPolicy`] – The main policy struct with per-module configurations
//! - [`PolicyBuilder`] – Builder for constructing policies from files, env vars, and overrides
//! - [`FailMode`] – Enumeration of failure behaviors (open/closed/log)
//!
//! ## Configuration Hierarchy
//!
//! Policies are resolved in the following order (later wins):
//!
//! 1. Compiled defaults (secure by default)
//! 2. Global config file (`wg-bastion.toml` or `wg-bastion.yaml`)
//! 3. Environment variables (`WG_BASTION_*`)
//! 4. Graph-level overrides (future)
//! 5. Node-level overrides (future)
//! 6. Request-level overrides (with audit logging)
//!
//! ## Example
//!
//! ```rust,ignore
//! use wg_bastion::config::PolicyBuilder;
//!
//! let policy = PolicyBuilder::new()
//!     .with_file("config/security.toml")?
//!     .with_env()
//!     .build()?;
//!
//! assert!(policy.enabled);
//! ```

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;
use validator::Validate;

/// Errors that can occur during policy configuration
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Failed to read configuration file
    #[error("Failed to read config file at {path}: {source}")]
    FileRead {
        /// Path that failed to read
        path: PathBuf,
        /// Underlying I/O error
        source: std::io::Error,
    },

    /// Failed to parse configuration
    #[error("Failed to parse {format} config: {source}")]
    ParseError {
        /// Format that failed to parse (YAML, TOML, JSON)
        format: String,
        /// Underlying parse error
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Unsupported or unrecognised configuration file extension
    #[error("Unsupported config file format: {message}")]
    UnsupportedFormat {
        /// Description of the problem
        message: String,
    },

    /// Configuration validation failed
    #[error("Policy validation failed: {0}")]
    Validation(#[from] validator::ValidationErrors),

    /// Environment variable parsing error
    #[error("Failed to parse environment variable {key}: {message}")]
    EnvParse {
        /// Environment variable key
        key: String,
        /// Error message
        message: String,
    },
}

/// Behavior when a security check fails
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailMode {
    /// Block the request and return an error
    Closed,
    /// Allow the request but log the security event
    Open,
    /// Log the event and continue (audit mode)
    LogOnly,
}

impl Default for FailMode {
    fn default() -> Self {
        Self::Closed // Secure by default
    }
}

/// Main security policy configuration
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct SecurityPolicy {
    /// Policy version for compatibility tracking
    #[validate(length(min = 1))]
    pub version: String,

    /// Global enable/disable flag
    pub enabled: bool,

    /// Default failure behavior
    pub fail_mode: FailMode,
    // Module-specific configurations (stubs for now)
    // pub input: InputPolicyConfig,
    // pub output: OutputPolicyConfig,
    // pub prompt: PromptPolicyConfig,
    // pub tools: ToolPolicyConfig,
    // pub rag: RagPolicyConfig,
    // pub agents: AgentPolicyConfig,
    // pub abuse: AbusePolicyConfig,
    // pub telemetry: TelemetryPolicyConfig,
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        Self {
            version: "1.0".to_string(),
            enabled: true,
            fail_mode: FailMode::Closed,
        }
    }
}

/// Builder for constructing security policies from multiple sources
#[derive(Debug, Default)]
pub struct PolicyBuilder {
    base: SecurityPolicy,
    file_path: Option<PathBuf>,
    use_env: bool,
}

impl PolicyBuilder {
    /// Create a new policy builder with secure defaults
    #[must_use]
    pub fn new() -> Self {
        Self {
            base: SecurityPolicy::default(),
            file_path: None,
            use_env: false,
        }
    }

    /// Load policy from a configuration file (YAML, TOML, or JSON)
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] if the file cannot be read or parsed
    pub fn with_file(mut self, path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        self.file_path = Some(path.to_path_buf());

        let content = std::fs::read_to_string(path).map_err(|source| ConfigError::FileRead {
            path: path.to_path_buf(),
            source,
        })?;

        let policy: SecurityPolicy = match path.extension().and_then(|e| e.to_str()) {
            Some("yaml" | "yml") => {
                serde_yaml::from_str(&content).map_err(|e| ConfigError::ParseError {
                    format: "YAML".to_string(),
                    source: Box::new(e),
                })?
            }
            Some("toml") => toml::from_str(&content).map_err(|e| ConfigError::ParseError {
                format: "TOML".to_string(),
                source: Box::new(e),
            })?,
            Some("json") => {
                serde_json::from_str(&content).map_err(|e| ConfigError::ParseError {
                    format: "JSON".to_string(),
                    source: Box::new(e),
                })?
            }
            _ => {
                return Err(ConfigError::UnsupportedFormat {
                    message: "file extension must be .yaml, .yml, .toml, or .json"
                        .to_string(),
                });
            }
        };

        self.base = policy;
        Ok(self)
    }

    /// Enable loading overrides from environment variables
    ///
    /// Looks for variables prefixed with `WG_BASTION_`, e.g.:
    /// - `WG_BASTION_ENABLED=false`
    /// - `WG_BASTION_FAIL_MODE=open`
    #[must_use]
    pub fn with_env(mut self) -> Self {
        self.use_env = true;
        self
    }

    /// Build the final security policy
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] if validation fails or environment variables are invalid
    pub fn build(mut self) -> Result<SecurityPolicy, ConfigError> {
        // Apply environment variable overrides
        if self.use_env {
            dotenvy::dotenv().ok(); // Load .env file if present

            if let Ok(enabled) = std::env::var("WG_BASTION_ENABLED") {
                self.base.enabled = enabled.parse().map_err(|_| ConfigError::EnvParse {
                    key: "WG_BASTION_ENABLED".to_string(),
                    message: "Must be 'true' or 'false'".to_string(),
                })?;
            }

            if let Ok(fail_mode) = std::env::var("WG_BASTION_FAIL_MODE") {
                self.base.fail_mode = match fail_mode.to_lowercase().as_str() {
                    "closed" => FailMode::Closed,
                    "open" => FailMode::Open,
                    "log_only" | "logonly" => FailMode::LogOnly,
                    _ => {
                        return Err(ConfigError::EnvParse {
                            key: "WG_BASTION_FAIL_MODE".to_string(),
                            message: "Must be 'closed', 'open', or 'log_only'".to_string(),
                        });
                    }
                };
            }
        }

        // Validate the final policy
        self.base.validate()?;

        Ok(self.base)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_policy() {
        let policy = SecurityPolicy::default();
        assert!(policy.enabled);
        assert_eq!(policy.fail_mode, FailMode::Closed);
        assert_eq!(policy.version, "1.0");
    }

    #[test]
    fn test_policy_builder() {
        let policy = PolicyBuilder::new().build().unwrap();
        assert!(policy.enabled);
    }

    #[test]
    fn test_fail_mode_serialization() {
        let json = serde_json::to_string(&FailMode::Closed).unwrap();
        assert_eq!(json, r#""closed""#);

        let parsed: FailMode = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, FailMode::Closed);
    }
}
