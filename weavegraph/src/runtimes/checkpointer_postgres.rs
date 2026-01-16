/*!
PostgreSQL Checkpointer

This module provides the `PostgresCheckpointer` async implementation of the
`Checkpointer` trait defined in `runtimes/checkpointer.rs`.

## Features

- **Complete Step History**: Stores full execution metadata including ran/skipped nodes
- **Pagination Support**: Efficient querying of large checkpoint histories
- **Optimistic Concurrency**: Prevention of concurrent checkpoint conflicts
- **Serde Integration**: Uses persistence models for consistent serialization

## Behavior

- Uses serde-based persistence models (see `runtimes::persistence`) for
  encoding `VersionedState`, frontier node kinds, and `versions_seen`.
- When the `postgres-migrations` feature is enabled, embedded
  migrations (`sqlx::migrate!("./migrations/postgres")`) are executed on connect;
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
- `steps.state_json` ← serialized `VersionedState` (JSONB)
- `steps.frontier_json` ← JSON array of encoded `NodeKind` (JSONB)
- `steps.versions_seen_json` ← JSON object (node → channel → version) (JSONB)
- `steps.ran_nodes_json` ← JSON array of executed nodes (JSONB)
- `steps.skipped_nodes_json` ← JSON array of skipped nodes (JSONB)
- `steps.updated_channels_json` ← JSON array of updated channel names (JSONB)

## NodeKind Encoding

NodeKinds are encoded as strings for JSON storage:
- `Start` → `"Start"`
- `End` → `"End"`
- `Custom(name)` → `"Custom:<name>"`
*/

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgPool, Row, postgres::PgRow};
use tracing::instrument;

use crate::{
    runtimes::checkpointer::{Checkpoint, Checkpointer, CheckpointerError, Result},
    runtimes::persistence::{PersistedState, PersistedVersionsSeen},
    state::VersionedState,
    types::NodeKind,
};

use super::checkpointer_postgres_helpers::{
    deserialize_json_value, require_json_field, serialize_json,
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

/// PostgreSQL-backed checkpointer with full step history.
///
/// Provides durable checkpoint storage with advanced querying capabilities
/// including pagination, filtering, and optimistic concurrency control.
///
/// # Storage Growth
///
/// This backend stores complete step history. Storage grows roughly with:
/// `(sessions × steps_per_session × state_size)`.
///
/// For long-running applications, plan periodic cleanup to control database size:
///
/// ## Option 1: Direct SQL maintenance (recommended)
///
/// ```bash
/// # Delete checkpoints older than 30 days
/// psql -c "DELETE FROM steps WHERE created_at < NOW() - INTERVAL '30 days'"
///
/// # Keep only latest 100 steps per session
/// psql -c "
///   DELETE FROM steps s
///   WHERE s.step NOT IN (
///     SELECT step FROM steps
///     WHERE session_id = s.session_id
///     ORDER BY step DESC
///     LIMIT 100
///   )
/// "
///
/// # Reclaim space
/// psql -c "VACUUM ANALYZE"
/// ```
///
/// ## Option 2: Application lifecycle management
///
/// Delete entire sessions when workflows complete or expire. The schema includes
/// timestamps (`created_at` on steps, `updated_at` on sessions) to facilitate
/// time-based policies.
pub struct PostgresCheckpointer {
    /// Shared PostgreSQL connection pool for concurrent checkpoint operations
    pool: Arc<PgPool>,
}

impl std::fmt::Debug for PostgresCheckpointer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresCheckpointer").finish()
    }
}

impl PostgresCheckpointer {
    /// Connect to a PostgreSQL database at `database_url`.
    /// Example URL: \"postgresql://user:password@localhost/weavegraph\"
    ///
    /// Returns a configured `PostgresCheckpointer` ready for use.
    #[must_use = "checkpointer must be used to persist state"]
    #[instrument(skip(database_url))]
    pub async fn connect(database_url: &str) -> std::result::Result<Self, CheckpointerError> {
        let pool = PgPool::connect(database_url)
            .await
            .map_err(|e| CheckpointerError::Backend {
                message: format!("connect error: {e}"),
            })?;
        // Run embedded migrations only if the feature is enabled (idempotent).
        #[cfg(feature = "postgres-migrations")]
        {
            if let Err(e) = sqlx::migrate!("./migrations/postgres").run(&pool).await {
                return Err(CheckpointerError::Backend {
                    message: format!("migration failure: {e}"),
                });
            }
        }
        #[cfg(not(feature = "postgres-migrations"))]
        {
            // Feature disabled: assume external migration orchestration already applied schema.
        }
        Ok(Self {
            pool: Arc::new(pool),
        })
    }
}

