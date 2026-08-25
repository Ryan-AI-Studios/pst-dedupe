# 0089 — Unique-EML Attach Ledger Parity

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.

- **Track ID:** 0089-UniqueEmlAttachLedger
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → Series M continuation
- **Cross-repo contract:** n/a
- **Status:** Ready — not started (review-folded 2026-08-24)
- **Depends on:** 0073 · 0078 · 0083 · 0067 (all **Completed**)
- **Spec authored:** 2026-08-24
- **Series:** M (Unique export fidelity residuals — continuation)
>
> **Review fold-in (2026-08-24):** dual-AI Ready review (`opencode-review.md` + `agy-review.md`) incorporated below.
> Disposition of each claim is in §2.6.

---

## 1. Objective

Bring `unique-eml` attach soft-skip / failure reporting to **full CSV ledger parity** with unique-pst’s 0073 `export_attachments.csv` — same header, locus, `source_id`, reason taxonomy, row-cap, CSV-injection safety, Mode A `winner_promoted` / soft-skip rows — not only the 0078 data-path counters that drive exit 64 / `fidelity`.

**Closes:** `D-0073-eml`.

---

## 2. Context (read before starting)

### 2.1 Why this track exists now

| Deferred / ceiling | Severity | Claim |
|---|---|---|
| **D-0073-eml** | P2 | unique-eml lacks full attach ledger CSV; 0078 only added counters for honest exit/`fidelity` |
| Code comment | — | `UniqueEmlCliArgs` / summary explicitly note residual **D-0073-eml** |
| Operator pain | — | unique-pst can produce actionable attach failure CSVs; unique-eml cannot |

### 2.2 Live code snapshot (verified 2026-08-24)

| Surface | State |
|---|---|
| `pst-dedup-cli/src/unique_export_report.rs` | `AttachLedgerSink`, `AttachLedgerRow`, `EXPORT_ATTACHMENTS_CSV_HEADER`, modes `full\|summary\|off`, row-cap, path-mode |
| `unique_eml_cmd.rs` | `attach_parts_failed` counter; **no** `attach_ledger` fields on `UniqueEmlCliArgs` |
| unique-pst | Ledger rows from (a) `pst-writer` `AttachEventSink` during materialize and (b) `soft_skip_attach_records` + `mark_promoted_winner` |
| `dedup-engine/src/eml_pack.rs` | Soft-fails increment a counter + `tracing` log — **no structured per-attach events** (locus, reason, filename, attach NID) |
| Crate graph | `dedup-engine` **must not** depend on `pst-dedup-cli` |

### 2.3 Product locks

1. **Reuse** `EXPORT_ATTACHMENTS_CSV_HEADER` byte-for-byte — do not invent a second CSV dialect.
2. Default ledger mode matches unique-pst (`full`).
3. Exit 64 / `fidelity` / Mode A (`promote_on_attach_fail`) remain consistent with ledger on, off, **or ledger init fail** (fail closed like unique-pst report-pack error).
4. `--ledger-path-mode` (`full` \| `basename`) on unique-eml (0081).
5. Synthetic fixtures only.

### 2.4 Architecture (normative)

**DTO boundary (no circular deps):**

```text
eml_pack (dedup-engine)
  → EmlWriteResult.attachment_events: Vec<EmlAttachEvent>
      { attach_index, filename, size, attach_method, reason_code, error_detail,
        /* locus fields available to pack: msg identity as pack already knows */ }

unique_eml_cmd (pst-dedup-cli)
  → map EmlAttachEvent + keep-set locus/source_id → AttachLedgerRow
  → AttachLedgerSink::enqueue
  → also drain resolved.soft_skip_attach_records
  → ledger.mark_promoted_winner() for Mode A (mirror unique_pst_cmd)
```

`EmlAttachEvent` stays in `dedup-engine`. `AttachLedgerSink` stays in CLI.

**Reason mapping (Phase 0 locks a table):** `EmlWriteError` variants → existing 0073 `reason_code` strings. Unmapped → a **generic documented code** (never silently drop the row).

**CSV path (locked):** `--out/export_attachments.csv` (pack root, beside `manifest.json` / `summary.json`). Optional `--report-dir` if unique-eml already has one; do **not** invent a `REPORTS/` subfolder unless unique-eml already uses that convention.

### 2.5 Dual-AI review disposition (2026-08-24)

