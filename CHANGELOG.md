# Changelog

All notable changes to **Dedupe Desk** / **pst-dedupe** are documented in this file.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning uses release candidates after Series I + Series K consolidation (`0.2.0-rc.N`).

## [0.2.0-rc.1] — 2026-07-24

First counsel-facing **release candidate** after platform Series I and clean unique-export Series K.
All workspace crates versioned **0.2.0-rc.1**. Matter schema pin: **`SCHEMA_VERSION` = 39**.

### Shipped capabilities

- **Offline matter Desk** (`dedupe-desk`): create/open matter, ingest, extract, reduce/promote, review, coding, privilege/redaction, produce (local).
- **Headless CLI** (`pst-dedup`): PST inspect/scan/dups, keep-set, unique-eml, **unique-pst**, matter/job/profile/workflow automation, report/qc/produce/gap.
- **Unique PST GUI wizard** (`pst-dedup-gui`): optional thin UI over the same `unique-pst` keep-set path (track 0072).
- **Series I platform (opt-in only):** matter encryption at rest, multi-user matter service, multi-tenant OIDC SSO, multi-jurisdiction produce/QC profiles, cloud blob/job backends.
- **Series K clean unique export:** scan integrity → keep-set → unique EML pack → production Unicode PST writer (streaming multi-GB path) → CLI report pack → optional Desk wizard.

### Modes

| Mode | Default |
|---|---|
| Desk solo + CLI offline | **Yes** |
| Matter service / SSO / cloud CAS | **Opt-in only** — never required for local matter |

See [`docs/operator-golden-path.md`](docs/operator-golden-path.md).

### Release hygiene (0062)

- `deny.toml` — strict license allow-list; GPL/AGPL-class copyleft denied for the release graph.
- CI: `cargo audit` + `cargo deny check` jobs (in addition to fmt/clippy/test).
- CycloneDX **SBOM** (`bom.json`) generated into the release package.
- Release profile **`debug = 1`** for usable field stacks; PDBs archived/shipped for support.
- Authenticode process documented; **operator-facing** ZIP must be signed (see `docs/release-signing.md`).

### Known limitations

- **Outlook / scanpst** structural verification of writer volumes is an **operator residual** (D-0068-02). Do not claim “Outlook production-ready” without that smoke.
- **Desk Connect** multi-user UX and produce profile dropdown UI residual → track **0064**.
- **Security red-team** campaign → track **0063**.
- OCR/STT require **operator-installed** external tools (not bundled).
- Multi-GB unique-pst soak is operator/nightly, not CI.
- Full residual ledger: [`docs/deferred.md`](docs/deferred.md).

### Binaries

| Binary | Crate |
|---|---|
| `dedupe-desk.exe` | dedupe-desk |
| `pst-dedup.exe` | pst-dedup-cli |
| `pst-dedup-gui.exe` | pst-dedup-gui |

### Tag

Git tag plan: **`v0.2.0-rc.1`** after merge to `main`.

## [0.1.0] — historical

Pre-RC development line (all crates `0.1.0`). Series A–K feature tracks landed on this version line before the RC freeze.
