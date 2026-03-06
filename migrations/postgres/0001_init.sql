-- 0001_init.sql
--
-- Initial PostgreSQL schema for Weavegraph session & step checkpointing.
-- This supports the `PostgresCheckpointer` implementation that can:
--   * Create / resume sessions by `session_id`
--   * Persist a full durable checkpoint after every barrier (superstep)
--   * Query historical steps (for audit, replay, diffing, debugging)
--
-- Design notes (aligned with runtimes/checkpointer.rs & runner.rs types):
--   Checkpoint fields we persist per step:
--     - session_id (string)
--     - step (u64 -> BIGINT)
--     - state (VersionedState)            -> JSONB
--     - frontier (Vec<NodeKind>)          -> JSONB
--     - versions_seen (HashMap<..>)       -> JSONB
--     - ran_nodes / skipped_nodes         -> JSONB (from StepReport)
--     - updated_channels                  -> JSONB (Vec<&'static str>)
--     - created_at timestamp
--     - (Optionally) concurrency_limit is denormalized at the session level
--
--   We also keep a denormalized "latest" snapshot on the `sessions` row so
--   resuming a session can be a single SELECT (without an aggregate).
--
--   JSONB is used for efficient storage and querying of JSON data.
--
--   Timestamps use TIMESTAMPTZ for timezone-aware storage (all times UTC).
--
--   Foreign keys are enforced (ON DELETE CASCADE ensures step history is removed
--   when a session is deleted).
--
--   Step numbering starts at 1 (after first barrier) though the schema does not
--   enforce an origin; the runner should ensure monotonic increment.
--
--   NodeKind serialization suggestion (not enforced here):
--     Start  -> "Start"
--     End    -> "End"
--     Other  -> {"Other":"<string>"}
--   or a simpler flat string encoding: "Start", "End", "Other:<name>"
--   (Must be consistent across state/frontier/ran/skipped arrays.)

---------------------------------------------------------------------------
-- Sessions
---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS sessions (
    id                       TEXT PRIMARY KEY, -- session_id
    created_at               TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at               TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Concurrency limit used when the session was created (for reference / resume)
    concurrency_limit        BIGINT NOT NULL,

    -- Denormalized latest checkpoint snapshot (mirrors most recent row in steps)
    last_step                BIGINT NOT NULL DEFAULT 0,
    last_state_json          JSONB,   -- Full VersionedState JSON (messages, extra, versions)
    last_frontier_json       JSONB,   -- JSON array of node kinds
    last_versions_seen_json  JSONB    -- JSON object: { "<node_id>": { "messages": <u64>, "extra": <u64>, ... } }
);

CREATE INDEX IF NOT EXISTS idx_sessions_updated_at ON sessions(updated_at DESC);

---------------------------------------------------------------------------
-- Steps (historical checkpoints)
---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS steps (
    session_id             TEXT    NOT NULL,
    step                   BIGINT  NOT NULL,
    created_at             TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Durable snapshot data
    state_json             JSONB   NOT NULL, -- Full VersionedState JSON
    frontier_json          JSONB   NOT NULL, -- JSON array
    versions_seen_json     JSONB   NOT NULL, -- JSON object of objects

    -- Execution metadata (from StepReport)
    ran_nodes_json         JSONB   NOT NULL, -- JSON array
    skipped_nodes_json     JSONB   NOT NULL, -- JSON array
    updated_channels_json  JSONB,            -- JSON array of updated channel names (may be empty/NULL)

    -- Optional future fields (placeholders for forward-compat):
    -- error_json          JSONB,  -- structured error info if barrier failed
    -- pause_reason_json   JSONB,  -- if an interrupt paused the session at this step

    PRIMARY KEY (session_id, step),
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_steps_session_step_desc
    ON steps(session_id, step DESC);

-- Fast access to chronological iteration (ascending)
CREATE INDEX IF NOT EXISTS idx_steps_session_step_asc
    ON steps(session_id, step ASC);

-- JSONB indexing for efficient containment queries used by query_steps()
-- (e.g. ran_nodes_json @> '["Start"]')
CREATE INDEX IF NOT EXISTS idx_steps_ran_nodes_gin
    ON steps USING GIN (ran_nodes_json);

CREATE INDEX IF NOT EXISTS idx_steps_skipped_nodes_gin
    ON steps USING GIN (skipped_nodes_json);


-- Denormalized session snapshot maintenance
--
-- We intentionally do NOT use database triggers to maintain sessions.last_*.
-- The application updates those fields in the same transaction as step writes.
-- This makes the "latest" pointer monotonic by construction even if steps are
-- written out-of-order (replays/imports/retries).

---------------------------------------------------------------------------
-- Integrity / Sanity Notes (enforced at application layer for now):
--   * step must be monotonic increasing per session (PRIMARY KEY + app logic)
--   * versions_seen_json should contain only non-negative integers
--   * frontier_json, ran_nodes_json, skipped_nodes_json are arrays of node encodings
--   * state_json must contain messages + extra + version metadata
--
-- Future migration ideas:
--   * Move JSON to full-text search for semantic search over messages
--   * Add error / pause tables for richer observability
--   * Differential checkpoints (store deltas after initial baseline)
--
-- End of migration.
