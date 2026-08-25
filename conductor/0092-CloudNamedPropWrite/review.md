# Track 0092 — CloudNamedPropWrite — Review

**Status:** Completed (engineering)  
**Branch:** `feat/0092-cloud-named-prop-write`  
**Ledger TX:** `512d13de-c95e-4f04-ae39-89ef3c3369e7`  
**Final Codex:** gpt-5.6-luna high — **PASS WITH DEFERRED P3** (`review.codex.r6.md`)

## Design locks

- Shared `NamedPropMapBuilder` (`named_prop_map.rs`) for production + fixture empty-plan path.
- Emit-only-when-used via per-volume `NamedPropWritePlan` (parents_only → empty).
- MUST `PidNameAttachmentProviderType`; MAY `AttachmentUrl` from `cloud_url`; PermissionType write-ready only.
- NPID `0x8000+` among used props in sorted name order.
- Hash buckets: BucketCount **251**, CRC-derived bucket index (MS-PST §2.4.7.5 / §5.3 weak CRC).
- Classic LongPathname + Pathname 0x3708; no hydrate; ledger URL unchanged.

## DoD matrix

| DoD | Status |
|---|---|
| 1 NPMAP streams + hash buckets | **Met** |
| 2 Attach PC ProviderType / classic / ledger | **Met** |
| 3 Determinism / emit-when-used / depth | **Met** |
| 4 QC ProviderType + scanpst skip-safe | **Met** (scanpst unavailable in CI; skip-safe) |
| 5 Close D-0084-cloud-named-prop-write | **Met** |
| 6 review.md + conductor Completed + ledger | **Met** (this file) |

## Review rounds

| Round | Verdict | Disposition |
|---|---|---|
| Internal r1 | PASS WITH DEFERRED P3 | PermissionType source residual |
| Codex r1 | FAIL | Per-volume plan, QC ProviderType, CRC test, fixture builder |
| Codex r2–r5 | FAIL | Mid-batch stop, empty-hash QC, CRC bucket key, depth/tests |
| Codex r6 | **PASS WITH DEFERRED P3** | Only PermissionType extract residual |

## Deferred

- **D-0092-permission-type-extract** — reader→canonical PermissionType plumbing (MAY).
- Encyclopedia / arbitrary NPMAP clone — out of scope.

## Operator notes

- Outlook visibility remains offline-pointer only (no hydrate).
- Optional Outlook open via existing `qc_attestation_v1` — not CI-blocking.
- `scanpst -no repair` when tool available (0080 skip-safe).

## Gates (orchestrator-observed)

- `cargo test -p pst-writer --lib named_prop` — pass (incl. CRC + depth)
- `cargo test -p pst-writer --test writer_fidelity -- cloud_` — pass
- `cargo test -p pst-dedup-cli --lib cloud_attachments` — pass
- `cargo test -p pst-dedup-cli --test unique_pst_qc_0080 -- cloud_provider` / `duplicate_cloud` — pass
- `cargo clippy -p pst-writer -p pst-dedup-cli --all-targets -- -D warnings` — pass
- `cargo fmt --all --check` — pass
