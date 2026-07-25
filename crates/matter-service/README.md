# matter-service

Local **multi-user matter HTTP service** (track **0058**) with optional **platform OIDC SSO** (track **0059**). Opt-in second/third product modes alongside single-exe Desk.

## Product modes

| Mode | Launch | Network | Writers |
|---|---|---|---|
| **Desk (default)** | Local open | None | Single operator |
| **Matter service** | `pst-dedup service serve --matter <path>` | Loopback HTTP (LAN only with `--allow-lan`) | One process owns the matter |
| **Platform SSO** | `serve --platform <platform.db> --matter <path>` | Same; OIDC login + tenant isolation | Matter must be registered under tenant |

## Architecture locks

- **One writer process** — `WriteGate` (`Mutex<Matter>`) serializes mutates; exclusive OS lock on `.matter.lock`
- **Local identity** — matter users + Argon2id secrets + bearer sessions
- **OIDC (opt-in)** — Authorization Code + PKCE via `openidconnect` discovery + JWKS ID-token verify (`iss`/`aud`/`exp`/`nonce`/`state`); mock IdP in tests
- **OCC** — mutates require `expected_version`; stale → HTTP **409** (`version_conflict`)
- **Locks / batches** — foreign lock → **409** (`locked`); batch feed is membership-constrained
- **Logout (P0)** — `POST /v1/logout` invalidates session **and** releases item locks + batch checkouts
- **Strict actor** — session `user_id` injected; JSON body `actor` ignored
- **Encrypted matters** — unlock once at serve; clients never receive DEK/passphrase
- **Default bind** — `127.0.0.1:7749`; non-loopback requires `--allow-lan`
- **Cross-tenant** — fail closed (prefer **404**)

## CLI

```text
pst-dedup service serve --matter <path> [--bind 127.0.0.1:7749] [--allow-lan] [--platform <platform.db>]
pst-dedup service bootstrap-admin --matter <path> --name <name> --password <pass>
pst-dedup service user add|list|disable …
pst-dedup platform tenant create --platform <db> --slug firm-a --name "Firm A" [--jit] [--oidc-required]
pst-dedup platform idp set --platform <db> --tenant firm-a --issuer URL --client-id ID --secret-env VAR \
  --allowed-domains firma.com
pst-dedup platform matter register --platform <db> --tenant firm-a --path C:\matters\case1
```

Encrypted serve: set `PST_DEDUPE_MATTER_PASSPHRASE` (or `--passphrase-env`).

Platform serve:

- `PST_DEDUPE_PLATFORM_MASTER_KEY` — required when IdP secrets are ciphertext in platform.db
- `PLATFORM_STORAGE_ROOT` — required for matter registration **and** platform open (revalidated at serve)

### Operator OIDC setup (Entra / Okta / generic)

1. Create a platform DB: `pst-dedup platform init --platform C:\dedupe\platform.db --print-pmk` and export the PMK as `PST_DEDUPE_PLATFORM_MASTER_KEY` (hex or base64 32-byte key).
2. Create a tenant and IdP:
   - Issuer must be the OIDC **issuer** URL (discovery: `{issuer}/.well-known/openid-configuration`).
   - Client ID from the IdP app registration.
   - Prefer `--secret-env OIDC_CLIENT_SECRET` (env-ref) over storing ciphertext.
   - When JIT is enabled, set `--allowed-domains firm.com` and/or `--required-groups …` (open JIT is rejected).
3. **Redirect URI (exact):** register with the IdP:
   - `http://127.0.0.1:7749/v1/oidc/callback` (default bind)
   - Or `http://<bind-host>:<port>/v1/oidc/callback` when using a custom `--bind`
   - The service **only** accepts this exact callback URI (no client-supplied redirect).
4. Register matter paths under `PLATFORM_STORAGE_ROOT` (strict subdirectory; directories only).
5. Serve: `pst-dedup service serve --platform … --matter …` (matter must already be multi-user + registered).
6. Role mapping: IdP `groups`/`roles` claims map via IdP `role_claim_map` JSON to `admin` / `reviewer` / `read_only` (default `reviewer`).

## OIDC routes (platform mode)

| Method | Path | Notes |
|---|---|---|
| GET | `/v1/oidc/login?tenant=…` | Start PKCE; `?format=json` for headless/tests; optional `handoff_url` (loopback only) |
| GET | `/v1/oidc/callback?code&state` | Exchange + issue bearer; with handoff → redirect one-time code to Desk loopback |
| POST | `/v1/oidc/exchange` | Redeem one-time handoff code → `LoginResponse` (Desk SSO, track **0064**) |
| POST | `/v1/logout` | Session kill + lock release |
| POST | `/v1/oidc/logout` | Same local effect (IdP RP logout residual) |
| GET | `/v1/tenants/me` | Current tenant metadata |
| GET | `/v1/platform/matters` | Tenant-scoped matter list |
| POST | `/v1/login` | Password login when OIDC not required |

### Desk Connect client (0064)

Native **dedupe-desk** can Connect as a multi-user client:

1. Operator starts this service on the host (loopback default).
2. Desk **Connect** dialog: `GET /healthz` then `POST /v1/login` (or SSO handoff).
3. Thin review: `GET /v1/items`, `GET /v1/items/{id}/body`, `POST /v1/items/{id}/codes` with `expected_version`.
4. Session actor is the bearer only — clients must not send body `actor` (ignored if present).
5. SSO: Desk binds `127.0.0.1:ephemeral`, opens system browser to `/v1/oidc/login?handoff_url=…`, redeems code via `/v1/oidc/exchange`. IdP `redirect_uri` remains the service callback only.

Produce and process-runner jobs stay on the **host** (Solo Desk or CLI) — this API does not expose produce.

## Honesty / scale

Designed for small concurrent review teams (≈≤10) on **local disk** SQLite. Do not host the matter database on a network filesystem. Multi-matter single process and cloud backends are residual / later tracks (**D-0058-08** / **0061**).

## Residuals (D-0059-* / D-0064-*)

- SAML 2.0
- IdP RP-initiated / back-channel logout (local logout + lock release is P0)
- Per-tenant matter CMK / external KMS (`TenantKeyProvider` stub only)
- Multi-matter single process host (D-0058-08)
- SCIM provisioning; Postgres platform DB
- Full remote Desk parity; custom URI scheme SSO; durable multi-instance handoff codes
