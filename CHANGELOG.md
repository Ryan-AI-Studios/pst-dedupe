# Changelog

All notable changes to **Dedupe Desk** / **pst-dedupe** are documented in this file.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning uses release candidates after Series I + Series K consolidation (`0.2.0-rc.N`).

## [Unreleased]

### Fixed (0105 — body-cloud window-edge normalize)

- Body-cloud window-edge duplicate of a kept URL with trailing sentence punctuation (or an already-noted over-length URL) no longer emits a false `BODY_CLOUD_LINK_WINDOW` marker. Closes **D-0097-window-edge-normalize**.

### Fixed (0104 — attachment TC Strategy A multipage)

- Unique-pst per-message attachment table (`0x671`) uses Strategy A (paged HN +
  row-matrix subnode + cell NID for long filenames) so large attach lists no
  longer trip single-page heap overflow. Closes **D-0093-attachment-tc-page**.

### Fixed (0103 — recipient TC SLBLOCK NID order)

- Unique-pst recipient-table SLBLOCK NIDs are sorted ascending so Outlook can
  resolve long-string cell NIDs (`add_subnode_leaf` emit-sort; matrix push not
  `insert(0)`). Closes **D-0100-slblock-nid-order**.

### Fixed (0102 — export oracle `export_risk.inputs` attest)

- Unique-pst `export_oracle` no longer recursive-strips the name `inputs`, so 0099
  `export_risk.inputs` attest fields survive normalize and compare. Job-level
  `summary.inputs` paths still blank at root only. Closes **D-0099-oracle-inputs-attest**.

### Added (0101 — unique-pst `--max-embedded-depth`)

- `unique-pst --max-embedded-depth` (default **3**, valid **1–8**; clap rejects outside that range). The same effective value reaches nested extract and the writer. Identity hash depth stays 3.
- `unique_export_report_v1` gained always-present `export.max_embedded_depth` (consumers should ignore unknown keys; schema id **not** bumped). Remaining over-depth nests still ledger `ATTACH_DEPTH_LIMIT`.

### Fixed (0099 — CRC / poly export-risk honesty)

- Unique-pst `export_risk` keys catastrophic/advisory `block_crc_read_rate` on the **effective** (non-poly) rate from scan `files[]`. Dual-rate poly-class CRC (`page≥0.50` AND `block≥0.50`) no longer forces `not_export_ready` / `re_export_recommended`.
- `ATTACH_STREAM_CRC` Info is discounted only when every CRC-noisy source is poly-class. Localized-block jobs and `attach_fail_rate` / failed volume / scan preflight are unchanged (fail closed).
- Attest fields: `effective_block_crc_read_rate`, `poly_class_crc_discounted`, `discount_attach_stream_crc`, `poly_class_crc_sources`. Raw CRC counters stay on `inputs`. Closes **D-0077-systematic-poly** (export-risk half). Residual: `D-0077-poly-fingerprint`, `D-0099-attach-crc-job-level`.

### Fixed (0098 — Template NID / folder contents collision)

- `alloc_nid` skips MS-PST reserved nidIndex `0x30` / `0x33` / `0x34` so a user folder’s hierarchy/contents/assoc tables cannot collide with empty template objects `0x60D`–`0x610` / `0x671` / `0x692`.
- Duplicate NBT NIDs fail closed at insert and at `write_nbt` (last-wins previously dropped folder contents from `folders()` / unique-pst verify).
- Closes **D-0098-template-nid-collision** (INC0102784: 4055 written / 4005 found; 50 Purges orphans).

### Fixed (0097 — Body-cloud truncation honesty)

- `export_body_cloud_links.csv` no longer emits empty-URL `BODY_CLOUD_LINK_TRUNCATED` rows for every body over the 100k scan window. Window-only zero-candidate messages emit **0** CSV rows; `body_scan_window_capped_messages` counts those bodies.
- `truncated` is a row-type discriminator. When document-shaped candidates were actually dropped, **≤1** honesty marker per message (`link_index=u32::MAX`) with reason `BODY_CLOUD_LINK_WINDOW` / `BODY_CLOUD_LINK_MAX_LINKS_EXCEEDED` / `BODY_CLOUD_LINK_URL_TRUNCATED` (pipe-joined). Umbrella `BODY_CLOUD_LINK_TRUNCATED` is gone.
- Over-length (>2048) document-shaped URLs (including SafeLinks nested targets) are no longer silent drops: marker carries the first 2048-char prefix; prefix is **not** a kept hit and does not increment `body_cloud_links_total`.
- Caps stay 100_000 / 2048 / 50. Closes **D-0097-body-cloud-truncate-honesty**.

