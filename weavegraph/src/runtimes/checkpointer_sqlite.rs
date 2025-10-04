/*!
SQLite Checkpointer

This module provides the `SQLiteCheckpointer` async implementation of the
`Checkpointer` trait defined in `runtimes/checkpointer.rs`.

## Features

- **Complete Step History**: Stores full execution metadata including ran/skipped nodes
- **Pagination Support**: Efficient querying of large checkpoint histories
- **Optimistic Concurrency**: Prevention of concurrent checkpoint conflicts
- **Serde Integration**: Uses persistence models for consistent serialization

## Behavior

- Uses serde-based persistence models (see `runtimes::persistence`) for
  encoding `VersionedState`, frontier node kinds, and `versions_seen`.
- When the `sqlite-migrations` feature is enabled (default), embedded
  migrations (`sqlx::migrate!("./migrations")`) are executed on connect;
  disabling the feature assumes external migration orchestration.

## Design Goals

- Keep this module focused on database I/O; pure serialization lives in
  the persistence module.
- Provide efficient querying with filtering and pagination support.
- Ensure data consistency with optimistic concurrency control.

## Database Schema

The checkpoint data maps to database tables as follows:

- `sessions.id` ← `checkpoint.session_id`
- `sessions.concurrency_limit` ← `checkpoint.concurrency_limit`
- `steps.session_id` ← `checkpoint.session_id`
- `steps.step` ← `checkpoint.step`
- `steps.state_json` ← serialized `VersionedState`
- `steps.frontier_json` ← JSON array of encoded `NodeKind`
- `steps.versions_seen_json` ← JSON object (node → channel → version)
- `steps.ran_nodes_json` ← JSON array of executed nodes
- `steps.skipped_nodes_json` ← JSON array of skipped nodes
- `steps.updated_channels_json` ← JSON array of updated channel names

## NodeKind Encoding

NodeKinds are encoded as strings for JSON storage:
- `Start` → `"Start"`
- `End` → `"End"`
- `Other(name)` → `"Other:<name>"`
*/

use std::sync::Arc;

use chrono::{DateTime, Utc};
use miette::Diagnostic;
use serde_json::Value;
use sqlx::{sqlite::SqliteRow, Row, SqlitePool};
use thiserror::Error;
use tracing::instrument;

use crate::{
    runtimes::checkpointer::{Checkpoint, Checkpointer, CheckpointerError, Result},
    runtimes::persistence::{PersistedState, PersistedVersionsSeen},
    state::VersionedState,
    types::NodeKind,
};

use super::checkpointer_sqlite_helpers::{
    deserialize_json, deserialize_json_value, require_json_field, serialize_json,
};

/// Query parameters for filtering step history.
#[derive(Debug, Clone, Default)]
pub struct StepQuery {
    /// Maximum number of results to return (capped at 1000)
    pub limit: Option<u32>,
    /// Number of results to skip (for pagination)
    pub offset: Option<u32>,
    /// Filter by minimum step number (inclusive)
    pub min_step: Option<u64>,
    /// Filter by maximum step number (inclusive)
    pub max_step: Option<u64>,
    /// Only return steps that executed the specified node
    pub ran_node: Option<NodeKind>,
    /// Only return steps that skipped the specified node
    pub skipped_node: Option<NodeKind>,
}

/// Pagination information for query results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageInfo {
    /// Total number of matching records
    pub total_count: u64,
    /// Number of records returned in this page
    pub page_size: u32,
    /// Zero-based offset of the first record in this page
    pub offset: u32,
    /// Whether there are more records after this page
    pub has_next_page: bool,
}

/// Paginated query result for step history.
#[derive(Debug, Clone)]
pub struct StepQueryResult {
    /// The matching checkpoints
    pub checkpoints: Vec<Checkpoint>,
    /// Pagination metadata
    pub page_info: PageInfo,
}

#[derive(Debug, Error, Diagnostic)]
pub enum SqliteCheckpointerError {
    #[error("SQLx error: {0}")]
    #[diagnostic(
        code(weavegraph::sqlite::sqlx),
        help("Ensure the SQLite database URL is valid and accessible.")
    )]
    Sqlx(#[from] sqlx::Error),

    #[error("JSON serialization error: {0}")]
    #[diagnostic(
        code(weavegraph::sqlite::serde),
        help("Check serialized shapes for state/frontier/versions_seen.")
    )]
    Serde(#[from] serde_json::Error),

    #[error("Missing persisted field: {0}")]
    #[diagnostic(
        code(weavegraph::sqlite::missing),
        help("Backfill or re-run migrations to populate the missing field.")
    )]
    Missing(&'static str),

    #[error("Backend error: {0}")]
    #[diagnostic(code(weavegraph::sqlite::backend))]
    Backend(String),

    #[error("Other error: {0}")]
    #[diagnostic(code(weavegraph::sqlite::other))]
    Other(String),
}

