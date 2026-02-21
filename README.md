# RamseyNet

A permissionless protocol for distributed Ramsey graph search and deterministic generative graph art.

RamseyNet is a peer-to-peer network where anyone can propose and verify Ramsey graphs, persist artifacts in content-addressed storage, and derive public leaderboards and Pareto frontiers without central control.

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (stable, with `wasm32-wasip1` target)
- [Node.js](https://nodejs.org/) 20+
- [pnpm](https://pnpm.io/) 9+

### Build & Run

```bash
# Build all Rust crates
cargo build --all

# Run tests
cargo test --all

# Start the API server
cargo run -p ramseynet-server

# In another terminal, start the web app
cd web
pnpm install
pnpm dev
```

The server runs on `http://localhost:3001` and the web app on `http://localhost:5173`.

## Project Structure

```
crates/
  ramseynet-types/      Shared protocol types
  ramseynet-graph/      RGXF graph encoding + content addressing
  ramseynet-verifier/   Ramsey verifier (native + WASM)
  ramseynet-ledger/     SQLite ledger for transactions
  ramseynet-server/     Axum HTTP/WebSocket server
  ramseynet-search/     Graph search heuristics
web/                    SvelteKit frontend
test-vectors/           Shared test data
docs/                   Whitepaper and specs
```

## License

MIT