### Added (0092 — Cloud named-prop NPMAP write)

- Production writer replaces empty `NID_NAME_TO_ID_MAP` stub with allowlisted NPMAP when used: GUID/entry/string streams + MS-PST hash buckets (`BucketCount=251`).
- Cloud pointer attaches re-emit `PidNameAttachmentProviderType` when known; MAY copy `AttachmentUrl` from `cloud_url` when present; classic `PidTagAttachLongPathname` (+ optional `PidTagAttachPathname`) retained. `AttachmentPermissionType` is writable on `WriteAttachment` when set, but PST→canonical extract of PermissionType is not plumbed yet (MAY residual).
- Emit-only-when-used via `NamedPropWritePlan` pre-scan (cloud-free exports keep empty stub).
- Closes **D-0084-cloud-named-prop-write** (encyclopedia residual remains out of scope).

### Added (0091 — Digest + probe unify)

- When `--strong-content-hash body-recip-attach` and `--deep-attach-preflight` both run, Pass-1 **Real by-value** attach digests seed a Full/ok probe cache; Pass-2 skips second stream I/O while still charging probe tallies once (`digest_stream_skips`).
- Method gate runs before cache hits; zero-ms probe timeout honored on seeds; `AttachProbePreflight` exposes `bytes_probed` + `digest_stream_skips`.
- Single-feature paths unchanged; Embedded/Unread/DepthLimit/unsupported-method digests do not seed.
- Closes **D-0086-digest-probe-unify**.

### Added (0090 — Embedded message content hash)

- Under `--strong-content-hash body-recip-attach`, method-5 / `message/rfc822` embeds use **`embedded-msg-hash/v1`** (header + body + recipients + child attaches in table index order) instead of unread-sentinel-only or raw-blob-only.
- `pst-reader`: `MessageNodeRef` + `read_embedded_message_identity` (nested message is an attach subnode, not NBT); body subnode size peeked via BBT/XBLOCK metadata before materialize.
- Depth cap sentinel `attach-depth-limit/v1` (max depth 3); missing-body `embedded-body-missing/v1`; honesty stats `strong_hash_embedded_parsed` / `_depth_limit` / `_unparsed`.
- **Not Relativity dedupe parity.** Closes **D-0086-embedded-email-hash**; leaves **D-0067-embedded-depth** open.

### Added (0089 — Unique-EML attach ledger parity)

- **`unique-eml`** streams `{out}/export_attachments.csv` (default `--attach-ledger=full`) with the same header as unique-pst (`EXPORT_ATTACHMENTS_CSV_HEADER`).
- Engine `EmlAttachEvent` on `EmlWriteResult` (DTO only; no engine→CLI dependency); CLI maps events + Mode A `soft_skip_attach_records` / `mark_promoted_winner` into `AttachLedgerSink`.
- Flags: `--attach-ledger`, `--attach-ledger-max-rows`, `--ledger-path-mode` (aligned with unique-pst). Fail-closed ledger init when mode ≠ `off`.
- Exit 64 / `fidelity` still driven by `attach_parts_failed` counters (ledger additive; `off` still classifies from counters).
- Closes **D-0073-eml**.

### Added (0088 — Sovereign cloud host allowlist)

- Body-cloud host allowlist extended with US GCC High / DoD suffixes: `*.sharepoint.us`, `admin.onedrive.us`, `*.sharepoint-mil.us`, `*.dps.mil`.
- SafeLinks unwrap for `*.safelinks.protection.office365.us` (nested target re-tested against document-shaped allowlist).
- `pst-reader` attach-path cloud heuristic tightened to suffix-safe host checks (rejects `notsharepoint.attacker.com`).
- Closes **D-0085-sovereign-cloud-hosts**. Opens **D-0088-usgovcloud-microsoft-tld** (future `.microsoft` TLD content hosts). 21Vianet excluded; GCC Moderate uses commercial endpoints.

### Added (0087 — Deterministic store RecordKey)

- Production `write_unicode_pst*` defaults to a **deterministic** 16-byte store `PidTagRecordKey` / EntryID ProviderUID (domain-separated SHA-256; length-prefixed MID/subject/submit/folder fingerprint; path-independent).
- `WritePstOpts`: `volume_index`, optional `store_key_material`, `store_record_key_mode` (`Deterministic` default / `Ephemeral` escape).
- unique-pst threads 0-based volume index and a job-global seed from ordered keep-set winner loci; summary reports `store_record_key_mode`.
- **Hard guarantee:** same winners + same layout → same RecordKey across re-runs and dest paths. **Best-effort:** full volume `sha256_hex` (B-tree/layout); 0079 structural oracle remains the honest fallback.
- Closes **D-0079-deterministic-key**. Volume-layout coupling and CoC wording in unique-pst export + eDiscovery runbook.