impl From<SqliteCheckpointerError> for CheckpointerError {
    fn from(e: SqliteCheckpointerError) -> Self {
        match e {
            SqliteCheckpointerError::Sqlx(err) => CheckpointerError::Backend {
                message: err.to_string(),
            },
            SqliteCheckpointerError::Serde(err) => CheckpointerError::Other {
                message: err.to_string(),
            },
            SqliteCheckpointerError::Missing(what) => CheckpointerError::Other {
                message: format!("missing persisted field: {what}"),
            },
            SqliteCheckpointerError::Backend(msg) => CheckpointerError::Backend { message: msg },
            SqliteCheckpointerError::Other(msg) => CheckpointerError::Other { message: msg },
        }
    }
}

/// SQLite-backed implementation of `Checkpointer`.
///
/// Provides persistent checkpoint storage with advanced querying capabilities
/// including pagination, filtering, and optimistic concurrency control.
pub struct SQLiteCheckpointer {
    /// Shared SQLite connection pool for concurrent checkpoint operations
    pool: Arc<SqlitePool>,
}

impl std::fmt::Debug for SQLiteCheckpointer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SQLiteCheckpointer").finish()
    }
}

impl SQLiteCheckpointer {
    /// Connect (or create) a SQLite database at `database_url`.
    /// Example URL: \"sqlite://weavegraph.db\"
    ///
    /// Returns a configured `SQLiteCheckpointer` ready for use.
    #[must_use = "checkpointer must be used to persist state"]
    #[instrument(skip(database_url))]
    pub async fn connect(database_url: &str) -> std::result::Result<Self, CheckpointerError> {
        let pool =
            SqlitePool::connect(database_url)
                .await
                .map_err(|e| CheckpointerError::Backend {
                    message: format!("connect error: {e}"),
                })?;
        // Run embedded migrations only if the feature is enabled (idempotent).
        #[cfg(feature = "sqlite-migrations")]
        {
            if let Err(e) = sqlx::migrate!("./migrations").run(&pool).await {
                return Err(CheckpointerError::Backend {
                    message: format!("migration failure: {e}"),
                });
            }
        }
        #[cfg(not(feature = "sqlite-migrations"))]
        {
            // Feature disabled: assume external migration orchestration already applied schema.
        }
        Ok(Self {
            pool: Arc::new(pool),
        })
    }
}

#[async_trait::async_trait]
impl Checkpointer for SQLiteCheckpointer {
    #[instrument(skip(self, checkpoint), err)]
    async fn save(&self, checkpoint: Checkpoint) -> Result<()> {
        // Serialize using persistence module (serde-based)
        let persisted_state = PersistedState::from(&checkpoint.state);
        let state_json = serialize_json(&persisted_state, "state")?;
        let frontier_enc: Vec<String> = checkpoint.frontier.iter().map(|k| k.encode()).collect();
        let frontier_json = serialize_json(&frontier_enc, "frontier")?;
        let persisted_vs = PersistedVersionsSeen(checkpoint.versions_seen.clone());
        let versions_seen_json = serialize_json(&persisted_vs, "versions_seen")?;

        // Serialize step execution metadata
        let ran_nodes_enc: Vec<String> = checkpoint.ran_nodes.iter().map(|k| k.encode()).collect();
        let ran_nodes_json = serialize_json(&ran_nodes_enc, "ran_nodes")?;
        let skipped_nodes_enc: Vec<String> = checkpoint
            .skipped_nodes
            .iter()
            .map(|k| k.encode())
            .collect();
        let skipped_nodes_json = serialize_json(&skipped_nodes_enc, "skipped_nodes")?;
        let updated_channels_json =
            serialize_json(&checkpoint.updated_channels, "updated_channels")?;

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CheckpointerError::Backend {
                message: format!("tx begin: {e}"),
            })?;

