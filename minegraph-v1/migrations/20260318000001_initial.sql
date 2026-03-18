-- MineGraph v1 initial schema

-- Server identity (server's own keypair for signing receipts)
CREATE TABLE IF NOT EXISTS server_config (
    key   TEXT PRIMARY KEY,
    value JSONB NOT NULL
);

-- Registered identities (workers and other participants)
CREATE TABLE IF NOT EXISTS identities (
    key_id       TEXT PRIMARY KEY,           -- first 16 hex of blake3(pubkey)
    public_key   TEXT NOT NULL UNIQUE,       -- 32 bytes hex
    display_name TEXT,                        -- optional human-readable name
    github_repo  TEXT,                        -- optional github repo link
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Deduplicated graph storage
CREATE TABLE IF NOT EXISTS graphs (
    cid        TEXT PRIMARY KEY,             -- blake3 hash of canonical graph6 (hex)
    n          INTEGER NOT NULL,             -- vertex count
    graph6     TEXT NOT NULL,                -- canonical graph6 encoding
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_graphs_n ON graphs(n);

-- Graph submissions (every submission attempt)
CREATE TABLE IF NOT EXISTS submissions (
    id         BIGSERIAL PRIMARY KEY,
    cid        TEXT NOT NULL REFERENCES graphs(cid),
    key_id     TEXT NOT NULL REFERENCES identities(key_id),
    signature  TEXT NOT NULL,                -- hex-encoded Ed25519 signature
    metadata   JSONB,                        -- worker_id, commit_hash, etc.
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_submissions_cid ON submissions(cid);
CREATE INDEX IF NOT EXISTS idx_submissions_key_id ON submissions(key_id);
CREATE INDEX IF NOT EXISTS idx_submissions_created_at ON submissions(created_at);

-- Precomputed scores for each graph
CREATE TABLE IF NOT EXISTS scores (
    cid          TEXT PRIMARY KEY REFERENCES graphs(cid),
    n            INTEGER NOT NULL,
    histogram    JSONB NOT NULL,             -- [{k, red, blue}, ...]
    goodman_gap  DOUBLE PRECISION NOT NULL,
    aut_order    DOUBLE PRECISION NOT NULL,  -- |Aut(G)|
    score_bytes  BYTEA NOT NULL,             -- serialized lexicographic score for comparison
    computed_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_scores_n ON scores(n);

-- Leaderboard: ranked entries per vertex count n
CREATE TABLE IF NOT EXISTS leaderboard (
    n            INTEGER NOT NULL,
    rank         INTEGER NOT NULL,
    cid          TEXT NOT NULL REFERENCES graphs(cid),
    key_id       TEXT NOT NULL REFERENCES identities(key_id),
    score_bytes  BYTEA NOT NULL,             -- for fast comparison
    admitted_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (n, rank)
);

CREATE INDEX IF NOT EXISTS idx_leaderboard_n_score ON leaderboard(n, score_bytes);
CREATE INDEX IF NOT EXISTS idx_leaderboard_cid ON leaderboard(cid);

-- Verification receipts (signed by the server)
CREATE TABLE IF NOT EXISTS receipts (
    id             BIGSERIAL PRIMARY KEY,
    cid            TEXT NOT NULL REFERENCES graphs(cid),
    server_key_id  TEXT NOT NULL,            -- server's key_id
    verdict        TEXT NOT NULL,            -- 'accepted' or 'rejected'
    score_json     JSONB,                    -- full score breakdown
    signature      TEXT NOT NULL,            -- server's Ed25519 signature (hex)
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_receipts_cid ON receipts(cid);
