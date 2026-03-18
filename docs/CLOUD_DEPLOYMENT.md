# Cloud Run Deployment Plan

Audit date: 2026-03-17

## Architecture (Recommended: Single Container)

One Cloud Run service, one container. The Rust server serves both the API and
the static SPA files. SQLite stays as-is with persistent volume storage.

```
Cloud Run (min-instances=1, max-instances=1)
  └── Single container
      ├── Rust binary: ramseynet-server
      │   ├── /api/*     → Axum API handlers
      │   └── /*         → Static SPA files (web/build/)
      └── /data/ramseynet.db  (Cloud Run Volume Mount)
```

## Current State (Audit Findings)

### What works today
- Server binds to `0.0.0.0` (correct for Cloud Run)
- Health endpoint at `/api/health` returns HTTP 200 JSON (suitable for probes)
- `rusqlite` uses `bundled` feature (compiles SQLite from source, no system dep)
- Web app is a pure static SPA (`adapter-static`, `ssr = false`)
- API uses relative paths (`/api/*`) — no hardcoded domain
- SQLite DB size will stay under 500MB even with heavy use

### What's missing
- No `PORT` env var support (Cloud Run requires this)
- No `DATABASE_PATH` env var support
- Server doesn't serve static files (API-only)
- No Dockerfile
- No `.dockerignore`
- CORS is fully permissive (`CorsLayer::permissive()`)
- No rate limiting
- Health check doesn't verify DB connectivity
- No request body size limits beyond Axum's 2MB default

## Code Changes Required

### 1. Server: `PORT` and `DATABASE_PATH` env var support

`crates/ramseynet-server/src/main.rs` — add `env` attribute to clap args:

```rust
#[arg(long, env = "PORT", default_value = "3001")]
port: u16,

#[arg(long, env = "DATABASE_PATH", default_value = "ramseynet.db")]
db_path: String,
```

### 2. Server: Serve static SPA files

Add `"fs"` feature to `tower-http` in workspace `Cargo.toml`:

```toml
tower-http = { version = "0.6", features = ["cors", "trace", "fs"] }
```

Add static file serving with SPA fallback in `crates/ramseynet-server/src/lib.rs`:

```rust
use tower_http::services::{ServeDir, ServeFile};

// In create_router():
let spa_dir = std::env::var("SPA_DIR").unwrap_or_else(|_| "web/build".to_string());
let spa = ServeDir::new(&spa_dir)
    .not_found_service(ServeFile::new(format!("{}/index.html", spa_dir)));

Router::new()
    .nest("/api", api_routes)
    .fallback_service(spa)
```

### 3. Security hardening

```rust
// Tighten CORS (replace CorsLayer::permissive())
use tower_http::cors::{CorsLayer, AllowOrigin};
let cors = CorsLayer::new()
    .allow_origin(AllowOrigin::exact("https://yourdomain.com".parse().unwrap()))
    .allow_methods([Method::GET, Method::POST])
    .allow_headers([CONTENT_TYPE]);

// Add body size limit
use axum::extract::DefaultBodyLimit;
.layer(DefaultBodyLimit::max(1024 * 1024)) // 1MB

// Rate limiting (consider tower::limit::RateLimitLayer or Cloud Armor)
```

### 4. Enhanced health check

```rust
async fn health(State(state): State<Arc<AppState>>) -> Json<Value> {
    let db_ok = tokio::task::spawn_blocking({
        let ledger = state.ledger.clone();
        move || ledger.health_check() // SELECT 1
    }).await.unwrap_or(false);

    Json(json!({
        "name": "MineGraph",
        "version": ramseynet_types::PROTOCOL_VERSION,
        "status": if db_ok { "ok" } else { "degraded" },
        "db": if db_ok { "connected" } else { "error" }
    }))
}
```

## Dockerfile

```dockerfile
# ── Build stage ──────────────────────────────────────────
FROM rust:1.83-bookworm AS builder

RUN apt-get update && apt-get install -y build-essential

# Install Node.js for web build
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y nodejs

WORKDIR /app
COPY . .

# Build Rust server
RUN cargo build --release -p ramseynet-server

# Build web SPA
WORKDIR /app/web
RUN npm ci && npm run build

# ── Runtime stage ────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/ramseynet-server /usr/local/bin/
COPY --from=builder /app/web/build /srv/web

ENV PORT=8080
ENV DATABASE_PATH=/data/ramseynet.db
ENV SPA_DIR=/srv/web
ENV RUST_LOG=info

EXPOSE 8080

CMD ["ramseynet-server"]
```

## .dockerignore

```
target/
node_modules/
.git/
logs/
*.db
*.db-wal
*.db-shm
web/.svelte-kit/
web/build/
```

## SQLite Persistent Storage Options