        // Ensure session row
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO sessions (id, concurrency_limit)
            VALUES (?1, ?2)
        "#,
        )
        .bind(&checkpoint.session_id)
        .bind(checkpoint.concurrency_limit as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| CheckpointerError::Backend {
            message: format!("insert session: {e}"),
        })?;

        // Insert or replace step row (allows idempotent re-save of same step)
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO steps (
                session_id,
                step,
                state_json,
                frontier_json,
                versions_seen_json,
                ran_nodes_json,
                skipped_nodes_json,
                updated_channels_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
        )
        .bind(&checkpoint.session_id)
        .bind(checkpoint.step as i64)
        .bind(&state_json)
        .bind(&frontier_json)
        .bind(&versions_seen_json)
        .bind(&ran_nodes_json)
        .bind(&skipped_nodes_json)
        .bind(&updated_channels_json)
        .execute(&mut *tx)
        .await
        .map_err(|e| CheckpointerError::Backend {
            message: format!("insert step: {e}"),
        })?;

        tx.commit().await.map_err(|e| CheckpointerError::Backend {
            message: format!("tx commit: {e}"),
        })?;

        Ok(())
    }

    #[instrument(skip(self, session_id), err)]
    async fn load_latest(&self, session_id: &str) -> Result<Option<Checkpoint>> {
        let row_opt: Option<SqliteRow> = sqlx::query(
            r#"
            SELECT
                s.id,
                s.last_step,
                s.last_state_json,
                s.last_frontier_json,
                s.last_versions_seen_json,
                s.concurrency_limit,
                s.updated_at
            FROM sessions s
            WHERE s.id = ?1
            "#,
        )
        .bind(session_id)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| CheckpointerError::Backend {
            message: format!("select latest: {e}"),
        })?;

        let row = match row_opt {
            Some(r) => r,
            None => return Ok(None),
        };

        let last_step: i64 = row.get("last_step");

        let state_json: Option<String> =
            row.try_get("last_state_json")
                .map_err(|e| CheckpointerError::Backend {
                    message: format!("last_state_json read: {e}"),
                })?;
        let frontier_json: Option<String> =
            row.try_get("last_frontier_json")
                .map_err(|e| CheckpointerError::Backend {
                    message: format!("last_frontier_json read: {e}"),
                })?;
        let versions_seen_json: Option<String> =
            row.try_get("last_versions_seen_json")
                .map_err(|e| CheckpointerError::Backend {
                    message: format!("last_versions_seen_json read: {e}"),
                })?;
        let concurrency_limit: i64 = row.get("concurrency_limit");
        let updated_at_str: String = row.get("updated_at");

        if last_step == 0 && state_json.is_none() {
            // Session row exists but no checkpoint has been persisted yet.
            return Ok(None);
        }

        let state_payload = require_json_field(state_json, "state_json")?;
        let frontier_payload = require_json_field(frontier_json, "frontier_json")?;
        let versions_seen_payload = require_json_field(versions_seen_json, "versions_seen_json")?;

        let state_val: Value = deserialize_json(&state_payload, "state")?;
        let frontier_val: Value = deserialize_json(&frontier_payload, "frontier")?;
        let versions_seen_val: Value = deserialize_json(&versions_seen_payload, "versions_seen")?;

        // Deserialize using persistence models
        let persisted_state: PersistedState = deserialize_json_value(state_val, "state")?;
        let state =
            VersionedState::try_from(persisted_state).map_err(|e| CheckpointerError::Other {
                message: format!("state convert: {e}"),
            })?;
        let frontier: Vec<NodeKind> = frontier_val
            .as_array()
            .ok_or_else(|| CheckpointerError::Other {
                message: "frontier not array".to_string(),
            })?
            .iter()
            .filter_map(|v| v.as_str())
            .map(NodeKind::decode)
            .collect();
        let persisted_vs: PersistedVersionsSeen =
            deserialize_json_value(versions_seen_val, "versions_seen")?;
        let versions_seen = persisted_vs.0;

        let created_at = DateTime::parse_from_rfc3339(&updated_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        Ok(Some(Checkpoint {
            session_id: session_id.to_string(),
            step: last_step as u64,
            state,
            frontier,
            versions_seen,
            concurrency_limit: concurrency_limit as usize,
            created_at,
            // Note: load_latest uses denormalized session data which doesn't include
            // step execution metadata. Use query_steps() for full checkpoint details.
            ran_nodes: vec![],
            skipped_nodes: vec![],
            updated_channels: vec![],
        }))
    }

    #[instrument(skip(self), err)]
    async fn list_sessions(&self) -> Result<Vec<String>> {
        let rows = sqlx::query(
            r#"
            SELECT id FROM sessions
            ORDER BY updated_at DESC
            "#,
        )
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| CheckpointerError::Backend {
            message: format!("list sessions: {e}"),
        })?;

        Ok(rows.into_iter().map(|r| r.get::<String, _>("id")).collect())
    }
}

