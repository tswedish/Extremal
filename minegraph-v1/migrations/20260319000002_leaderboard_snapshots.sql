-- Periodic leaderboard score snapshots for history tracking.
-- Captured by a server background task every ~10 minutes.

CREATE TABLE IF NOT EXISTS leaderboard_snapshots (
    id          BIGSERIAL PRIMARY KEY,
    n           INTEGER NOT NULL,
    entry_count INTEGER NOT NULL,
    -- Score statistics across all entries at snapshot time
    best_gap    DOUBLE PRECISION,   -- min goodman_gap
    worst_gap   DOUBLE PRECISION,   -- max goodman_gap
    median_gap  DOUBLE PRECISION,
    avg_gap     DOUBLE PRECISION,
    best_aut    DOUBLE PRECISION,   -- max aut_order (best symmetry)
    avg_aut     DOUBLE PRECISION,
    -- Aggregate clique counts (sum across all entries)
    total_k4_red   BIGINT DEFAULT 0,
    total_k4_blue  BIGINT DEFAULT 0,
    total_k5_red   BIGINT DEFAULT 0,
    total_k5_blue  BIGINT DEFAULT 0,
    -- Metadata
    snapshot_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_snapshots_n_time ON leaderboard_snapshots(n, snapshot_at);
