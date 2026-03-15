#!/usr/bin/env bash
# RamseyNet dev helper
#
# Native WSL2 usage (recommended):
#   bash scripts/wsl-dev.sh <command>
#
# From Windows PowerShell:
#   wsl.exe -d Ubuntu -e bash scripts/wsl-dev.sh <command>
#
# Commands:
#   test       - cargo test --all
#   clippy     - cargo clippy --all-targets -- -D warnings
#   build      - cargo build --all
#   web        - pnpm install && pnpm build (in web/)
#   web-dev    - pnpm dev (live reload on :5173)
#   ci         - run full CI suite (clippy + test + web build)
#   server     - start the API server on port 3001
#   server-log - start the API server with file logging (logs/)
#   search     - start the search worker (requires --k, --ell, --n)
#   seed       - seed the database with test graphs

set -euo pipefail

# Resolve repo root from script location (works for any user/path)
REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"

# Ensure cargo is in PATH
source "$HOME/.cargo/env" 2>/dev/null || true

cmd="${1:-ci}"
shift 2>/dev/null || true

# Parse --release flag (can appear anywhere in the remaining args)
CARGO_PROFILE=""
PROFILE_LABEL="dev"
remaining_args=()
for arg in "$@"; do
  if [ "$arg" = "--release" ]; then
    CARGO_PROFILE="--release"
    PROFILE_LABEL="release"
  else
    remaining_args+=("$arg")
  fi
done
set -- "${remaining_args[@]+"${remaining_args[@]}"}"

case "$cmd" in
  test)
    echo "=== Running tests ($PROFILE_LABEL) ==="
    cargo test --all $CARGO_PROFILE "$@"
    ;;
  clippy)
    echo "=== Running clippy ==="
    cargo clippy --all-targets -- -D warnings "$@"
    ;;
  build)
    echo "=== Building all crates ($PROFILE_LABEL) ==="
    cargo build --all $CARGO_PROFILE "$@"
    ;;
  web)
    echo "=== Building web app ==="
    cd web
    CI=true pnpm install
    pnpm build
    ;;
  web-dev)
    echo "=== Starting web dev server ==="
    cd web
    pnpm install --silent
    pnpm dev "$@"
    ;;
  ci)
    echo "=== Full CI suite ==="
    echo "--- clippy ---"
    cargo clippy --all-targets -- -D warnings
    echo "--- tests ---"
    cargo test --all
    echo "--- web build ---"
    cd web
    CI=true pnpm install
    pnpm build
    echo "=== CI passed! ==="
    ;;
  server)
    echo "=== Starting server ($PROFILE_LABEL) ==="
    cargo run $CARGO_PROFILE -p ramseynet-server -- "$@"
    ;;
  server-log)
    LOGDIR="$REPO/logs"
    mkdir -p "$LOGDIR"
    TIMESTAMP=$(date +%Y%m%d-%H%M%S)
    LOGFILE="$LOGDIR/server-$TIMESTAMP.log"
    echo "=== Starting server ($PROFILE_LABEL, logging to $LOGFILE) ==="
    RUST_LOG=info cargo run $CARGO_PROFILE -p ramseynet-server -- "$@" 2>&1 | tee "$LOGFILE"
    ;;
  search)
    echo "=== Starting search worker ($PROFILE_LABEL) ==="
    cargo run $CARGO_PROFILE -p ramseynet-worker -- "$@"
    ;;
  bench)
    echo "=== Running benchmarks ==="
    cargo bench -p ramseynet-verifier "$@"
    ;;
  seed)
    echo "=== Seeding ledger ==="
    bash "$REPO/scripts/seed-ledger.sh" "$@"
    ;;
  e2e)
    echo "=== Running E2E tests ==="
    bash "$REPO/scripts/e2e-test.sh" "$@"
    ;;
  *)
    echo "Usage: wsl-dev.sh {test|clippy|build|web|web-dev|ci|server|server-log|search|bench|seed|e2e}"
    echo "  Add --release for optimized builds (server, search, build, test)"
    exit 1
    ;;
esac