| # | Claim | Source | Disposition | Spec landing |
|---|---|---|---|---|
| O1 | Not CLI-only; need event surface in `eml_pack`; ledger TX covers both crates | opencode | **Agree** | §2.4; plan ledger start; §8 |
| O2 | Explicit EML-error → 0073 reason_code map; never drop unmapped | opencode | **Agree** | §2.4 |
| O3 | Mode A `winner_promoted` + soft-skip rows are hard requirements | opencode | **Agree** | DoD-2 |
| O4 | Lock CSV at pack root; operators script paths | opencode | **Agree** | §2.4 path lock |
| O5 | Ledger init fail in `full` must fail closed | opencode | **Agree** | DoD-4; lock 3 |
| O6 | Long-term unify pst-writer + eml_pack event vocab | opencode | **Decline (this track)** | Opportunity only; would expand 0089 |
| A1 | `EmlAttachEvent` on `EmlWriteResult`; CLI drains into sink | agy | **Agree** | §2.4 |
| A2 | Drain `soft_skip_attach_records` + `mark_promoted_winner` | agy | **Agree** | §2.4; DoD-2 |
| A3 | Path ambiguity `--out` vs REPORTS | agy | **Agree (lock pack root)** | §2.4 |
| A4 | `--ledger-path-mode` on unique-eml | agy | **Agree** | lock 4; DoD-1 |
| A5 | Identical header column list | agy | **Agree via constant** | `EXPORT_ATTACHMENTS_CSV_HEADER` (agy’s abbreviated list is incomplete — use the live constant including `volume_*` / `winner_promoted` / peers) |

---

## 3. In scope

1. CLI flags: `--attach-ledger`, `--attach-ledger-max-rows`, `--ledger-path-mode`.
2. `EmlAttachEvent` (+ reason mapping) in `eml_pack`; CLI maps to `AttachLedgerSink`.
3. Mode A soft-skip + `mark_promoted_winner` parity with unique-pst.
4. Tests: attach fail → CSV + identical header; Mode A rows; ledger `off` still exit 64; ledger init fail fail-closed; row-cap truncated marker.
5. Close `D-0073-eml`.

## 4. Out of scope

- Redesigning unique-pst ledger (`0073` Complete).
- GUI wizard attach-ledger UI (`D-0073-gui`).
- `--also-eml` co-export (`D-0071-also-eml`).
- Changing EML MIME multipart format.
- Unifying `pst-writer` `AttachEventSink` with `EmlAttachEvent` (future).

## 5. Preconditions & dependencies

- **P1:** 0073 attach ledger types + 0078 unique-eml exit/`fidelity` counters.
- *Verified:* `AttachLedgerSink` exists; `eml_pack` has counters only; no engine→CLI dep.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Crate cycle | DTO in engine; sink in CLI |
| Second reason dialect | Phase 0 mapping table; generic fallback code |
| Double-counting vs counters | Ledger additive; counters remain classify source of truth |
| Perf on huge skip sets | Existing row-cap + truncated marker |

## 7. Definition of Done

- [ ] **DoD-1 — Flags:** `unique-eml` accepts `--attach-ledger`, `--attach-ledger-max-rows`, `--ledger-path-mode` aligned with unique-pst; default `full`.
- [ ] **DoD-2 — CSV:** Soft-fail/skip **and** Mode A `soft_skip_attach_records` produce `export_attachments.csv` at `--out/export_attachments.csv` with **identical header** to unique-pst (`EXPORT_ATTACHMENTS_CSV_HEADER`); `mark_promoted_winner` wired; injection-safe cells.
- [ ] **DoD-3 — Cap:** Row-cap + truncated marker behavior matches 0073.
- [ ] **DoD-4 — Exit:** Exit 64 / `fidelity` / counters correct with ledger on or off; ledger init failure in `full` **fail-closed**.
- [ ] **DoD-5 — Deferred:** `D-0073-eml` closed.
- [ ] **DoD-6 — Recorded:** `review.md`; conductor **Completed**; ledger TX committed.

## 8. Verification commands

```powershell
cargo test -p dedup-engine -- eml_pack
cargo test -p pst-dedup-cli -- unique_eml
cargo test -p pst-dedup-cli -- export_exit_0078
cargo fmt --all --check
cargo clippy -p dedup-engine -p pst-dedup-cli --all-targets -- -D warnings
ledgerful verify
```
