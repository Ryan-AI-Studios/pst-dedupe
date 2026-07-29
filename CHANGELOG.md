# Changelog

All notable changes to **Dedupe Desk** / **pst-dedupe** are documented in this file.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning uses release candidates after Series I + Series K consolidation (`0.2.0-rc.N`).

## [Unreleased]

### Added (0082 — Recipient table fidelity)

- **Reader:** `pst-reader` walks per-message recipient TC (`NID_TYPE_RECIPIENT_TABLE`); structured `Recipient` / `RecipientType` (To/Cc/Bcc) with SMTP + EX address fields; never invents rows from Display* props.
- **Writer:** MS-PST recipient table template at NID **`0x692`** (14 MUST columns); every message gets a recipient TC subnode (empty table allowed); optional `PidTagSmtpAddress` column when known.
- **Tier-2.5 identity:** when a recipient table is present, fingerprint uses per-row cascade **SMTP → EX DN → display** over To+Cc+Bcc (sorted); table-less messages keep the display-string path.
- **BCC disclosure:** CLI `--include-bcc-recipients` (default **OFF**) writes Bcc rows + `PidTagDisplayBcc`; default suppresses BCC on the deliverable; identity hashing still includes BCC when the table is present.
- **`export_messages.csv`:** trailing `bcc_suppressed` boolean; summary `bcc_suppressed_message_count`.
- **Telemetry:** `sent_message_with_no_recipients_count` (empty TC + not UNSENT; not an `export_risk` / hard-fail).
- **`retryable`:** additive boolean on unique-export summary JSON (transient cancel/IO only; no new exit integers).
- **`fidelity_contract_v1`:** `recipient_table` → `Preserved` (BCC rows remain `DroppedByDesign` unless the flag is set).
- Closes **D-0080-recipient-table**, **D-0076-recipient-table**, **D-0068-04** recipient half, **D-0078-retryable**; decides **D-0080-bcc-policy** (opt-in write + suppress ledger). Named-prop / Mode A promote / unique-eml ledger / deterministic store key remain open.

### Added (0073 — Export attachment failure ledger)

- **unique-pst** streams `export_attachments.csv` (default `--attach-ledger=full`) with locus keys, stable reason codes, CSV injection neutralization, and a default 500k row cap.
- Additive `unique_export_report_v1` fields: `attachments_failed_by_reason`, `attachment_ledger*`, `attachments_omitted_by_policy`.
- `export_messages.csv` appends `attachments_failed_count` (column prefix stable).
- Writer: expanded `AttachmentFidelityKind` / locus events; every soft-fail path emits accounting; `parents_only` is severity `info` (omit ≠ fail).

### Residuals

- **D-0073-promote** — Mode A pre-write promote-on-attach-fail not shipped (ledger-only Mode C).
- **D-0073-eml** — unique-eml has no attach ledger parity yet.
- **D-0073-gui** — no GUI attach-ledger controls.

### Added (0081 — Unique export deps + operator docs)

- **`--ledger-path-mode full|basename`** (default **`full`**): basename rewrites `source_path` columns in `export_messages.csv` + `export_attachments.csv` for handoff only.
- **`export_messages.csv` trailing `source_id`** column (0-based index into `summary.inputs`; empty when unmapped) — join key under basename when multi-source packs share a basename.
- Standalone **`qc-pst`** resolves basenamed source opens via `source_id` + `summary.inputs` when the CSV path is missing.
- Operator runbook: [`docs/unique-pst-ediscovery-runbook.md`](docs/unique-pst-ediscovery-runbook.md). **D-0073-basename closed**.

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
