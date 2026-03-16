# MineGraph Signing System — Design Document

Status: **Design only** — not yet implemented. Build after current experiment
cycle completes and strategy improvements stabilize.

## Goal

Attribute graph discoveries to specific identities. When the leaderboard is
public, submitters get credit for their finds. Unsigned submissions remain
valid but show as "anon."

## Non-goals for v1

- Key encryption / passphrases (friction for automated workers)
- HSM / KMS / remote signing
- SSH key import
- Agent / signing daemon
- Key revocation UI (just delete from DB if needed)
- Per-worker key management
- Teams / organizations

## Identity model

An identity is an **Ed25519 keypair**. That's it.

- **Key ID**: first 16 hex chars of SHA-256(public_key_bytes). Deterministic,
  no registration required for basic use.
- **Public key**: 32 bytes, hex-encoded for display and storage.
- **Secret key**: 64 bytes (ed25519 expanded), stored as a local file.
- **Display name**: optional, set at registration time. Falls back to key ID.

## Signing protocol

### What gets signed

A **canonical submission payload** — the exact bytes that determine the graph
identity. Sorted JSON keys, no whitespace:

```json
{"bits_b64":"...","encoding":"utri_b64_v1","k":5,"ell":5,"n":25}
```

This is the RGXF encoding of the canonical (nauty-relabeled) graph plus the
Ramsey parameters. The CID is derived from the graph bits, so signing this
implicitly covers the CID.

### Signature format

Ed25519 detached signature (64 bytes), hex-encoded in the submission payload:

```json
{
  "k": 5,
  "ell": 5,
  "n": 25,
  "graph": { "n": 25, "encoding": "utri_b64_v1", "bits_b64": "..." },
  "signature": "a1b2c3...64_bytes_hex...",
  "key_id": "3f8a1b2c4d5e6f70"
}
```

If `signature` and `key_id` are absent, the submission is anonymous.

### Verification

Server-side verification:

1. If `signature` and `key_id` are present in the submit request:
   a. Look up the public key by `key_id` in the `identities` table
   b. If not found: accept submission but mark as "unverified_signature"
   c. If found: verify the Ed25519 signature against the canonical payload
   d. If valid: record the `key_id` on the submission and leaderboard entry
   e. If invalid: reject the submission with 400 (bad signature)
2. If neither is present: accept as anonymous ("anon")

The server never requires signatures. Unsigned submissions are first-class.

## Schema changes

### New table: `identities`

```sql
CREATE TABLE IF NOT EXISTS identities (
    key_id       TEXT PRIMARY KEY,          -- first 16 hex of SHA-256(pubkey)
    public_key   TEXT NOT NULL UNIQUE,      -- 32 bytes hex
    display_name TEXT,                       -- optional human-readable name
    created_at   TEXT NOT NULL               -- ISO 8601
);
```

### Modified tables

```sql
-- graph_submissions: add signer column
ALTER TABLE graph_submissions ADD COLUMN key_id TEXT;

-- leaderboard: add signer column
ALTER TABLE leaderboard ADD COLUMN key_id TEXT;
```

Both columns are nullable. NULL = anonymous.

## New crate: `ramseynet-identity`

Minimal crate with no heavy dependencies beyond `ed25519-dalek`.

```
ramseynet-identity/
├── Cargo.toml
└── src/
    └── lib.rs
```

### Public API

```rust
/// Generate a new Ed25519 keypair.
pub fn generate_keypair() -> (PublicKey, SecretKey)

/// Compute the key ID (first 16 hex chars of SHA-256 of public key bytes).
pub fn key_id(public_key: &PublicKey) -> String

/// Sign a canonical submission payload.
pub fn sign_submission(
    secret_key: &SecretKey,
    k: u32, ell: u32, n: u32,
    rgxf_json: &serde_json::Value,
) -> String  // hex-encoded 64-byte signature

/// Verify a signature against a public key and payload.
pub fn verify_submission(
    public_key: &PublicKey,
    k: u32, ell: u32, n: u32,
    rgxf_json: &serde_json::Value,
    signature_hex: &str,
) -> bool

/// Canonical payload bytes for signing (sorted JSON, no whitespace).
fn canonical_payload(k: u32, ell: u32, n: u32, rgxf_json: &serde_json::Value) -> Vec<u8>
```

### Key file format

Simple JSON at `~/.config/minegraph/key.json`:

```json
{
  "key_id": "3f8a1b2c4d5e6f70",
  "public_key": "abc123...64_hex_chars...",
  "secret_key": "def456...128_hex_chars..."
}
```

No encryption for v1. These are attribution keys, not financial keys.

## CLI additions

### Worker

```
--signing-key PATH    Path to MineGraph signing key file (optional)
```

