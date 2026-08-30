# 0113 — Produce checklist (DAT-only chrome wizard)

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do not re-open unique-export (0108–0109),
> matter-home overview math (**0110**), first-pass queue (**0111** / **0117**),
> three-pane coding (**0112** / **0118**), zpdf (**0114**), OPT (**0115** parked),
> or Process fold (**0116**). Do not vendor `C:\dev\dedupe-frontend`.
> Do not mint a BCC-default track.

- **Track ID:** 0113-ProduceChecklist
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** Hermes `E-Discovery — ideal frontend` produce checklist. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-08-30); do **not** chase it at execute.
- **Cross-repo contract:** mock at `C:\dev\dedupe-frontend` is research only (layout density, not tokens, not TIFF/OPT).
- **Status:** Completed
- **Depends on:** **0112 Completed** (PR **#115** / `81a3aad`) · **0110 Completed** (PR **#111** / `5a76f0b`) · produce **0040** (`matter-produce` `run_produce`) · QC **0041** (`matter-qc` `run_production_qc` + `check_qc_gate_for_pack`) · profiles **0060** · Desk Solo produce **0064** (profile + `bates_start`) · privilege log **0031** (`export_privilege_log`) · `matter-core` schema **v39**
- **Spec authored:** 2026-08-30 (placeholder → Ready)
- **Series:** O (Review chrome) — fourth track
>
> **Closes / absorbs:** `D-0113-produce-checklist` (this track). Partial chrome absorb of **D-0040-04** (volume `privilege-log.csv`, not `PRIVILEGE/` folder) and **D-0031-09** (chrome volume log ControlNumber = Bates when a production_item exists). Does **not** close D-0040-01 / D-0060-04 (0115), D-0032-01 / D-0034-02 (0114), D-0040-10 slipsheets, D-0031-03 category logs, D-0117, D-0118.
> **HITL:** owner launches the **release** EXE, opens a **synthetic** 3-item family matter (one withheld, one warning, one clean produce). INC* unique-pst is **not** a gate. Codesign is **D-0062-codesign**.
>
> **Last-PR fold-in (2026-08-30):** PRs **#116, #115, #114, #113**. Disposition in §2.8. Three **0112** window Bugbot items **minted 0118**. Queue Bugbot stays **0117**. Catalog lock already folded in 0112.
>
> **Review fold-in (2026-08-30):** `opencode-review.md` + `agy-review.md`. Disposition in §2.10 and `foldin-note.md`. Locks: privilege-log `filter_ids` = produced ∪ withheld-in-scope; `order_ids_family_together` preserves first-occurrence family order; warning overrides are **session/payload** (audit is evidence only; no audit-query helper); QC + produce share `scope=item_ids` + the same effective pack.
>
> **Stack lock (inherit 0110–0112):** Tauri **2** + Leptos **0.8 CSR** on **stable** Rust. Plex / paper / cool chrome. Red = privilege / withhold / **blocker** only. No daemon. **No `process-runner`.** No 0117/0118 ID reuse for this wizard.

---

## 1. Objective

Replace the **0110** `/matters/:id/produce` stub with a live **produce checklist** on the same `dedupe-chrome` EXE: five steps (Set / Number / Format / Burn / Pre-flight) wired to `matter-qc` + `matter-produce`. Default set is **responsive AND NOT withheld**. Privilege-in-set is a **hard block**. Volume is **DAT + natives + text** (`DATA/load.dat`, not a fake `DATA.dat`). No OPT, no TIFF, no page-level Bates.

This advances **unique-export / production defensibility** by putting counsel-facing Finalize on the **same** withhold + redacted-text + QC-fingerprint gates Desk/CLI already enforce — not a second packaging pipeline, not a mock that ships `IMAGES/` with nothing in it.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

**0112 Completed** (PR **#115** / `81a3aad`): three-pane coding writes `item_codes` / `item_privilege` orthogonally so a produce set of `responsive AND NOT withheld` is meaningful. Home **Produced** chip and the window Bates field still say `—` / `0113`. Unique-export Series S is closed. The remaining counsel gap after coding is **honest produce**.

### 2.2 Live APIs (plan-time 2026-08-30, HEAD `d9644dc`; re-verify at execute)

| Surface | Fact |
|---|---|
| `crates/matter-core/src/schema.rs` | `SCHEMA_VERSION == 39`. **No schema bump this track.** |
| `matter-produce` | `run_produce(matter, job_id, params, cancel, progress)` — Option C (does **not** `create_job`). Scopes `review_corpus` / `item_ids`. `bates_start` **required** ≥ 1 (never stored in a profile). Default profile `us_concordance_native_text_v1`. `require_qc_pass` default **true** via profile. `fail_if_withheld` default **false** (skip). Chrome **must** send `fail_if_withheld=true`. |
| Volume layout (live) | `<vol>/DATA/load.dat` (UTF-8 BOM, þ/¶/®) + optional `load.csv`; `NATIVES/`; `TEXT/`; `README.txt`. Paths Windows-style (`NATIVES\PROD000001.eml`). **Not** `DATA.dat` at volume root. |
| Bates on DAT | `BEGBATES` = `ENDBATES` = `CONTROL_NUMBER` (one number per native). Confirmed `dat.rs` / `run.rs`. Page-level Bates **does not exist** until **0115**. |
| Selection order | `review_corpus`: `ORDER BY COALESCE(review_order, 999999999), id`. `item_ids`: **preserves first-occurrence order**. Family expand (engine) then **sorts ids** (lossy vs parent-first). Chrome must pass **already ordered** `item_ids` and `expand_family=false`. |
| `matter-qc` | `run_production_qc` + `check_qc_gate_for_pack`. `withheld_in_selection` default **error**. `passed` = zero Error findings (warnings allowed). Fingerprint = sorted-id SHA-256 + `#pack=<pack_id>`. |
| Packs | `qc_default_v1` (legacy alias `default_production_qc_v1`); `qc_strict_privilege_v1`; `qc_native_heavy_v1`. |
| `FilterSpec` | Flat AND. `FILTER_SPEC_VERSION == 1`. `preset_responsive()` = `code any_of [responsive]`. `preset_withheld()` = `privilege_withhold eq true`. **No** produce preset today — this track **adds** `FilterSpec::preset_produce_responsive()` (§3.3). `include_family` expands after hits; outer rows still `in_review=1`. Nested OR is **D-0028-02** (out). |
| `list_items_filtered_thin` | Full `ReviewListRow` (includes `family_id` / `parent_item_id` / `review_order`). **No** `list_item_ids_filtered` today — this track **adds** it. Do **not** extend `ReviewListRow`. |
| `production_sets` / `production_items` | Schema v20. **No** public `list_production_sets` / `latest_control_number` / `count_produced_items` on `Matter` today (produce crate uses `connection()` internally). Chrome **must not** `connection()` SQL — this track **adds** thin helpers (§3.4). |
| `export_privilege_log` | Document-by-document CSV. ControlNumber = **item_id**. Formats stored: `standard` (default), `automated_metadata`, `category`. **`category` is not implemented** (0031 spec; Desk ComboBox excludes it and falls back to `standard`). `description_required` lives on `privilege_protocol`. |
| Overview | `load_case_overview` has **no** produced count. Home chip is hardcoded `—` / `0113`. |
| Chrome host | Produce route is `ProduceStub`. Commands: 0110 four + 0111 six + 0112 window set. Actor `"chrome"`. Encrypted → `encrypted`, no `open_*`. Workers via `join_worker`. |
| Chrome deps | `matter-core` + `matter-search` only. This track **adds** `matter-produce` + `matter-qc`. **Forbidden:** `process-runner`, `dedupe-desk`, `pst-reader`, `pst-writer`, `matter-service`, `zpdf`. |
| Desk analog | egui Produce dialog: profile + Bates prefix/start + fail-if-withheld + expand-family + `require_qc_pass` + preflight. Starts a **process-runner** job. Chrome does **not** copy that job host. |
| CI | `chrome-ui` job: wasm32 + `trunk` **0.21.14** + `cargo test -p dedupe-chrome`. Keep it. |
| MS-PST | **N/A this track.** |

### 2.3 Mock + Hermes (research only; re-verified 2026-08-30)

`C:\dev\dedupe-frontend/frontend/src/pages/produce.rs`: three panes (production sets + protocol | five steps | pre-flight cards). Steps: **Set / Number / Format / Burn / Pre-flight**. Source search copy: `Responsive NOT withheld`. Family-together checkbox **locked on**. Blocker vs warning cards; warning override requires reason + signed audit footnote. Finalize disabled while blocked.

**Steal:** pane density, five-step skeleton, blocker/warning/passed badges, override+reason, padlock Finalize, “every override is audited.”

**Do not copy / do not fake:** coral `#ec3013`; `ACME0002` Bates; **page counts**; TIFF G4 / 300 dpi / OPT; “14 colour-sensitive pages”; slipsheets; “EDRM category B image + native”; pending-mark QC queue; invented VOL001–003 rows; `IMAGES/`.

Hermes **page-level Bates default** is the **0115** lock (image productions). Concordance DAT for **natives** is one row per document with BegBates/EndBates (often equal when there are no page images). A 2026 ESI protocol example still pairs DAT with OPT **when images exist**. This track ships the DAT half only.

### 2.4 Plan-time crate pins (re-verify at execute)

| Crate | Plan-time | Rule |
|---|---|---|
| `tauri` | **2.x** stable (`Cargo.toml` `version = "2"`) | Reject **3.x / pre-release**. |
| `leptos` / `leptos_router` | **0.8** (`features = ["csr"]`) | Reject 0.9-beta. |
| `zpdf` | 0.13.x | **0114 only.** Do not add. |
| Rust | **stable** | Do not switch to nightly. |

### 2.5 Tools / recall

Ran from `C:\dev\Dedupe` (0112 already merged; tree otherwise clean except untracked scratch):

- `ai-brains preflight --summary` (inited; 3979 pinned).
- `ai-brains sync query` / recall: 0060 no image/OPT; `bates_start` required; withhold/redacted-text hard invariants; 0111 Control# is `review_order` not Bates. Stale “Desk remains egui for produce” superseded by this chrome wizard (Desk Process still stays until **0116**).
- `ledgerful doctor --json` `readyForPublish: true`; `ledger status --compact` **0 pending / 0 unaudited drift** before this tx. Doctor: phantom-promote, sig-pin, completion-unreachable, impact-stale — none block planning.
- Ledger tx for this planning pass: `a0276927-0e11-49b2-8ceb-e12bd65b8207`.
- `scan --impact` after spec write (docs/conductor + 0118 mint expected **LOW**).

### 2.6 How this advances the north star

Not UI polish: Finalize must refuse withheld items and refuse produce without a **fresh passed** QC fingerprint for the **same** ids + pack. Lying “export IMAGES/” or a Bates that is not in `production_items` fails the track. Unique-pst CLI is unchanged.

### 2.7 Crate boundaries

| Crate | This track |
|---|---|
| `dedupe-chrome` | Wizard UI + host commands. Blocking workers. |
| `matter-core` | Thin **read** helpers + `FilterSpec` preset + optional privilege-log ControlNumber map. **No** packaging. **No** chrome `connection()` SQL. |
| `matter-qc` | Unchanged rules (call `run_production_qc` / gate). Chrome-only extra blockers are **not** new pack rules. |
| `matter-produce` | Unchanged packaging (call `run_produce`). Do **not** change engine `fail_if_withheld` default (Desk/CLI skip stays). |
| `dedupe-desk` / `process-runner` | **Do not depend.** Do not fold Process. |

### 2.8 Last-PR Cursor comments (mandatory)

| PR | Surface | Disposition |
|---|---|---|
| **#116** | docs 0112 Completed registry | none |
| **#115** | 0112 window Bugbot (3) | **Valid, not this track.** Minted **0118** (stale async load High; post-save snapshot Medium; unused `#[test]` Low). Do **not** fold into produce. |
| **#114** | docs 0111 Completed | none |
| **#113** | queue Bugbot (3) + catalog write-lock | Queue → **0117** (already). Catalog lock **folded in 0112**. Do not restole. |

### 2.9 External currency (plan-time)

- Concordance DAT: one **document** row; BegBates/EndBates; natives + text directories; OPT is the **image** companion (N/A until 0115).
- Native-file productions copy/rename natives and do not burn page Bates.
- N/A: MS-PST structures.

### 2.10 Review fold-in (2026-08-30)

Sources: `opencode-review.md`, `agy-review.md`. Live checks: `export_privilege_log` `IN (filter_ids)` (`privilege.rs`); `audit_events` INSERT + chain verify only (no action reader); produce set status `"complete"` / `"complete_with_errors"` (`run.rs:1027-1031`); fingerprint sorts ids (`qc.rs:37-51`).

| Id | Disposition |
|---|---|
| opencode-M1 / agy-M1 | **Fold** — §3.7 `filter_ids` = produced ∪ withheld-in-scope; DoD-4 withheld row |
| opencode-M2 / agy-M2 | **Fold** — §3.4 first-occurrence family order; DoD-4 cross-family |
| opencode-M3 | **Fold (b)** — session/payload gate; audit append is evidence; no audit reader; no schema bump |
| agy-M3 / opencode-m3 | **Fold** — QC `scope=item_ids` + same `effective_qc_pack_id` as produce |
| opencode-m1 / agy-m1 | **Fold** — `complete` ∪ `complete_with_errors`; **exclude `failed`** |
| opencode-m2 / agy-m2 | **Fold** — `COUNT(DISTINCT item_id)` |
| opencode-m4 | **Fold** — produce_start re-resolves; membership drift → stale blocker; never silent re-QC |
| opencode-m5 | **Fold** — DoD-2 names both blockers |
| agy-O1 | **Fold** into M3/m4 — overrides keyed to current findings |
| opencode-O1 | **Fold** — `produce_page` is **read** |
| opencode-O2 | **Fold** — home tooltip + queue `— · 0113` stub copy |
| opencode-O3 | **Fold** — `control_numbers` also fills ParentControlNumber |

---

## 3. In scope

### 3.1 Route + chrome layout

Replace `ProduceStub` on `/matters/:id/produce`. Four 0110 tabs stay. Queue + window routes unchanged.

Layout (steal mock density, Plex tokens):

```text
LEFT: production sets (thin rows from Matter helper)
CENTER: steps 1–5
  1 Set     — default search + count (no fake page count)
  2 Number  — prefix + bates_start; family-together locked
  3 Format  — NATIVES + TEXT + DATA/load.dat; OPT/TIFF greyed “0115”
  4 Burn    — CAS redacted text (0032); PDF geometric burn “0114”
  5 Pre-flight — live QC + chrome extras; Re-run
FOOT: Finalize (primary) disabled while any blocker
```

Home **Produced** chip: integer from `count_produced_items`. Drop the `0113` subtitle **and** the `title="Produce checklist ships in track 0113"` tooltip. Zero is `0`, not `—`, once this track ships (empty matter with no productions is `0`). Queue produced/Bates cell (`queue.rs` `— · 0113`) becomes `—` (no Bates column this track — do **not** extend `ReviewListRow`).

Review window Bates: `latest_control_number(item_id)` or `—` with empty note (not `"0113"`). Control# remains `review_order`. Do **not** invent `ACME0002`.

### 3.2 Default produce set (normative)

Host default `FilterSpec` (also `FilterSpec::preset_produce_responsive()` plus flags):

| Field | Value |
|---|---|
| `scope` | `review_corpus` (`in_review = 1`) |
| `include_family` | **true** (locked in UI; operator cannot turn off this track) |
| conditions | `code any_of [responsive]` **AND** `privilege_withhold eq false` |

Operator may switch source to **entire review corpus** (no responsive condition, still `privilege_withhold eq false` + `include_family`). That is an explicit choice, still excluding withheld at the filter.

**Do not** silently drop withheld family members after expand. If `include_family` pulls a withheld child, they stay in the candidate list → QC `withheld_in_selection` **error** → chrome **blocker**. Counsel must clear withhold or stop expanding (expand is locked on → they must change coding / withhold, not a silent skip).

Host **re-resolves** ids from the FilterSpec at QC and at produce (do not trust a stale id list from the WebView as the only membership). Tests may pass `item_ids` directly. After resolve, host runs `order_ids_family_together` (**first-occurrence family order**, parent first within a family) and uses that order as `ProduceParams.item_ids` with `expand_family=false`.

`produce_start` compares membership (set equality; fingerprint is order-insensitive) to the last QC run. Membership drift → `qc_gate` **stale** blocker. **Never** silently re-run QC inside `produce_start`. When membership is unchanged, pass the **same ordered list** used for that QC run.

### 3.3 Engine params from chrome (normative)

| Param | Chrome value |
|---|---|
| `scope` | `item_ids` |
| `item_ids` | ordered resolved list |
| `bates_prefix` | operator (default `PROD`) |
| `bates_start` | operator, required ≥ 1; **no silent 1** if the field is empty |
| `production_profile` | default `us_concordance_native_text_v1`; picker from `list_production_profiles` |
| `qc_pack_id` | **same** `effective_qc_pack_id` as the QC run (from the selected profile; do not default-pack QC then profile-pack produce) |
| `fail_if_withheld` | **true** (belt; QC already errors) |
| `require_qc_pass` | **true** (no UI bypass) |
| `expand_family` | **false** (already expanded + ordered) |
| `include_csv_twin` | true |
| `export_eml_if_missing_native` | true |
| `output_dir` | null → matter `exports/productions/<name_or_stamp>/` |

`Matter::create_job("qc")` / `create_job("produce")` then `run_production_qc` / `run_produce` on `std::thread` via `join_worker`. **No `process-runner`.** Cancel/progress for multi-GB is **0116** residual (`D-0113-long-job`). DoD fixture is small; blocking IPC is acceptable.

Number step: optional **hint** of latest set `next_seq` for the same prefix (**D-0060-03** not closed — start stays explicit).

Page-level vs doc-level segmented control: **doc-level selected and locked**. Page-level disabled with copy “Page-level Bates ships with image productions (**0115**). This DAT volume uses one Bates per native (`BEGBATES=ENDBATES`).”

### 3.4 Matter-core helpers (this track adds)

All `Matter` methods; chrome never `connection()`.

| Helper | Contract |
|---|---|
| `FilterSpec::preset_produce_responsive()` | `code any_of [responsive]` + `privilege_withhold eq false`; caller sets `include_family`. |
| `list_item_ids_filtered(&FilterSpec) -> Vec<String>` | Same WHERE as thin list; **ids only**; same ORDER BY as `list_items_filtered_thin`. |
| `order_ids_family_together(ids) -> Vec<String>` | **Families in first-occurrence order of the input list** (or min `review_order` in that list per family — first-occurrence is the lock). Within a family: parent (`parent_item_id` null) first, then children by `review_order` NULLS LAST, then `id`. Do **not** sort on raw `family_id` (opaque strings scramble reviewer sequence). Unknown ids dropped. Null `family_id` = its own group at first occurrence. |
| `count_produced_items() -> u64` | `COUNT(DISTINCT pi.item_id)` where `pi.status='ok'` **and** `ps.status IN ('complete','complete_with_errors')`. **Exclude** `failed` / `partial` (aborted volumes are not counsel-facing). |
| `list_production_sets_thin() -> Vec<…>` | id, name, status, produced_ok_count, bates_prefix, next_seq, output_root (path ok — not mail). |
| `latest_control_number(item_id) -> Option<String>` | Latest set with `ps.status IN ('complete','complete_with_errors')` **and** `pi.status='ok'`; skip `SKIP_*`. Order `produced_at DESC`, tiebreak `ps.id DESC`. **Exclude `failed`** (items packaged before a `fail_if_withheld` abort must not show as Bates). |
| `PrivilegeLogExportParams.control_numbers` | Optional `id → Bates` map. When set, **ControlNumber** uses the map for that row’s id, else item_id. **ParentControlNumber** uses the map for `parent_item_id` when present, else the raw parent id. |

No schema migration.

### 3.5 Host commands

Register in `generate_handler!` + `allow-*` in `capabilities/default.json` (rebuild autogen tomls). No `fs:default`.

| Command | Open | Role |
|---|---|---|
| `produce_page` | **read** | sets + resolved count + default filter JSON + last QC gate summary + next_seq hint + produced_count |
| `produce_qc_run` | **write** | Resolve ids; `QcParams { scope: "item_ids", item_ids, expand_family_for_scan: false, pack_id: Some(effective_qc_pack_id of the **same** selected profile as produce_start) }`; `create_job("qc")` + `run_production_qc`; return findings (item_id + rule_id + severity + message only — **no** subject/body/path) |
| `produce_start` | **write** | Re-resolve ids; if membership ≠ last QC set → stale blocker (do **not** silent re-QC). Refuse if blockers (§3.6). Require override **payload** covering every current Warn finding (`recorded_by` + `reason` + `rule_id` + `item_id` or set). Then append `produce.warning_override` audit (evidence only — **do not** query `audit_events` to gate). `create_job("produce")` + `run_produce` with **those same ordered ids** + same pack; on success export privilege log into volume as `privilege-log.csv` |
| `matter_overview` | read (existing) | add `produced: u64` from `count_produced_items` on the **same** `open_for_read` |
| `review_document` | read (existing) | fill `bates` from `latest_control_number`; `bates_note` empty or `"from production"` — never `"0113"` |

Encrypted / missing root: same `encrypted` / `not_found` as 0110–0112. Actor for jobs/audit: `"chrome"`. Warning overrides live in the **UI/session payload** (lost on app restart → operator re-records). Audit rows are provenance, not the gate. Key: `rule_id` + `item_id` (or set-level) against **currently resolved** findings — a filter change that adds a new warning is not covered by an old override.

### 3.6 Blockers vs warnings (normative)

**Engine Error findings** (default pack) → chrome **BLOCKER** (padlock). Cannot override. Includes `withheld_in_selection`, `broken_family_orphan_child`, `redacted_text_missing`, `missing_native` (non-email), `empty_selection`, `only_withheld`, and Error-severity `missing_text`.

**Engine Warn findings** → chrome **WARNING**. Finalize stays disabled until **each current** warning has a non-empty override in the **payload** (`recorded_by`, `reason`, `rule_id`, `item_id` or `set`, qc run id). Empty reason refused. Host then `append_audit` `produce.warning_override` for each. **No** `Matter` audit-query helper this track (`audit_events` has INSERT + hash-chain verify only; schema bump forbidden). Restarting the EXE clears UI overrides.

**Chrome-only extras** (not new `matter-qc` rules; do not change pack JSON):

| Extra | Severity | When |
|---|---|---|
| `uncoded_in_set` | **blocker** | Candidate lacks a responsiveness group code (`responsive` / `not_responsive` / `needs_second_look`). Family membership is not a determination. |
| `privilege_log_blank` | **blocker** | `privilege_protocol.description_required` and `export_privilege_log` would count `blank_description_count > 0` for withheld/asserted rows in scope. |
| `qc_gate` | **blocker** | `check_qc_gate_for_pack` is Missing / Failed / Stale after the last run (or before first run). |

Privilege-in-set is **never** a warning. `require_qc_pass=false` is **not** a chrome control.

“Open in review” on a finding navigates to `/matters/:id/review/:docId` (0112 window). Do not implement a new QC queue.

### 3.7 Privilege log co-export

After a **successful** produce, host calls `export_privilege_log` to `<vol>/privilege-log.csv`.

**Normative `filter_ids`:** **produced ∪ withheld-in-scope** (scope = `review_corpus` when the produce FilterSpec is review-corpus; `entire_matter` if the operator chose entire-matter). Do **not** pass produced-only ids — that silently drops the withheld documents the log exists to account for. Eligibility still 0031: `include_on_log=1` + status ∈ asserted/under_review/partial_redaction.

`control_numbers` map: Bates for **produced** ids only. Withheld rows keep ControlNumber = **item_id**. ParentControlNumber: map lookup on `parent_item_id` when the parent was produced.

Log format radio: **standard** (default) / **automated_metadata**. `category` shown disabled with “not implemented (**D-0031-03**)” — do **not** emit a collapsed log.

Do not put `item_privilege.description` on the DAT (already forbidden).

### 3.8 Burn / Format honesty

- Burn = confirm 0032 `redacted_text_sha256` for items with `redaction_count > 0`. Engine already fail-closes. Copy: “Only CAS redacted text is packaged. Geometric PDF burn is **0114**. Highlights never burn.”
- Format chips: NATIVES, TEXT, DAT **on**. TIFF / PDF image / OPT **off** and labeled **0115**. Do not create `IMAGES/` or `IMAGE.opt`.
- Slipsheets **off** (**D-0040-10**). Withheld are not in the volume.

### 3.9 Tokens / a11y / CSP

Inherit 0110–0112. Blocker badges may use privilege red (`#9B2C2C`). No `#ec3013`. CSP unchanged. Skip-to already includes produce tab via matter home.

### 3.10 Hygiene

- Production: no `unwrap` / `expect`. `main` still returns `Result`.
- Never mutate source PSTs. Never commit client PSTs, `output/`, `evidence/`, or matter folders with mail.
- Tests: `tempfile` only. Fixture order: `insert_family` → parent → children; `ensure_default_review_set` then `in_review`; `seed_default_codes`; `put_bytes` for a tiny native/text. No client PST.
- `ui/` stays workspace-excluded.

---

## 4. Out of scope (do NOT do here)

- **0118** review-window stale-load / post-save snapshot / path_id `#[test]` (PR **#115** Bugbot).
- **0117** queue header/spacer / vacant page / arrow `scroll_top`.
- **0114** zpdf / Image raster / geometric burn.
- **0115** TIFF G4, Opticon OPT, LFP, page-level Bates, `IMAGES/`.
- **0116** process-runner, long-job progress/cancel, swallowing egui Process.
- Slipsheets / placeholders for withheld (**D-0040-10**).
- Category / thread-collapsed privilege logs (**D-0031-03**).
- CP1252 DAT (**D-0040-06**), notes on DAT (**D-0040-08**), clawback (**D-0031-06**).
- Changing engine `fail_if_withheld` / `require_qc_pass` **defaults** (Desk/CLI).
- Schema bump, unique-pst flags, BCC-default.
- Encrypted open/passphrase. Axum daemon. Leptos SSR. `tauri` 3.x. leptos 0.9.
- Vendoring mock tokens. Chrome `connection()` SQL. Extending `ReviewListRow`.
- Changing `ensure_item_privilege_conn`.
- Remote produce HTTP (**D-0064-02**).

---

## 5. Preconditions & dependencies

- **P1 (blocking):** 0112 window + produce stub present. `SCHEMA_VERSION` 39. `run_produce` / `run_production_qc` / `check_qc_gate_for_pack` / `export_privilege_log` / `FilterSpec::preset_responsive` still pub. This track **adds** the §3.4 helpers. Re-verify at execute.
- **P2:** Windows WebView2; CI `chrome-ui` stays.
- **P3:** `wasm32-unknown-unknown` + `trunk` 0.21.14.
- *Verified to date:* §2.2–2.4, §2.8. Last-PR: 0118 minted; 0117 untouched.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Fake OPT / IMAGES | Format step greys 0115; tests assert no `IMAGES` / `*.opt`. |
| Page-level Bates without pages | Control locked to doc-level; DAT `BEGBATES=ENDBATES`. |
| Privilege leak in set | Filter excludes withheld; QC error; `fail_if_withheld=true`; no chrome override. |
| Silent family drop | Do not strip withheld after expand; blocker instead. |
| Stale QC authorizing produce | Host re-resolves ids; `require_qc_pass` + pack fingerprint. |
| Warning override without reason | Finalize disabled; empty reason refused; payload gate (audit is evidence only). |
| Privilege log omits withheld | `filter_ids` = produced ∪ withheld-in-scope; DoD-4 asserts the withheld row. |
| Family Bates scramble | First-occurrence family order; DoD-4 two-family relative order. |
| QC pack / scope mismatch | QC and produce share `item_ids` + `effective_qc_pack_id`. |
| Two pipelines | No process-runner; engines called in-process. |
| Chrome SQL | Helpers on `Matter` only. |
| Long produce freezes IPC | DoD small fixture; residual **D-0113-long-job** → 0116. |
| CATEGORY log | Disabled; D-0031-03 stays. |
| Coral / mock port | Tokens inherit 0110. |
| `DATA.dat` vs `DATA/load.dat` | Live layout wins. |

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Wizard replaces stub:** `/matters/:id/produce` is the five-step checklist (not “Produce checklist is 0113.”). Four 0110 tabs still work. Queue + 0112 window still work. Format step does **not** offer a working TIFF/OPT path. `dedupe-desk` still builds. No `process-runner` dep on `dedupe-chrome`.
- [ ] **DoD-2 — Default set + privilege-in-set:** Host tempfile: parent+2 children in review; seed catalog; code parent `responsive`; child A withheld; child B uncoded. Default produce FilterSpec count **includes** the withheld child because `include_family` (in_review). First `produce_qc_run` on that default set returns **both** Error `withheld_in_selection` **and** chrome `uncoded_in_set` (child B), and may include Warn `withheld_family_member`. `produce_start` **fails** (no volume natives for child A). Coding child A responsive and clearing withhold, plus coding child B responsive, then QC pass, then start succeeds. `fail_if_withheld` on the chrome params JSON is true. `produce_qc_run` used `scope=item_ids` and the same pack produce will use.
- [ ] **DoD-3 — Checklist gate:** After a passed QC with only Warn findings, Finalize stays disabled until the **payload** has `recorded_by` + `reason` for each current warning (host does not read `audit_events` to decide). Error findings keep Finalize disabled even with a reason. Empty selection is a blocker. Re-run after adding a withheld item reports **stale** or new error — produce refused (**never** silent re-QC inside start). Changing the set so a new warning appears without a new reason keeps Finalize disabled (old overrides do not cover new `rule_id`+`item_id`).
- [ ] **DoD-4 — Volume + Bates + chip:** Successful produce writes `DATA/load.dat` (BOM + `BEGBATES` column), `NATIVES/`, `TEXT/`, and `privilege-log.csv` at volume root. **No** `IMAGES/` and **no** `IMAGE.opt`. DAT `BEGBATES==ENDBATES==CONTROL_NUMBER` for the produced native. Privilege log ControlNumber for a produced item is that Bates (not raw `itm_…` when a production_item exists). **Withheld-in-scope member appears in `privilege-log.csv` with ControlNumber = item_id** (no Bates). `review_document` on a produced item returns the same Bates (not `"0113"`). `matter_overview.produced` ≥ 1. Family order: parent control number < child control number (same prefix). **Two families:** first-seen family in the resolved list keeps a lower Bates prefix-range than the later family (`order_ids_family_together` does not sort on raw `family_id`).
- [ ] **DoD-5 — Helpers + CI:** `list_item_ids_filtered` / `order_ids_family_together` / `count_produced_items` covered by `dedupe-chrome` or `matter-core` tests (tempfile). New commands have `allow-*`. Encrypted root → `encrypted`. `cargo test -p dedupe-chrome` + `cargo test -p matter-core --lib` (or targeted) + `cargo check -p dedupe-desk`. Workspace fmt/clippy/test + `chrome-ui` trunk stay green. No production `unwrap`/`expect`. CSP unchanged. No schema bump.
- [ ] **DoD-6 — Recorded:** `review.md`; registry **Completed**; CHANGELOG Unreleased sentence; `D-0113-produce-checklist` closed; ledger committed (`FEATURE`). Unblocks nothing that was waiting except counsel DAT from chrome. **0114** / **0115** / **0116** / **0117** / **0118** stay as they are.

**Owner HITL (not CI):** release EXE, synthetic 3-doc family, withheld blocker visible in red, warning override requires text, clean produce opens `load.dat`, home chip not `0113`. INC* waived.

---

## 8. Verification commands (reference)

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p dedupe-chrome
cargo test -p matter-qc
cargo test -p matter-produce
cargo check -p dedupe-desk
# rustup target add wasm32-unknown-unknown
# trunk build --config crates/dedupe-chrome/ui/Trunk.toml --release
```

---

## 9. Deferred (absorb / decline)

| Row | Disposition |
|---|---|
| **D-0113-produce-checklist** | **Absorb / close** on Implement. |
| **D-0040-04** privilege log in volume | **Partial** — `privilege-log.csv` at volume root. Not `PRIVILEGE/` folder. Residual folder layout stays. |
| **D-0031-09** Bates on privilege log | **Partial** — chrome volume log uses `control_numbers` map. Desk/CLI export still item_id. Residual for those surfaces. |
| **D-0031-03** category logs | **Decline.** Radio disabled. |
| **D-0040-01** / **D-0060-04** TIFF/OPT | Remain parked; **0115**. |
| **D-0040-10** slipsheets | **Decline.** Skip withheld. |
| **D-0040-06** / **D-0040-07** / **D-0040-08** | **Decline.** UTF-8 BOM + ® + notes excluded. |
| **D-0040-09** / **D-0041-09** GUI smoke | Analog HITL. Residual stays. |
| **D-0041-03** auto-fix | **Decline.** Report-only. |
| **D-0041-11** incomplete_parent default error | **Decline.** Warn + override. |
| **D-0060-03** auto next Bates | **Partial hint only.** Start still required. |
| **D-0064-02** remote produce | **Decline.** |
| **D-0032-01** / **D-0034-02** | Remain; **0114**. |
| **D-0117-queue-virtualization** | Remain. |
| **D-0118-review-window-async** | **Minted** this pass; remain Proposed. |
| **D-0110-deny-unic** | Remain residual / upstream. |
| **D-0116-process-fold** | Remain. |
| **D-0113-long-job** | **Mint** as residual on Implement if blocking IPC is too slow; owner **0116**. Do not block DoD. |
| **D-0108-keepset-crc-retaint** | Unique-export. **Decline.** |
| **D-0062-codesign** | Release ops. **Decline.** |
| Last-PR #115 three window items | **Minted 0118.** |
| Last-PR #113 queue items | **0117.** |
| Mock TIFF / page Bates / slipsheets | **Decline** (0115 / D-0040-10). |
| Mock `tokens.css` retune | `C:\dev\dedupe-frontend` only. |
| opencode-M1 / agy-M1 produced-only log | **Folded** — §3.7 ∪ withheld. |
| opencode-M2 / agy-M2 family_id sort | **Folded** — first-occurrence. |
| opencode-M3 audit reader | **Folded (b)** — payload gate; no reader. |
| agy-M3 pack mismatch | **Folded** — shared pack + `item_ids`. |
| opencode-m1..m5 / agy-m1 m2 / O1–O3 | **Folded** as §2.10. |

---

## Series O index (do not reorder)

| ID | Item | After this plan |
|---|---|---|
| **0110** | Matter chrome + one overview command | **Completed** (PR **#111** / `5a76f0b`) |
| **0111** | Virtualized first-pass queue | **Completed** (PR **#113** / `3c4ca65`) |
| **0112** | Three-pane review window | **Completed** (PR **#115** / `81a3aad`) |
| **0113** | Produce checklist; DAT only | **Completed** (PR **#117** / `f192b2d`) |
| **0114** | zpdf raster + geometric redact | Proposed |
| **0115** | TIFF G4 + OPT | **Parked** |
| **0116** | Fold egui Process | Proposed |
| **0117** | Queue virtualization residuals (PR #113) | Proposed |
| **0118** | Review-window async residuals (PR #115) | **Proposed — placeholder** |

Next free conductor ID: **0119**.