### Added (0086 — Attach-content strong identity)

- **`--strong-content-hash body-recip-attach`** live on scan / dups / keep-set / unique-eml / unique-pst (default remains `off`).
- Full-stream per-attachment **SHA-256** via chunked `open_attachment_data` (64 KiB; no multi-GB `Vec`); digests sorted into Tier-2.5 strong preimage.
- **Choice B unread sentinels** for cloud-link / open-fail / CRC / length-mismatch / budget / cancel:
  `SHA-256("pst-dedup/attach-unread/v1\0" || name_lower || "\0" || size_le_u32)` — never omit slots, never tier-downgrade to `body-recip`.
- Legitimate size-0 empty stream → real `SHA-256("")`; size &gt; 0 + empty/short stream → unread.
- Budgets: `--strong-hash-attach-max-attaches` (50k), `--strong-hash-attach-max-bytes` (1 GiB), `--strong-hash-attach-per-attach-max-bytes` (512 MiB).
- Stats: `strong_hash_attach_unread` / `_digested` / `_bytes` / `_truncated`; soft stderr warning when combined with `--identity-ignore-inline-attachments`.
- Fail-closed: `list_attachments_strict` under `body-recip-attach` (row PC errors → Err → Skip); `has_attachments=true` + empty list → Skip; hard-reject `--no-attachments` + `body-recip-attach`.
- NIST multi-block SHA-256 KAT on the same `sha2` path; synthetic PST integration proves same name:size different bytes split only at attach level.
- Closes **D-0076-attach-content**. Opens **D-0086-embedded-email-hash**, **D-0086-digest-probe-unify**.

### Added (0085 — Body-inline cloud link detect)

- **Offline body scan** for **document-shaped** commercial SharePoint/OneDrive URLs in HTML (primary) and plain body: action tokens `:w:`/`:x:` (Excel mandatory)/`:p:`/`:b:`/`:u:` (exclude `:f:`); Office/PDF extensions; `1drv.ms`; SafeLinks unwrap when nested target is document-shaped.
- **Report pack:** `export_body_cloud_links.csv` (multi-row hit-list; full query preserved; CSV injection neutralized without URL rewrite); `export_messages.csv` appends `body_cloud_link_count`; summary `messages_with_body_cloud_links` / `body_cloud_links_total` / `body_cloud_link_truncated_messages`.
- **Caps:** 100_000 body window, 2048 URL length, 50 links/message; truncation marker `BODY_CLOUD_LINK_TRUNCATED`.
- **0097 follow-up:** umbrella `BODY_CLOUD_LINK_TRUNCATED` removed. Markers are now `BODY_CLOUD_LINK_WINDOW` / `BODY_CLOUD_LINK_MAX_LINKS_EXCEEDED` / `BODY_CLOUD_LINK_URL_TRUNCATED` (≤1 per message, `link_index=u32::MAX`) only when a document-shaped candidate was dropped; split `body_scan_window_capped_messages`. Update counsel greps.
- **Product rules:** no network fetch; no invented Attachment Table rows; body hits do **not** set `is_attach_incomplete` / Mode A promote; exit 64 not forced by body-only hits; commercial host allowlist only.
- **`fidelity_contract_v1.cloud_modern_attachments`:** attach-table **and** body-inline document-shaped detect offline; payload never Preserved; Mode A body-only known gap stated; sovereign residual **D-0085-sovereign-cloud-hosts**.
- Closes **D-0084-body-cloud-links**. Opens **D-0085-sovereign-cloud-hosts**.

### Added (0084 — Named property resolution & cloud attach detect)

- **NPMAP reader** (`NID_NAME_TO_ID_MAP` `0x61`): parse Entry/GUID/String streams; resolve GUID+LID and GUID+name → NPID; cache per `PstFile`; degrade on corrupt/missing (no hard-fail open).
- **Attachment-table cloud detect** (independent OR): `PidNameAttachmentProviderType` (PSETID_Attachment + name) without usable binary **or** web-ref/non-portable method without payload **or** conservative URL-shaped path fallback. Explicit `is_cloud_link` + `cloud_provider` / `cloud_url` on attach descriptors.
- **Incomplete / Mode A:** CloudLink without offline payload → `is_attach_incomplete` true; Mode A can prefer a physical peer.
- **Ledger:** reason `ATTACH_CLOUD_LINK` (fail severity; prefer over bare `ATTACH_METHOD_UNSUPPORTED` when CloudLink); `export_attachments.csv` appends `cloud_provider,cloud_url` (injection-neutralized).
- **Pointer preserve (anti-ghost):** unique-PST writes metadata/pointer attach row for CloudLink (method + filename + best-effort long pathname URL); **never** invents binary; no network hydration.
- **`fidelity_contract_v1`:** `cloud_modern_attachments` + `PidNameAttachmentProviderType` → BestEffort with honest attach-table scope + body residual named (payload never Preserved).
- Closes **D-0080-cloud-attachments** (attach-table detect). Opens **D-0084-body-cloud-links**, **D-0084-cloud-named-prop-write**. Narrows **D-0068-04** named-prop residual.