// Extended SQLiteCheckpointer methods (not part of base Checkpointer trait)
impl SQLiteCheckpointer {
    /// Query step history with filtering and pagination.
    ///
    /// This method provides comprehensive access to checkpoint history with
    /// support for filtering by step range, node execution, and pagination
    /// for efficient access to large histories.
    ///
    /// # Parameters
    ///
    /// * `session_id` - Session to query
    /// * `query` - Filter and pagination parameters
    ///
    /// # Returns
    ///
    /// * `Ok(StepQueryResult)` - Matching checkpoints with pagination info
    /// * `Err(CheckpointerError)` - Query execution failure
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use weavegraph::runtimes::checkpointer_sqlite::{SQLiteCheckpointer, StepQuery};
    /// use weavegraph::types::NodeKind;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let checkpointer = SQLiteCheckpointer::connect("sqlite://app.db").await?;
    ///
    /// // Get recent steps with pagination
    /// let query = StepQuery {
    ///     limit: Some(10),
    ///     offset: Some(0),
    ///     min_step: Some(5),
    ///     ..Default::default()
    /// };
    ///
    /// let result = checkpointer.query_steps("session1", query).await?;
    /// println!("Found {} steps", result.page_info.page_size);
    /// # Ok(())
    /// # }
    /// ```
    #[instrument(skip(self), err)]
    pub async fn query_steps(&self, session_id: &str, query: StepQuery) -> Result<StepQueryResult> {
        // Build WHERE clause conditions
        let mut conditions = vec!["session_id = ?1".to_string()];
        let mut param_count = 1;

        if query.min_step.is_some() {
            param_count += 1;
            conditions.push(format!("step >= ?{param_count}"));
        }
        if query.max_step.is_some() {
            param_count += 1;
            conditions.push(format!("step <= ?{param_count}"));
        }
        if query.ran_node.is_some() {
            param_count += 1;
            conditions.push(format!(
                "JSON_EXTRACT(ran_nodes_json, '$') LIKE ?{param_count}"
            ));
        }
        if query.skipped_node.is_some() {
            param_count += 1;
            conditions.push(format!(
                "JSON_EXTRACT(skipped_nodes_json, '$') LIKE ?{param_count}"
            ));
        }

        let where_clause = conditions.join(" AND ");

        // Count total matching records
        let count_sql = format!("SELECT COUNT(*) as total FROM steps WHERE {where_clause}");

        let limit = query.limit.unwrap_or(100).min(1000); // Cap at 1000
        let offset = query.offset.unwrap_or(0);

        // Query with pagination
        let select_sql = format!(
            r#"SELECT 
                session_id, step, state_json, frontier_json, versions_seen_json,
                ran_nodes_json, skipped_nodes_json, updated_channels_json, created_at
               FROM steps 
               WHERE {where_clause}
               ORDER BY step DESC
               LIMIT {limit} OFFSET {offset}"#
        );

        // Execute count query
        let mut count_query = sqlx::query(&count_sql).bind(session_id);
        if let Some(min_step) = query.min_step {
            count_query = count_query.bind(min_step as i64);
        }
        if let Some(max_step) = query.max_step {
            count_query = count_query.bind(max_step as i64);
        }
        if let Some(ran_node) = &query.ran_node {
            count_query = count_query.bind(format!("%{}%", ran_node.encode()));
        }
        if let Some(skipped_node) = &query.skipped_node {
            count_query = count_query.bind(format!("%{}%", skipped_node.encode()));
        }

        let total_count: i64 = count_query
            .fetch_one(&*self.pool)
            .await
            .map_err(|e| CheckpointerError::Backend {
                message: format!("count query: {e}"),
            })?
            .get("total");

        // Execute select query
        let mut select_query = sqlx::query(&select_sql).bind(session_id);
        if let Some(min_step) = query.min_step {
            select_query = select_query.bind(min_step as i64);
        }
        if let Some(max_step) = query.max_step {
            select_query = select_query.bind(max_step as i64);
        }
        if let Some(ran_node) = &query.ran_node {
            select_query = select_query.bind(format!("%{}%", ran_node.encode()));
        }
        if let Some(skipped_node) = &query.skipped_node {
            select_query = select_query.bind(format!("%{}%", skipped_node.encode()));
        }

        let rows =
            select_query
                .fetch_all(&*self.pool)
                .await
                .map_err(|e| CheckpointerError::Backend {
                    message: format!("select query: {e}"),
                })?;

        // Convert rows to checkpoints
        let mut checkpoints = Vec::new();
        for row in rows {
            let checkpoint = self.row_to_checkpoint(session_id, &row).await?;
            checkpoints.push(checkpoint);
        }

        let page_info = PageInfo {
            total_count: total_count as u64,
            page_size: checkpoints.len() as u32,
            offset,
            has_next_page: (offset + limit) < total_count as u32,
        };

        Ok(StepQueryResult {
            checkpoints,
            page_info,
        })
    }

    /// Save a checkpoint with optimistic concurrency control.
    ///
    /// This method prevents concurrent modifications by checking that the
    /// session's last step matches the expected value before saving.
    /// This ensures checkpoint sequence integrity in multi-writer scenarios.
    ///
    /// # Parameters
    ///
    /// * `checkpoint` - The checkpoint to save
    /// * `expected_last_step` - Expected current step number (for concurrency control)
    ///
    /// # Returns
    ///
    /// * `Ok(())` - Checkpoint saved successfully
    /// * `Err(CheckpointerError::Backend)` - Concurrency conflict or storage error
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use weavegraph::runtimes::checkpointer_sqlite::SQLiteCheckpointer;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let checkpointer = SQLiteCheckpointer::connect("sqlite://app.db").await?;
    /// # let checkpoint = todo!();
    ///
    /// // Save step 5, expecting current step to be 4
    /// match checkpointer.save_with_concurrency_check(checkpoint, Some(4)).await {
    ///     Ok(()) => println!("Checkpoint saved successfully"),
    ///     Err(e) => println!("Concurrency conflict or error: {}", e),
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[instrument(skip(self, checkpoint), err)]
    pub async fn save_with_concurrency_check(
        &self,
        checkpoint: Checkpoint,
        expected_last_step: Option<u64>,
    ) -> Result<()> {
        // Serialize checkpoint data
        let persisted_state = PersistedState::from(&checkpoint.state);
        let state_json = serialize_json(&persisted_state, "state")?;
        let frontier_enc: Vec<String> = checkpoint.frontier.iter().map(|k| k.encode()).collect();
        let frontier_json = serialize_json(&frontier_enc, "frontier")?;
        let persisted_vs = PersistedVersionsSeen(checkpoint.versions_seen.clone());
        let versions_seen_json = serialize_json(&persisted_vs, "versions_seen")?;
        let ran_nodes_enc: Vec<String> = checkpoint.ran_nodes.iter().map(|k| k.encode()).collect();
        let ran_nodes_json = serialize_json(&ran_nodes_enc, "ran_nodes")?;
        let skipped_nodes_enc: Vec<String> = checkpoint
            .skipped_nodes
            .iter()
            .map(|k| k.encode())
            .collect();
        let skipped_nodes_json = serialize_json(&skipped_nodes_enc, "skipped_nodes")?;
        let updated_channels_json =
            serialize_json(&checkpoint.updated_channels, "updated_channels")?;

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CheckpointerError::Backend {
                message: format!("tx begin: {e}"),
            })?;

