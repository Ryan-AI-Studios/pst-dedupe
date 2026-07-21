# matter-service

Local **multi-user matter HTTP service** (track **0058**). Opt-in second product mode alongside single-exe Desk.

## Product mode

| Mode | Launch | Network | Writers |
|---|---|---|---|
| **Desk (default)** | Local open | None | Single operator |
| **Matter service** | `pst-dedup service serve --matter <path>` | Loopback HTTP (LAN only with `--allow-lan`) | One process owns the matter |

## Architecture locks

- **One writer process** — `WriteGate` (`Mutex<Matter>`) serializes mutates; exclusive OS lock on `.matter.lock`
- **Local identity only** — matter users + Argon2id secrets + bearer sessions (no OIDC)
- **OCC** — mutates require `expected_version`; stale → HTTP **409** (`version_conflict`)
- **Locks / batches** — foreign lock → **409** (`locked`); batch feed is membership-constrained
- **Strict actor** — session `user_id` injected; JSON body `actor` ignored
- **Encrypted matters** — unlock once at serve; clients never receive DEK/passphrase
- **Default bind** — `127.0.0.1:7749`; non-loopback requires `--allow-lan`

## CLI

```text
pst-dedup service serve --matter <path> [--bind 127.0.0.1:7749] [--allow-lan]
pst-dedup service bootstrap-admin --matter <path> --name <name> --password <pass>
pst-dedup service user add|list|disable …
```

Encrypted serve: set `PST_DEDUPE_MATTER_PASSPHRASE` (or `--passphrase-env`).

## Honesty / scale

Designed for small concurrent review teams (≈≤10) on **local disk** SQLite. Do not host the matter database on a network filesystem. Multi-tenant SSO and cloud backends are later tracks (**0059** / **0061**).
