# matter-platform

**Platform control plane** for multi-tenant isolation + OIDC SSO (track **0059**).

Separate from case `matter.db`: tenants, IdP configuration, and matter registration live in **`platform.db`**.

## Product modes

| Mode | Auth | Tenancy |
|---|---|---|
| Desk solo | Free-string actor | None (`tenant_id` null) |
| Local multi-user (0058) | Matter password users | None |
| **Platform / SSO (0059, opt-in)** | OIDC (+ optional local) | This crate + matter `tenant_id` |

## Architecture locks

- Isolation = **registry + matter boundary** (not shared multi-tenant `items`)
- IdP `client_secret`: **env-ref** (`secret_env`) **or** AEAD under **Platform Master Key** (`PST_DEDUPE_PLATFORM_MASTER_KEY`) — never plaintext in DB
- Matter paths must sit under **`PLATFORM_STORAGE_ROOT`** (strict subdirectory)
- JIT provisioning requires **domain and/or group allowlist** (open JIT forbidden)
- `TenantKeyProvider` is a **stub only** (no cloud CMK in 0059)

## CLI (via pst-dedup)

```text
pst-dedup platform tenant create --platform <db> --slug firm-a --name "Firm A" [--jit] [--oidc-required]
pst-dedup platform idp set --platform <db> --tenant firm-a --issuer URL --client-id ID --secret-env VAR \
  --allowed-domains firma.com
pst-dedup platform matter register --platform <db> --tenant firm-a --path C:\matters\case1
```

Env:

- `PST_DEDUPE_PLATFORM_MASTER_KEY` — base64 (32 bytes) or hex (64 chars)
- `PLATFORM_STORAGE_ROOT` — allowed matter storage root(s), `;`-separated

## See also

- `crates/matter-service` — OIDC routes + logout lock release
- `ARCHITECTURE.md` — three product modes