| Option | Cost/mo | WAL Support | Durability | Complexity |
|--------|---------|-------------|------------|------------|
| **GCS FUSE mount** | ~$0.03 | No (use DELETE journal) | Good | Low |
| **Filestore NFS** | ~$200 (1TB min) | Yes | Excellent | Medium |
| **Litestream → GCS** | ~$0.10 | Yes | Excellent | Medium |
| **Local disk + GCS backup** | ~$0.03 | Yes | Moderate | Low |

**Recommendation for MVP:** GCS FUSE with DELETE journal mode, or Litestream
for continuous replication with WAL mode.

If using GCS FUSE, change journal mode in `schema.rs`:
```sql
PRAGMA journal_mode=DELETE;  -- instead of WAL (GCS FUSE doesn't support WAL)
```

## GCP Deployment Steps

### Phase 1: Code changes
```bash
# 1. Add PORT/DATABASE_PATH env var support to server CLI
# 2. Add static file serving to server
# 3. Tighten CORS
# 4. Enhance health check
# 5. Create Dockerfile and .dockerignore
# 6. Test locally:
docker build -t minegraph .
docker run -p 8080:8080 -v $(pwd)/data:/data minegraph
```

### Phase 2: GCP setup
```bash
# Enable APIs
gcloud services enable run.googleapis.com artifactregistry.googleapis.com

# Create Artifact Registry repo
gcloud artifacts repositories create minegraph \
  --repository-format=docker \
  --location=us-central1

# Build and push
gcloud builds submit --tag us-central1-docker.pkg.dev/PROJECT/minegraph/server:latest

# Create GCS bucket for database (if using GCS FUSE)
gsutil mb gs://PROJECT-minegraph-data
```

### Phase 3: Deploy
```bash
gcloud run deploy minegraph \
  --image=us-central1-docker.pkg.dev/PROJECT/minegraph/server:latest \
  --port=8080 \
  --min-instances=1 \
  --max-instances=1 \
  --memory=512Mi \
  --cpu=1 \
  --set-env-vars="RUST_LOG=info" \
  --execution-environment=gen2 \
  --add-volume=name=dbvol,type=cloud-storage,bucket=PROJECT-minegraph-data \
  --add-volume-mount=volume=dbvol,mount-path=/data \
  --allow-unauthenticated
```

### Phase 4: Custom domain (optional)
```bash
gcloud run domain-mappings create --service=minegraph --domain=minegraph.example.com
```

## Security Checklist

- [ ] Restrict CORS to production domain
- [ ] Add rate limiting (Cloud Armor or tower middleware)
- [ ] Set explicit request body size limit (1MB)
- [ ] Add RGXF payload size limit
- [ ] Health check verifies DB connectivity
- [ ] No secrets in Docker image (use env vars / Secret Manager)
- [ ] HTTPS enforced (Cloud Run handles TLS termination)
- [ ] Consider API keys for write endpoints (`/api/submit`, `/api/keys`)
- [ ] Review metadata field for injection risks (already validated as JSON, max 4KB)

## Cost Estimate

| Resource | Monthly Cost |
|----------|-------------|
| Cloud Run (1 vCPU, 512MB, always-on) | $15-25 |
| GCS bucket (<1GB) | $0.03 |
| Artifact Registry | $0.10/GB |
| Cloud Build (occasional) | $0-5 |
| **Total** | **~$15-30/mo** |

## Scaling Considerations

The current architecture (SQLite, single instance) supports:
- ~1000 requests/sec for reads (leaderboard queries)
- ~100 submissions/sec for writes (verification is CPU-bound)
- Unlimited web SPA serving (static files, cacheable)

If horizontal scaling is needed later, the migration path is:
1. Replace `ramseynet-ledger` with PostgreSQL-backed implementation (~2-4 weeks)
2. Use Cloud SQL for PostgreSQL
3. Increase `max-instances` on Cloud Run
4. Add connection pooling (`sqlx::PgPool` or `deadpool-postgres`)

## Key Files Reference

| File | Relevance |
|------|-----------|
| `crates/ramseynet-server/src/main.rs` | CLI args, server startup |
| `crates/ramseynet-server/src/lib.rs` | Router, CORS, all API handlers |
| `crates/ramseynet-ledger/src/lib.rs` | DB connection, Mutex wrapper |
| `crates/ramseynet-ledger/src/schema.rs` | SQLite schema, migrations, PRAGMAs |
| `crates/ramseynet-ledger/src/queries.rs` | All SQL queries (SQLite dialect) |
| `web/svelte.config.js` | SvelteKit adapter-static config |
| `web/vite.config.ts` | Dev proxy config |
| `web/src/lib/api.ts` | API client (relative `/api` paths) |