### Added (0083 — Mode A promote-on-attach-fail)

- **`--promote-on-attach-fail`** (default **off**) on `unique-pst` and `unique-eml`: pre-write promote when a keep-set peer materializes with incomplete attachments and a ranked peer is complete.
- **Incomplete predicate** centralized as `is_attach_incomplete` (`stream_available == false` or fail-severity attach fidelity; not body soft flags / parents_only omit / CRC noise). **0084** extends this for attachment-table cloud/modern link-only attaches.
- **`decided_by` vocabulary:** `promoted_after_attach_incomplete`, `promoted_after_materialize_fail` (hard path unchanged), `mode_c_fallback_all_peers_incomplete` (all materializable peers incomplete → highest-ranked materializable; not group drop).
- Summary counters: `promote_on_attach_fail`, `promoted_after_attach_incomplete_count`, `mode_c_fallback_all_peers_incomplete_count`.
- Attach ledger: `winner_promoted` / peer locus honesty for soft-skipped incomplete peers and promoted winners.
- **Mode B** write-time mid-message promote **permanently declined**. Cross-custodian disclosure documented (Sedona term; `duplicate_sources` invariant after promote).
- Closes **D-0073-promote**. **D-0073-eml** full ledger CSV closed in **0089**.

### Added (0082 — Recipient table fidelity)

- **Reader:** `pst-reader` walks per-message recipient TC (`NID_TYPE_RECIPIENT_TABLE`); structured `Recipient` / `RecipientType` (To/Cc/Bcc) with SMTP + EX address fields; never invents rows from Display* props.
- **Writer:** MS-PST recipient table template at NID **`0x692`** (14 MUST columns); every message gets a recipient TC subnode (empty table allowed); optional `PidTagSmtpAddress` column when known.
- **Tier-2.5 identity:** when a recipient table is present, fingerprint uses per-row cascade **SMTP → EX DN → display** over To+Cc+Bcc (sorted); table-less messages keep the display-string path.
- **BCC disclosure:** CLI `--include-bcc-recipients` (default **OFF**) writes Bcc rows + `PidTagDisplayBcc`; default suppresses BCC on the deliverable; identity hashing still includes BCC when the table is present.
- **`export_messages.csv`:** trailing `bcc_suppressed` boolean; summary `bcc_suppressed_message_count`.
- **Telemetry:** `sent_message_with_no_recipients_count` (empty TC + not UNSENT; not an `export_risk` / hard-fail).
- **`retryable`:** additive boolean on unique-export summary JSON (transient cancel/IO only; no new exit integers).
- **`fidelity_contract_v1`:** `recipient_table` → `Preserved` (BCC rows remain `DroppedByDesign` unless the flag is set).
- Closes **D-0080-recipient-table**, **D-0076-recipient-table**, **D-0068-04** recipient half, **D-0078-retryable**; decides **D-0080-bcc-policy** (opt-in write + suppress ledger). Named-prop / unique-eml ledger / deterministic store key remain open (**Mode A promote closed in 0083**).

### Added (0073 — Export attachment failure ledger)

- **unique-pst** streams `export_attachments.csv` (default `--attach-ledger=full`) with locus keys, stable reason codes, CSV injection neutralization, and a default 500k row cap.
- Additive `unique_export_report_v1` fields: `attachments_failed_by_reason`, `attachment_ledger*`, `attachments_omitted_by_policy`.
- `export_messages.csv` appends `attachments_failed_count` (column prefix stable).
- Writer: expanded `AttachmentFidelityKind` / locus events; every soft-fail path emits accounting; `parents_only` is severity `info` (omit ≠ fail).

### Residuals

- **D-0073-promote** — **closed / 0083** (Mode A flag; Mode B declined).
- **D-0073-eml** — **closed / 0089** (unique-eml attach ledger CSV parity).
- **D-0073-gui** — no GUI attach-ledger / Mode A controls.

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