If provided, the worker signs every submission. The key_id and signature are
included in the POST body. If not provided, submissions are anonymous.

### Standalone commands (future, or in a `minegraph` CLI binary)

```bash
minegraph keygen                          # generate key, write to ~/.config/minegraph/key.json
minegraph keygen --output ./my_key.json   # custom path
minegraph register-key                    # POST public key to server
minegraph register-key --server URL       # custom server
minegraph whoami                          # show key_id and display name
```

For v1, these could be subcommands of the worker binary or a simple script.

## Server API changes

### New endpoints

```
POST /api/keys
  Body: { "public_key": "hex...", "display_name": "optional" }
  Response: { "key_id": "3f8a1b2c4d5e6f70", "display_name": "..." }

GET /api/keys/{key_id}
  Response: { "key_id": "...", "public_key": "...", "display_name": "...", "created_at": "..." }
```

### Modified endpoints

```
POST /api/submit
  Body now optionally includes:
    "signature": "hex...",
    "key_id": "hex..."
  Response now includes:
    "signed_by": "3f8a1b2c4d5e6f70" | null

GET /api/leaderboards/{k}/{l}/{n}
  Entries now include:
    "key_id": "3f8a1b2c4d5e6f70" | null

GET /api/submissions/{cid}
  Response now includes:
    "key_id": "3f8a1b2c4d5e6f70" | null
```

## Web app changes

### Leaderboard table

Add a "Submitter" column showing:
- Key ID (truncated, linked to a key detail page) if signed
- "anon" in muted text if unsigned

### Submission detail page

Show "Signed by: {key_id}" or "Unsigned submission" in the metadata section.

### Homepage

If the #1 graph is signed, show the submitter identity alongside the gem.

## Backward compatibility

- Unsigned submissions continue to work exactly as today
- Existing leaderboard entries get `key_id = NULL` (anonymous)
- The `signature` and `key_id` fields in the submit request are optional
- No migration needed for existing data — new columns are nullable

## Security considerations

### Threat model

The primary threat is **impersonation** — someone claiming credit for a
discovery they didn't make. Ed25519 signatures prevent this.

The secondary threat is **spam** — flooding the server with garbage
submissions under a fake identity. Mitigation: rate limiting per key_id,
and the existing verification pipeline rejects invalid graphs anyway.

### What we explicitly don't protect against

- Compromised worker machines (the key is on disk in plaintext)
- Key theft (use per-worker keys as mitigation)
- Quantum attacks (Ed25519 is not post-quantum, but this is a graph
  search project, not a bank)

### Trust model

Registration is permissionless — anyone can register a public key.
The server trusts that whoever holds the corresponding private key is
the rightful submitter. There is no KYC, no email verification, no
OAuth. This matches the "permissionless protocol" ethos.

## Dependency

- `ed25519-dalek` — well-maintained, pure Rust, no C dependencies
- `sha2` — for key ID derivation (already in the dependency tree via RGXF CID)

## Implementation order

### Phase 1 (v1 — 2-3 evenings)

1. Create `ramseynet-identity` crate
2. Add `--signing-key` to worker CLI
3. Worker signs canonical payloads, includes in submit request
4. Server stores `key_id` on submissions (accepts but doesn't verify yet)
5. Web app shows "anon" vs key_id on leaderboard and submission pages

### Phase 2 (v2 — 1-2 evenings)

1. Server verifies signatures against registered public keys
2. `POST /api/keys` endpoint for key registration
3. `GET /api/keys/{key_id}` endpoint
4. Reject submissions with invalid signatures
5. Display name support

### Phase 3 (later)

1. Per-key submission stats page
2. Key management in web app
3. `minegraph` CLI binary with keygen/register subcommands
4. Optional per-worker keys for fleet deployments

## Open questions

1. **Should unregistered keys be allowed?** If someone signs a submission
   with a key that isn't registered, should the server: (a) accept and
   store the key_id anyway, (b) reject, or (c) accept but mark as
   "unverified"? I lean toward (c) for permissionless ethos.

2. **Should the key_id be part of the leaderboard ranking?** Currently
   ranking is purely by graph score. Adding identity doesn't change
   ranking, but it affects how ties are displayed.

3. **Namespace for display names?** Should display names be unique?
   Probably yes to avoid confusion, but enforcement can wait for v2.

## References

- Ed25519: https://ed25519.cr.yp.to/
- ed25519-dalek: https://docs.rs/ed25519-dalek
- Current submit flow: `crates/ramseynet-server/src/lib.rs` (submit_graph handler)
- Current schema: `crates/ramseynet-ledger/src/lib.rs` (create_tables)
- Phase 6 placeholder in CLAUDE.md: "ed25519 identity, duels, libp2p"