#[async_trait::async_trait]
impl Checkpointer for PostgresCheckpointer {
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

        // Ensure session row (upsert with ON CONFLICT DO NOTHING)
        sqlx::query(
            r#"
            INSERT INTO sessions (id, concurrency_limit)
            VALUES ($1, $2)
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        .bind(&checkpoint.session_id)
        .bind(checkpoint.concurrency_limit as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| CheckpointerError::Backend {
            message: format!("insert session: {e}"),
        })?;

        // Insert or replace step row (upsert for idempotent re-save of same step)
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
            ) VALUES ($1, $2, $3::jsonb, $4::jsonb, $5::jsonb, $6::jsonb, $7::jsonb, $8::jsonb)
            ON CONFLICT (session_id, step) DO UPDATE SET
                state_json = EXCLUDED.state_json,
                frontier_json = EXCLUDED.frontier_json,
                versions_seen_json = EXCLUDED.versions_seen_json,
                ran_nodes_json = EXCLUDED.ran_nodes_json,
                skipped_nodes_json = EXCLUDED.skipped_nodes_json,
                updated_channels_json = EXCLUDED.updated_channels_json
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

        // Maintain denormalized latest snapshot on sessions row.
        //
        // IMPORTANT: steps may be written out-of-order (replays/imports/retries),
        // so this update MUST be monotonic. We only advance last_* when the new
        // step is >= the currently recorded last_step.
        sqlx::query(
            r#"
            UPDATE sessions
            SET
                updated_at = NOW(),
                last_step = CASE WHEN last_step <= $2 THEN $2 ELSE last_step END,
                last_state_json = CASE WHEN last_step <= $2 THEN $3::jsonb ELSE last_state_json END,
                last_frontier_json = CASE WHEN last_step <= $2 THEN $4::jsonb ELSE last_frontier_json END,
                last_versions_seen_json = CASE WHEN last_step <= $2 THEN $5::jsonb ELSE last_versions_seen_json END
            WHERE id = $1
            "#,
        )
        .bind(&checkpoint.session_id)
        .bind(checkpoint.step as i64)
        .bind(&state_json)
        .bind(&frontier_json)
        .bind(&versions_seen_json)
        .execute(&mut *tx)
        .await
        .map_err(|e| CheckpointerError::Backend {
            message: format!("update session latest: {e}"),
        })?;

        tx.commit().await.map_err(|e| CheckpointerError::Backend {
            message: format!("tx commit: {e}"),
        })?;