        // Check concurrency constraint if specified
        if let Some(expected_step) = expected_last_step {
            let current_step: Option<i64> =
                sqlx::query_scalar("SELECT last_step FROM sessions WHERE id = ?1")
                    .bind(&checkpoint.session_id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(|e| CheckpointerError::Backend {
                        message: format!("concurrency check: {e}"),
                    })?;

            match current_step {
                Some(step) if step != expected_step as i64 => {
                    return Err(CheckpointerError::Backend {
                        message: format!(
                            "concurrency conflict: expected step {}, found {}",
                            expected_step, step
                        ),
                    });
                }
                None if expected_step != 0 => {
                    return Err(CheckpointerError::Backend {
                        message: format!(
                            "concurrency conflict: session not found, expected step {}",
                            expected_step
                        ),
                    });
                }
                _ => {} // Check passed
            }
        }

        // Ensure session row exists
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO sessions (id, concurrency_limit)
            VALUES (?1, ?2)
        "#,
        )
        .bind(&checkpoint.session_id)
        .bind(checkpoint.concurrency_limit as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| CheckpointerError::Backend {
            message: format!("insert session: {e}"),
        })?;

        // Insert step row (fail if step already exists to prevent overwrites)
        sqlx::query(
            r#"
            INSERT INTO steps (
                session_id,
                step,
                state_json,
                frontier_json,
                versions_seen_json,
                ran_nodes_json,
                skipped_nodes_json,
                updated_channels_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
        )
        .bind(&checkpoint.session_id)
        .bind(checkpoint.step as i64)
        .bind(&state_json)
        .bind(&frontier_json)
        .bind(&versions_seen_json)
        .bind(&ran_nodes_json)
        .bind(&skipped_nodes_json)
        .bind(&updated_channels_json)
        .execute(&mut *tx)
        .await
        .map_err(|e| CheckpointerError::Backend {
            message: format!("insert step: {e}"),
        })?;

        tx.commit().await.map_err(|e| CheckpointerError::Backend {
            message: format!("tx commit: {e}"),
        })?;

        Ok(())
    }

    /// Helper to convert a database row to a Checkpoint.
    async fn row_to_checkpoint(
        &self,
        session_id: &str,
        row: &sqlx::sqlite::SqliteRow,
    ) -> Result<Checkpoint> {
        let step: i64 = row.get("step");
        let state_json: String = row.get("state_json");
        let frontier_json: String = row.get("frontier_json");
        let versions_seen_json: String = row.get("versions_seen_json");
        let ran_nodes_json: String = row.get("ran_nodes_json");
        let skipped_nodes_json: String = row.get("skipped_nodes_json");
        let updated_channels_json: String = row.get("updated_channels_json");
        let created_at_str: String = row.get("created_at");

        // Deserialize using persistence models
        let state_val: Value = deserialize_json(&state_json, "state")?;
        let frontier_val: Value = deserialize_json(&frontier_json, "frontier")?;
        let versions_seen_val: Value = deserialize_json(&versions_seen_json, "versions_seen")?;
        let ran_nodes_val: Value = deserialize_json(&ran_nodes_json, "ran_nodes")?;
        let skipped_nodes_val: Value = deserialize_json(&skipped_nodes_json, "skipped_nodes")?;
        let updated_channels_val: Value =
            deserialize_json(&updated_channels_json, "updated_channels")?;

        let persisted_state: PersistedState = deserialize_json_value(state_val, "state")?;
        let state =
            VersionedState::try_from(persisted_state).map_err(|e| CheckpointerError::Other {
                message: format!("state convert: {e}"),
            })?;

        let frontier: Vec<NodeKind> = frontier_val
            .as_array()
            .ok_or_else(|| CheckpointerError::Other {
                message: "frontier not array".to_string(),
            })?
            .iter()
            .filter_map(|v| v.as_str())
            .map(NodeKind::decode)
            .collect();

        let ran_nodes: Vec<NodeKind> = ran_nodes_val
            .as_array()
            .ok_or_else(|| CheckpointerError::Other {
                message: "ran_nodes not array".to_string(),
            })?
            .iter()
            .filter_map(|v| v.as_str())
            .map(NodeKind::decode)
            .collect();

        let skipped_nodes: Vec<NodeKind> = skipped_nodes_val
            .as_array()
            .ok_or_else(|| CheckpointerError::Other {
                message: "skipped_nodes not array".to_string(),
            })?
            .iter()
            .filter_map(|v| v.as_str())
            .map(NodeKind::decode)
            .collect();

        let updated_channels: Vec<String> = updated_channels_val
            .as_array()
            .ok_or_else(|| CheckpointerError::Other {
                message: "updated_channels not array".to_string(),
            })?
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.to_string())
            .collect();

