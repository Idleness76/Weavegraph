-- 0001_init.sql
--
-- Initial SQLite schema for Weavegraph session & step checkpointing.
-- This supports a future `SQLiteCheckpointer` implementation that can:
--   * Create / resume sessions by `session_id`
--   * Persist a full durable checkpoint after every barrier (superstep)
--   * Query historical steps (for audit, replay, diffing, debugging)
--
-- Design notes (aligned with runtimes/checkpointer.rs & runner.rs types):
--   Checkpoint fields we persist per step:
--     - session_id (string)
--     - step (u64 -> INTEGER)
--     - state (VersionedState)            -> JSON (TEXT)
--     - frontier (Vec<NodeKind>)          -> JSON (TEXT)
--     - versions_seen (HashMap<..>)       -> JSON (TEXT)
--     - ran_nodes / skipped_nodes         -> JSON (TEXT) (from StepReport)
--     - updated_channels                  -> JSON (TEXT) (Vec<&'static str>)
--     - created_at timestamp
--     - (Optionally) concurrency_limit is denormalized at the session level
--
--   We also keep a denormalized "latest" snapshot on the `sessions` row so
--   resuming a session can be a single SELECT (without an aggregate).
--
--   JSON is stored as TEXT (SQLite default). The application layer (SQLx) is
--   responsible for (de)serialization and validation. We may later add CHECK
--   constraints using json_valid(...) if desired (requires JSON1 extension).
--
--   Timestamps are stored in RFC3339/ISO8601 (UTC) via strftime. All times UTC.
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
--
PRAGMA foreign_keys = ON;

---------------------------------------------------------------------------
-- Sessions
---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS sessions (
    id                       TEXT PRIMARY KEY, -- session_id
    created_at               TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at               TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),

    -- Concurrency limit used when the session was created (for reference / resume)
    concurrency_limit        INTEGER NOT NULL,

    -- Denormalized latest checkpoint snapshot (mirrors most recent row in steps)
    last_step                INTEGER NOT NULL DEFAULT 0,
    last_state_json          TEXT,   -- Full VersionedState JSON (messages, extra, versions)
    last_frontier_json       TEXT,   -- JSON array of node kinds
    last_versions_seen_json  TEXT    -- JSON object: { "<node_id>": { "messages": <u64>, "extra": <u64>, ... } }
);

CREATE INDEX IF NOT EXISTS idx_sessions_updated_at ON sessions(updated_at DESC);

---------------------------------------------------------------------------
-- Steps (historical checkpoints)
---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS steps (
    session_id             TEXT    NOT NULL,
    step                   INTEGER NOT NULL,
    created_at             TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),

    -- Durable snapshot data
    state_json             TEXT    NOT NULL, -- Full VersionedState JSON
    frontier_json          TEXT    NOT NULL, -- JSON array
    versions_seen_json     TEXT    NOT NULL, -- JSON object of objects

    -- Execution metadata (from StepReport)
    ran_nodes_json         TEXT    NOT NULL, -- JSON array
    skipped_nodes_json     TEXT    NOT NULL, -- JSON array
    updated_channels_json  TEXT,             -- JSON array of updated channel names (may be empty/NULL)

    -- Optional future fields (placeholders for forward-compat):
    -- error_json          TEXT,  -- structured error info if barrier failed
    -- pause_reason_json   TEXT,  -- if an interrupt paused the session at this step

    PRIMARY KEY (session_id, step),
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_steps_session_step_desc
    ON steps(session_id, step DESC);

-- Fast access to chronological iteration (ascending)
CREATE INDEX IF NOT EXISTS idx_steps_session_step_asc
    ON steps(session_id, step ASC);

---------------------------------------------------------------------------
-- Triggers
---------------------------------------------------------------------------

-- Keep sessions.updated_at & denormalized latest snapshot in sync on insert.
CREATE TRIGGER IF NOT EXISTS trg_steps_after_insert
AFTER INSERT ON steps
BEGIN
    UPDATE sessions
    SET
        updated_at              = strftime('%Y-%m-%dT%H:%M:%fZ','now'),
        last_step               = NEW.step,
        last_state_json         = NEW.state_json,
        last_frontier_json      = NEW.frontier_json,
        last_versions_seen_json = NEW.versions_seen_json
    WHERE id = NEW.session_id;
END;

-- (Optional) If you later allow updating a step row (should be rare),
-- keep the denormalized snapshot accurate only when updating the latest step.
CREATE TRIGGER IF NOT EXISTS trg_steps_after_update
AFTER UPDATE ON steps
WHEN (SELECT last_step FROM sessions WHERE id = NEW.session_id) = NEW.step
BEGIN
    UPDATE sessions
    SET
        updated_at              = strftime('%Y-%m-%dT%H:%M:%fZ','now'),
        last_state_json         = NEW.state_json,
        last_frontier_json      = NEW.frontier_json,
        last_versions_seen_json = NEW.versions_seen_json
    WHERE id = NEW.session_id;
END;

---------------------------------------------------------------------------
-- Views (Convenience)
---------------------------------------------------------------------------

-- Latest checkpoint per session (essentially mirrors sessions.* but sourced
-- from authoritative steps table if you prefer not to trust denormalized columns).
CREATE VIEW IF NOT EXISTS v_latest_checkpoints AS
SELECT
    s.id AS session_id,
    s.concurrency_limit,
    s.created_at AS session_created_at,
    s.updated_at AS session_updated_at,
    st.step,
    st.created_at AS step_created_at,
    st.state_json,
    st.frontier_json,
    st.versions_seen_json,
    st.ran_nodes_json,
    st.skipped_nodes_json,
    st.updated_channels_json
FROM sessions s
LEFT JOIN steps st
  ON st.session_id = s.id
 AND st.step = s.last_step;

---------------------------------------------------------------------------
-- Integrity / Sanity Notes (enforced at application layer for now):
--   * step must be monotonic increasing per session (PRIMARY KEY + app logic)
--   * versions_seen_json should contain only non-negative integers
--   * frontier_json, ran_nodes_json, skipped_nodes_json are arrays of node encodings
--   * state_json must contain messages + extra + version metadata
--
-- Future migration ideas:
--   * Move JSON to FTS5 virtual tables for semantic search over messages
--   * Add error / pause tables for richer observability
--   * Differential checkpoints (store deltas after initial baseline)
--
-- End of migration.