        Ok(())
    }

    #[instrument(skip(self, session_id), err)]
    async fn load_latest(&self, session_id: &str) -> Result<Option<Checkpoint>> {
        let row_opt: Option<PgRow> = sqlx::query(
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
            WHERE s.id = $1
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

        let state_json: Option<Value> =
            row.try_get("last_state_json")
                .map_err(|e| CheckpointerError::Backend {
                    message: format!("last_state_json read: {e}"),
                })?;
        let frontier_json: Option<Value> =
            row.try_get("last_frontier_json")
                .map_err(|e| CheckpointerError::Backend {
                    message: format!("last_frontier_json read: {e}"),
                })?;
        let versions_seen_json: Option<Value> =
            row.try_get("last_versions_seen_json")
                .map_err(|e| CheckpointerError::Backend {
                    message: format!("last_versions_seen_json read: {e}"),
                })?;
        let concurrency_limit: i64 = row.get("concurrency_limit");
        let updated_at: DateTime<Utc> = row.get("updated_at");

        if last_step == 0 && state_json.is_none() {
            // Session row exists but no checkpoint has been persisted yet.
            return Ok(None);
        }

        let state_val = require_json_field(state_json, "state_json")?;
        let frontier_val = require_json_field(frontier_json, "frontier_json")?;
        let versions_seen_val = require_json_field(versions_seen_json, "versions_seen_json")?;

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

        Ok(Some(Checkpoint {
            session_id: session_id.to_string(),
            step: last_step as u64,
            state,
            frontier,
            versions_seen,
            concurrency_limit: concurrency_limit as usize,
            created_at: updated_at,
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

// Extended PostgresCheckpointer methods (not part of base Checkpointer trait)
impl PostgresCheckpointer {
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
    /// use weavegraph::runtimes::checkpointer_postgres::{PostgresCheckpointer, StepQuery};
    /// use weavegraph::types::NodeKind;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let checkpointer = PostgresCheckpointer::connect("postgresql://localhost/app").await?;
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
        let mut conditions = vec!["st.session_id = $1".to_string()];
        let mut param_count = 1;

        if query.min_step.is_some() {
            param_count += 1;
            conditions.push(format!("st.step >= ${param_count}"));
        }
        if query.max_step.is_some() {
            param_count += 1;
            conditions.push(format!("st.step <= ${param_count}"));
        }
        if query.ran_node.is_some() {
            param_count += 1;
            conditions.push(format!("st.ran_nodes_json @> ${param_count}::jsonb"));
        }
        if query.skipped_node.is_some() {
            param_count += 1;
            conditions.push(format!("st.skipped_nodes_json @> ${param_count}::jsonb"));
        }

        let where_clause = conditions.join(" AND ");

        // Count total matching records
        let count_sql = format!("SELECT COUNT(*) as total FROM steps st WHERE {where_clause}");

        let limit = query.limit.unwrap_or(100).min(1000); // Cap at 1000
        let offset = query.offset.unwrap_or(0);

        // Query with pagination
        let select_sql = format!(
            r#"SELECT
                st.session_id,
                st.step,
                st.state_json,
                st.frontier_json,
                st.versions_seen_json,
                st.ran_nodes_json,
                st.skipped_nodes_json,
                st.updated_channels_json,
                st.created_at,
                s.concurrency_limit
               FROM steps st
               JOIN sessions s ON s.id = st.session_id
               WHERE {where_clause}
               ORDER BY st.step DESC
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
            count_query = count_query.bind(serde_json::json!([ran_node.encode()]));
        }
        if let Some(skipped_node) = &query.skipped_node {
            count_query = count_query.bind(serde_json::json!([skipped_node.encode()]));
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
            select_query = select_query.bind(serde_json::json!([ran_node.encode()]));
        }
        if let Some(skipped_node) = &query.skipped_node {
            select_query = select_query.bind(serde_json::json!([skipped_node.encode()]));
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
            let checkpoint = self.row_to_checkpoint(session_id, &row)?;
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
    /// use weavegraph::runtimes::checkpointer_postgres::PostgresCheckpointer;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let checkpointer = PostgresCheckpointer::connect("postgresql://localhost/app").await?;
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

        // Ensure session row exists
        sqlx::query(
            r#"
            INSERT INTO sessions (id, concurrency_limit)
            VALUES ($1, $2)
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        .bind(&checkpoint.session_id)
        .bind(checkpoint.concurrency_limit as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| CheckpointerError::Backend {
            message: format!("insert session: {e}"),
        })?;

        // Check concurrency constraint if specified.
        //
        // We take a row-level lock so concurrent writers serialize their checks.
        if let Some(expected_step) = expected_last_step {
            let current_step: i64 =
                sqlx::query_scalar("SELECT last_step FROM sessions WHERE id = $1 FOR UPDATE")
                    .bind(&checkpoint.session_id)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|e| CheckpointerError::Backend {
                        message: format!("concurrency check: {e}"),
                    })?;

            if current_step != expected_step as i64 {
                return Err(CheckpointerError::Backend {
                    message: format!(
                        "concurrency conflict: expected step {}, found {}",
                        expected_step, current_step
                    ),
                });
            }
        }

        // Insert or replace step row (upsert for idempotent re-save of same step)
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
            ) VALUES ($1, $2, $3::jsonb, $4::jsonb, $5::jsonb, $6::jsonb, $7::jsonb, $8::jsonb)
            ON CONFLICT (session_id, step) DO UPDATE SET
                state_json = EXCLUDED.state_json,
                frontier_json = EXCLUDED.frontier_json,
                versions_seen_json = EXCLUDED.versions_seen_json,
                ran_nodes_json = EXCLUDED.ran_nodes_json,
                skipped_nodes_json = EXCLUDED.skipped_nodes_json,
                updated_channels_json = EXCLUDED.updated_channels_json
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

        // Maintain denormalized latest snapshot (monotonic).
        sqlx::query(
            r#"
            UPDATE sessions
            SET
                updated_at = NOW(),
                last_step = CASE WHEN last_step <= $2 THEN $2 ELSE last_step END,
                last_state_json = CASE WHEN last_step <= $2 THEN $3::jsonb ELSE last_state_json END,
                last_frontier_json = CASE WHEN last_step <= $2 THEN $4::jsonb ELSE last_frontier_json END,
                last_versions_seen_json = CASE WHEN last_step <= $2 THEN $5::jsonb ELSE last_versions_seen_json END
            WHERE id = $1
            "#,
        )
        .bind(&checkpoint.session_id)
        .bind(checkpoint.step as i64)
        .bind(&state_json)
        .bind(&frontier_json)
        .bind(&versions_seen_json)
        .execute(&mut *tx)
        .await
        .map_err(|e| CheckpointerError::Backend {
            message: format!("update session latest: {e}"),
        })?;

        tx.commit().await.map_err(|e| CheckpointerError::Backend {
            message: format!("tx commit: {e}"),
        })?;

        Ok(())
    }

    /// Helper to convert a database row to a Checkpoint.
    fn row_to_checkpoint(&self, session_id: &str, row: &PgRow) -> Result<Checkpoint> {
        let step: i64 = row.get("step");
        let state_json: Value = row.get("state_json");
        let frontier_json: Value = row.get("frontier_json");
        let versions_seen_json: Value = row.get("versions_seen_json");
        let ran_nodes_json: Value = row.get("ran_nodes_json");
        let skipped_nodes_json: Value = row.get("skipped_nodes_json");
        let updated_channels_json: Option<Value> =
            row.try_get("updated_channels_json")
                .map_err(|e| CheckpointerError::Backend {
                    message: format!("updated_channels_json read: {e}"),
                })?;
        let created_at: DateTime<Utc> = row.get("created_at");
        let concurrency_limit: i64 = row.get("concurrency_limit");

        // Deserialize using persistence models
        let persisted_state: PersistedState = deserialize_json_value(state_json, "state")?;
        let state =
            VersionedState::try_from(persisted_state).map_err(|e| CheckpointerError::Other {
                message: format!("state convert: {e}"),
            })?;

        let frontier: Vec<NodeKind> = frontier_json
            .as_array()
            .ok_or_else(|| CheckpointerError::Other {
                message: "frontier not array".to_string(),
            })?
            .iter()
            .filter_map(|v| v.as_str())
            .map(NodeKind::decode)
            .collect();

        let ran_nodes: Vec<NodeKind> = ran_nodes_json
            .as_array()
            .ok_or_else(|| CheckpointerError::Other {
                message: "ran_nodes not array".to_string(),
            })?
            .iter()
            .filter_map(|v| v.as_str())
            .map(NodeKind::decode)
            .collect();

        let skipped_nodes: Vec<NodeKind> = skipped_nodes_json
            .as_array()
            .ok_or_else(|| CheckpointerError::Other {
                message: "skipped_nodes not array".to_string(),
            })?
            .iter()
            .filter_map(|v| v.as_str())
            .map(NodeKind::decode)
            .collect();

        let updated_channels: Vec<String> = match updated_channels_json {
            None => vec![],
            Some(v) => v
                .as_array()
                .ok_or_else(|| CheckpointerError::Other {
                    message: "updated_channels not array".to_string(),
                })?
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect(),
        };

        let persisted_vs: PersistedVersionsSeen =
            deserialize_json_value(versions_seen_json, "versions_seen")?;
        let versions_seen = persisted_vs.0;

        Ok(Checkpoint {
            session_id: session_id.to_string(),
            step: step as u64,
            state,
            frontier,
            versions_seen,
            concurrency_limit: concurrency_limit as usize,
            created_at,
            ran_nodes,
            skipped_nodes,
            updated_channels,
        })
    }
}