        let persisted_vs: PersistedVersionsSeen =
            deserialize_json_value(versions_seen_val, "versions_seen")?;
        let versions_seen = persisted_vs.0;

        let created_at = DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        Ok(Checkpoint {
            session_id: session_id.to_string(),
            step: step as u64,
            state,
            frontier,
            versions_seen,
            concurrency_limit: 1, // Will need to be retrieved from session table if needed
            created_at,
            ran_nodes,
            skipped_nodes,
            updated_channels,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channels::Channel;
    use crate::runtimes::checkpointer::restore_session_state;
    use crate::runtimes::checkpointer::Checkpoint as CP;

    use crate::state::VersionedState;
    use rustc_hash::FxHashMap;
    use std::fs::File;
    use tempfile::TempDir;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_sqlite_checkpointer_roundtrip() {
        let cp = SQLiteCheckpointer::connect("sqlite::memory:")
            .await
            .expect("connect sqlite memory");
        let mut state = VersionedState::new_with_user_message("hello");
        state
            .extra
            .get_mut()
            .insert("k".into(), serde_json::json!(42));

        let mut versions_seen: FxHashMap<String, FxHashMap<String, u64>> = FxHashMap::default();
        versions_seen.insert(
            "Start".into(),
            FxHashMap::from_iter([("messages".into(), 1_u64), ("extra".into(), 1_u64)]),
        );

        let cp_struct = CP {
            session_id: "sessX".into(),
            step: 1,
            state: state.clone(),
            frontier: vec![NodeKind::End],
            versions_seen: versions_seen.clone(),
            concurrency_limit: 4,
            created_at: Utc::now(),
            ran_nodes: vec![NodeKind::Start],
            skipped_nodes: vec![],
            updated_channels: vec!["messages".to_string()],
        };

        // Save (async trait method)
        cp.save(cp_struct.clone()).await.expect("save");

        // Load
        let loaded = cp
            .load_latest("sessX")
            .await
            .expect("load_latest")
            .expect("Some checkpoint");
        assert_eq!(loaded.step, 1);
        assert_eq!(loaded.frontier, vec![NodeKind::End]);
        assert_eq!(
            loaded
                .versions_seen
                .get("Start")
                .and_then(|m| m.get("messages"))
                .copied(),
            Some(1)
        );
        assert_eq!(loaded.state.messages.snapshot()[0].role, "user");
        assert_eq!(
            loaded.state.extra.snapshot().get("k"),
            Some(&serde_json::json!(42))
        );

        // Restore session state utility compatibility
        let session_state = restore_session_state(&loaded);
        assert_eq!(session_state.step, 1);
        assert_eq!(session_state.frontier.len(), 1);
        assert_eq!(session_state.scheduler.concurrency_limit, 4);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_list_sessions() {
        let cp = SQLiteCheckpointer::connect("sqlite::memory:")
            .await
            .expect("connect");
        for i in 0..3 {
            let s_id = format!("s{i}");
            let state = VersionedState::new_with_user_message("x");
            let cp_struct = Checkpoint {
                session_id: s_id.clone(),
                step: 1,
                state: state.clone(),
                frontier: vec![NodeKind::End],
                versions_seen: FxHashMap::default(),
                concurrency_limit: 1,
                created_at: Utc::now(),
                ran_nodes: vec![],
                skipped_nodes: vec![NodeKind::End],
                updated_channels: vec![],
            };
            cp.save(cp_struct).await.unwrap();
        }
        let mut sessions = cp.list_sessions().await.unwrap();
        sessions.sort();
        assert_eq!(sessions, vec!["s0", "s1", "s2"]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_load_nonexistent() {
        let cp = SQLiteCheckpointer::connect("sqlite::memory:")
            .await
            .expect("connect");
        let res = cp.load_latest("nope").await.unwrap();
        assert!(res.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_resume_step_zero_checkpoint() {
        let temp_dir = TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("checkpoint.db");
        let db_url = format!("sqlite://{}", db_path.to_string_lossy());

        File::create(&db_path).expect("create db file");

        let cp_initial = SQLiteCheckpointer::connect(&db_url)
            .await
            .expect("connect initial");

        let state = VersionedState::new_with_user_message("hi");
        let checkpoint = Checkpoint {
            session_id: "sess0".into(),
            step: 0,
            state: state.clone(),
            frontier: vec![NodeKind::Start],
            versions_seen: FxHashMap::default(),
            concurrency_limit: 2,
            created_at: Utc::now(),
            ran_nodes: vec![],
            skipped_nodes: vec![],
            updated_channels: vec![],
        };

        cp_initial.save(checkpoint).await.expect("save step0");
        drop(cp_initial);

        let cp_reloaded = SQLiteCheckpointer::connect(&db_url)
            .await
            .expect("reconnect");
        let loaded = cp_reloaded
            .load_latest("sess0")
            .await
            .expect("load_latest result")
            .expect("checkpoint present");

        assert_eq!(loaded.step, 0);
        assert_eq!(loaded.frontier, vec![NodeKind::Start]);
        assert_eq!(loaded.state.messages.snapshot(), state.messages.snapshot(),);
        assert!(loaded.versions_seen.is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_step_execution_metadata() {
        let cp = SQLiteCheckpointer::connect("sqlite::memory:")
            .await
            .expect("connect");

        let state = VersionedState::new_with_user_message("test");
        let checkpoint = Checkpoint {
            session_id: "test_session".into(),
            step: 1,
            state: state.clone(),
            frontier: vec![NodeKind::End],
            versions_seen: FxHashMap::default(),
            concurrency_limit: 2,
            created_at: Utc::now(),
            ran_nodes: vec![NodeKind::Start, NodeKind::Other("TestNode".into())],
            skipped_nodes: vec![NodeKind::End],
            updated_channels: vec!["messages".to_string(), "extra".to_string()],
        };

        cp.save(checkpoint.clone()).await.expect("save checkpoint");

        // Query the step to verify execution metadata is preserved
        let query = StepQuery {
            limit: Some(10),
            ..Default::default()
        };

        let result = cp
            .query_steps("test_session", query)
            .await
            .expect("query steps");
        assert_eq!(result.checkpoints.len(), 1);

        let loaded = &result.checkpoints[0];
        assert_eq!(
            loaded.ran_nodes,
            vec![NodeKind::Start, NodeKind::Other("TestNode".into())]
        );
        assert_eq!(loaded.skipped_nodes, vec![NodeKind::End]);
        assert_eq!(
            loaded.updated_channels,
            vec!["messages".to_string(), "extra".to_string()]
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_query_steps_pagination() {
        let cp = SQLiteCheckpointer::connect("sqlite::memory:")
            .await
            .expect("connect");

        // Create multiple checkpoints
        for step in 1..=5 {
            let state = VersionedState::new_with_user_message(&format!("step {step}"));
            let checkpoint = Checkpoint {
                session_id: "paginate_session".into(),
                step,
                state,
                frontier: vec![NodeKind::End],
                versions_seen: FxHashMap::default(),
                concurrency_limit: 1,
                created_at: Utc::now(),
                ran_nodes: if step % 2 == 0 {
                    vec![NodeKind::Start]
                } else {
                    vec![]
                },
                skipped_nodes: vec![NodeKind::End],
                updated_channels: vec!["messages".to_string()],
            };
            cp.save(checkpoint).await.expect("save checkpoint");
        }

        // Test pagination with limit
        let query = StepQuery {
            limit: Some(2),
            offset: Some(0),
            ..Default::default()
        };

        let result = cp
            .query_steps("paginate_session", query)
            .await
            .expect("query steps");
        assert_eq!(result.page_info.total_count, 5);
        assert_eq!(result.page_info.page_size, 2);
        assert_eq!(result.page_info.offset, 0);
        assert!(result.page_info.has_next_page);
        assert_eq!(result.checkpoints.len(), 2);

        // Results should be in descending order (newest first)
        assert_eq!(result.checkpoints[0].step, 5);
        assert_eq!(result.checkpoints[1].step, 4);

        // Test second page
        let query = StepQuery {
            limit: Some(2),
            offset: Some(2),
            ..Default::default()
        };

        let result = cp
            .query_steps("paginate_session", query)
            .await
            .expect("query steps");
        assert_eq!(result.page_info.page_size, 2);
        assert_eq!(result.page_info.offset, 2);
        assert!(result.page_info.has_next_page);
        assert_eq!(result.checkpoints[0].step, 3);
        assert_eq!(result.checkpoints[1].step, 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_query_steps_filtering() {
        let cp = SQLiteCheckpointer::connect("sqlite::memory:")
            .await
            .expect("connect");

        // Create checkpoints with different characteristics
        for step in 1..=10 {
            let state = VersionedState::new_with_user_message(&format!("step {step}"));
            let checkpoint = Checkpoint {
                session_id: "filter_session".into(),
                step,
                state,
                frontier: vec![NodeKind::End],
                versions_seen: FxHashMap::default(),
                concurrency_limit: 1,
                created_at: Utc::now(),
                ran_nodes: if step <= 5 {
                    vec![NodeKind::Start]
                } else {
                    vec![NodeKind::Other("TestNode".into())]
                },
                skipped_nodes: vec![NodeKind::End],
                updated_channels: vec!["messages".to_string()],
            };
            cp.save(checkpoint).await.expect("save checkpoint");
        }

        // Test filtering by step range
        let query = StepQuery {
            min_step: Some(3),
            max_step: Some(7),
            ..Default::default()
        };

        let result = cp
            .query_steps("filter_session", query)
            .await
            .expect("query steps");
        assert_eq!(result.page_info.total_count, 5); // steps 3, 4, 5, 6, 7
        assert_eq!(result.checkpoints.len(), 5);

        // Test filtering by ran node
        let query = StepQuery {
            ran_node: Some(NodeKind::Start),
            ..Default::default()
        };

        let result = cp
            .query_steps("filter_session", query)
            .await
            .expect("query steps");
        assert_eq!(result.page_info.total_count, 5); // steps 1-5 ran Start node
        assert_eq!(result.checkpoints.len(), 5);

        // Test filtering by skipped node
        let query = StepQuery {
            skipped_node: Some(NodeKind::End),
            ..Default::default()
        };

        let result = cp
            .query_steps("filter_session", query)
            .await
            .expect("query steps");
        assert_eq!(result.page_info.total_count, 10); // All steps skipped End node
        assert_eq!(result.checkpoints.len(), 10);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_optimistic_concurrency_control() {
        let cp = SQLiteCheckpointer::connect("sqlite::memory:")
            .await
            .expect("connect");

        let state = VersionedState::new_with_user_message("test");

        // Save initial checkpoint (step 0)
        let checkpoint0 = Checkpoint {
            session_id: "concurrency_session".into(),
            step: 0,
            state: state.clone(),
            frontier: vec![NodeKind::Start],
            versions_seen: FxHashMap::default(),
            concurrency_limit: 1,
            created_at: Utc::now(),
            ran_nodes: vec![],
            skipped_nodes: vec![],
            updated_channels: vec![],
        };

        cp.save_with_concurrency_check(checkpoint0, None)
            .await
            .expect("save initial checkpoint");

        // Save step 1 expecting current step to be 0 (should succeed)
        let checkpoint1 = Checkpoint {
            session_id: "concurrency_session".into(),
            step: 1,
            state: state.clone(),
            frontier: vec![NodeKind::End],
            versions_seen: FxHashMap::default(),
            concurrency_limit: 1,
            created_at: Utc::now(),
            ran_nodes: vec![NodeKind::Start],
            skipped_nodes: vec![],
            updated_channels: vec!["messages".to_string()],
        };

        cp.save_with_concurrency_check(checkpoint1, Some(0))
            .await
            .expect("save step 1 with correct expectation");

        // Try to save step 2 expecting current step to be 0 (should fail)
        let checkpoint2 = Checkpoint {
            session_id: "concurrency_session".into(),
            step: 2,
            state,
            frontier: vec![NodeKind::End],
            versions_seen: FxHashMap::default(),
            concurrency_limit: 1,
            created_at: Utc::now(),
            ran_nodes: vec![NodeKind::Start],
            skipped_nodes: vec![],
            updated_channels: vec!["messages".to_string()],
        };

        let result = cp.save_with_concurrency_check(checkpoint2, Some(0)).await;
        assert!(result.is_err());

        if let Err(CheckpointerError::Backend { message }) = result {
            assert!(message.contains("concurrency conflict"));
            assert!(message.contains("expected step 0"));
            assert!(message.contains("found 1"));
        } else {
            panic!("Expected concurrency conflict error");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_concurrency_conflict_nonexistent_session() {
        let cp = SQLiteCheckpointer::connect("sqlite::memory:")
            .await
            .expect("connect");

        let state = VersionedState::new_with_user_message("test");
        let checkpoint = Checkpoint {
            session_id: "nonexistent_session".into(),
            step: 1,
            state,
            frontier: vec![NodeKind::End],
            versions_seen: FxHashMap::default(),
            concurrency_limit: 1,
            created_at: Utc::now(),
            ran_nodes: vec![NodeKind::Start],
            skipped_nodes: vec![],
            updated_channels: vec!["messages".to_string()],
        };

        // Try to save expecting step 5 when session doesn't exist (should fail)
        let result = cp.save_with_concurrency_check(checkpoint, Some(5)).await;
        assert!(result.is_err());

        if let Err(CheckpointerError::Backend { message }) = result {
            assert!(message.contains("concurrency conflict"));
            assert!(message.contains("session not found"));
            assert!(message.contains("expected step 5"));
        } else {
            panic!("Expected concurrency conflict error");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_step_query_empty_result() {
        let cp = SQLiteCheckpointer::connect("sqlite::memory:")
            .await
            .expect("connect");

        // Query non-existent session
        let query = StepQuery {
            limit: Some(10),
            ..Default::default()
        };

        let result = cp
            .query_steps("nonexistent_session", query)
            .await
            .expect("query steps");
        assert_eq!(result.page_info.total_count, 0);
        assert_eq!(result.page_info.page_size, 0);
        assert!(!result.page_info.has_next_page);
        assert!(result.checkpoints.is_empty());
    }
}
