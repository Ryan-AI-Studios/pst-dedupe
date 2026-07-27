# 0073 — Export Attachment Failure Ledger — Plan

> **Ledger:** `ledgerful ledger start 0073-exportattachmentfailureledger --category FEATURE --message "Attach failure ledger + reason codes"`

**Status:** **Ready** — execute after reading `spec.md` (review folds 2026-07-26).

## Locks (from spec)

1. **Locus columns** + **source_id** (join); path same as export_messages (§3.3 / §3.3.1)  
2. **Stable reason codes** §3.2; omit ≠ fail; zero-byte success  
3. **Every `attachments_failed++` → accounting**; histogram complete even if CSV capped  
4. **Batched CSV** via non-blocking sink (§3.4.6); default `--attach-ledger=full`  
5. **CSV injection neutralization** on free-text (§3.4.5)  
6. **CSV row cap** default 500k + ATTACH_LEDGER_TRUNCATED (§3.4.7)  
7. **Additive** `unique_export_report_v1` histogram + truncation fields  
8. **Promote = Mode A pre-write only** (or residual)  
9. **Exit honesty unchanged** (attach fails → ok=false)  
10. **One row per attach** — never per CRC page  
11. **unique-eml** parity or **D-0073-eml**  

## Phase 0 — Inventory → DoD-1 foundation

- [ ] Inventory every `attachments_failed++` in `crates/pst-writer/src/production.rs`  
- [ ] Inventory `attachments_omitted_by_policy` / parents_only  
- [ ] Inventory materialize attach soft paths (`ATTACH_META_FAILED`) in keepset/materializer  
- [ ] Inventory `eml_pack` soft-skip for residual decision  
- [ ] Map each path → reason code (§3.2)  
- [ ] `ledgerful ledger start 0073-exportattachmentfailureledger --category FEATURE --message "…"`  
- [ ] Optional: `ledgerful scan --impact`  

## Phase 1 — Writer taxonomy + events → DoD-1, DoD-2, DoD-4, DoD-6, DoD-7

- [ ] Expand `AttachmentFidelityKind` (or parallel public reason enum) with stable `as_str()`  
- [ ] Expand event DTO with locus fields (§3.3)  
- [ ] Add optional **sink** on write APIs (`FnMut` / trait) for streaming  
- [ ] Emit on: unsupported method, resolve None, stream Err, depth, embedded unparsed, policy omit (info)  
- [ ] Thread locus from materialize → WriteAttachment/WriteMessage  
- [ ] Unit tests: each fail path emits event; zero-byte success; omit ≠ fail  
- [ ] Invariant helper: fail events == attachments_failed  

## Phase 2 — Report pack CSV + summary → DoD-3, DoD-5, DoD-9, DoD-11–14

- [ ] Shared `csv_escape_cell` with formula neutralization (`=+\-@`)  
- [ ] `export_attachments.csv` writer with **mpsc/batch thread** (or Send-ready buffer)  
- [ ] Wire unique-pst: accounting always; CSV via background writer  
- [ ] `--attach-ledger full|summary-only|off`; optional max-rows  
- [ ] Row cap: final `ATTACH_LEDGER_TRUNCATED` + summary flags  
- [ ] `source_id` column + inputs order documented  
- [ ] Additive summary fields: by_reason, ledger path/mode, truncated, rows_written  
- [ ] Preferred: `attachments_failed_count` on `export_messages.csv` (append column)  
- [ ] Tests: soft-fail, injection, truncation, source_id, summary-only/off  

## Phase 3 — Promote Mode A (or residual) → DoD-8

- [ ] If shipping: `--promote-on-attach-fail` pre-write incomplete → next peer by policy order  
- [ ] decisions.csv / promoted_from_failure honesty  
- [ ] Test: two peers, first attach-broken, second clean → second written  
- [ ] If not shipping: **D-0073-promote** residual; Mode C ledger-only documented  

## Phase 4 — unique-eml → DoD-10

- [ ] Minimal reason-coded skip rows in pack **or**  
- [ ] **D-0073-eml** residual + docs honesty  

## Phase 5 — Docs → DoD-15

- [ ] `docs/unique-pst-export.md` — ledger layout, flags, join/`source_id`, reason→action, **report-dir sensitivity**, CSV open safety  
- [ ] `docs/pst-writer-fidelity-v1.md` — event/reason expansion  
- [ ] CHANGELOG note (additive report fields)  
- [ ] Cross-link 0074 shared codes / 0077 noise boundary  

## Phase 6 — Gate + finalize → DoD-16, DoD-17

- [ ] `cargo test -p pst-writer --test writer_fidelity`  
- [ ] `cargo test -p pst-writer --test writer_streaming`  
- [ ] `cargo test -p pst-dedup-cli --test unique_pst`  
- [ ] `cargo clippy -p pst-writer -p pst-dedup-cli --all-targets -- -D warnings`  
- [ ] Full workspace gate before commit  
- [ ] `review.md` (evidence, residual D-0073-*, fold disposition)  
- [ ] Update `conductor/conductor.md`, `sequencing.md`, `ROADMAP.md` if needed → **Completed**  
- [ ] `docs/deferred.md` D-0073-*  
- [ ] `ledgerful ledger commit <tx-id> --summary "…" --reason "…"`  

## Suggested order (blast radius)

1. Taxonomy + emit on all silent `++` (writer-only)  
2. Locus threading from materialize  
3. CSV sink + unique-pst + summary histogram  
4. export_messages incomplete column  
5. Promote Mode A or residual  
6. unique-eml residual/parity  
7. Docs + review  

## Handoff notes

- **Do not** repair source PSTs.  
- **Do not** write-time promote without message atomicity design.  
- **Do not** put attach bytes or bodies in the ledger.  
- **Do not** break fixed export_messages column **prefix** without docs (append only).  
- **Do not** change attach→exit policy without 0078.  
- **Do not** write raw formula-leading cells to CSV.  
- **Do not** unbounded-stream fail rows to disk without the row cap.  
- Rollback: flag `--attach-ledger=off` restores pre-CSV behavior; counts still honest.
