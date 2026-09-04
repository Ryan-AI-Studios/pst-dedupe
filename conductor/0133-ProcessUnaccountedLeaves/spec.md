# 0133 — Process unaccounted leaves (extract remaining)

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Do **not** steal **0122** Busy / extract-all queue / `job_for_orphan` string-lock,
> **0126** jobs-grain / Dupes-NIST `—` / unaccounted **arithmetic**, **0119** produce latch.
> Do not vendor `C:\dev\dedupe-frontend`. No BCC-default. Do not steal **0100–0104**.
> Do **not** fudge `unaccounted_for` to 0.

- **Track ID:** 0133-ProcessUnaccountedLeaves
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\`
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-09-03); do not chase it.
- **Cross-repo contract:** mock at `C:\dev\dedupe-frontend` is **workflow research only**. Steal “reconciliation names the gap + a path to review-ready.” Do **not** port ACME 0-unaccounted theater.
- **Status:** Ready — not started
- **Depends on:** **0126 Completed** (PR **#145** / `73c0496`) · schema **v41** (no bump)
- **Spec authored:** 2026-09-03 (placeholder) → **2026-09-03 Ready** (plan-time HEAD `cc88576`)
- **Series:** V (chrome–mockup operational parity)

> **Closes / absorbs:** `D-0133-unaccounted-leaves`. Does **not** close D-0116-drop (0134), D-0116-workflow, D-0024-01, D-0062-codesign, D-0067-embedded-depth.
> **HITL:** owner chrome EXE: ingest two synthetic PSTs (or operator-local INC* — never git), leave unextracted → Unaccounted-for **2** with **named** leaves + **Extract remaining**; extract one → **1** + one remaining name; extract both while idle → **0** on the **0126** rule. Source PSTs read-only. INC* unique-pst is **not** a gate. Codesign is **D-0062-codesign**.

---

## 1. Objective

Make Process reconciliation **name** unextracted PST inventory (chrome HITL 2026-09-03: `unaccounted_for = 2` after ingest, no names, no CTA) and offer **Extract remaining** that queues **only those leaves** on the existing extract-all drain — without painting mock “0 unaccounted” while leaves remain.

Silent zero is the same honesty class as a silent unique-export drop. Unique-export is unchanged.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

Operator chrome HITL **2026-09-03** on `INC0102784.pst` + `INC0102784-2.pst`: ingest succeeded (`entries_ok=1` each); Review stayed 0; minus-stack **Unaccounted-for 2** with no names and no CTA. Mockup shows a continuous ingest→items→review-ready story. Chrome’s **0126** formula is already correct. The miss is **operator workflow**, not the number.

### 2.2 Live APIs (plan-time 2026-09-03, HEAD `cc88576`; **re-verify at execute**)

| Surface | Fact |
|---|---|
| `matter-core` `SCHEMA_VERSION` | **41**. No bump. |
| `dedupe-chrome/src/process.rs` `unaccounted_for` | `pst_gap` = inventory ids **not** in successful `extract_pst` checkpoint `pst_item_id` (stage `pst_extract`) **plus** `failed_unlogged` (Failed jobs with empty `item_errors_for_job`). **0** only when idle + every leaf extracted (or no leaves) + no failed-unlogged. **Do not rewrite.** |
| Test | `unaccounted_nonzero_when_pst_inventory_without_extract` already encodes 2→1→0 on the **number**. |
| `ProcessPageResponse` | Has `unaccounted_for`, `pst_inventory`, `sources`. **No** unextracted-id list today. WASM mirror `ui/src/invoke.rs` — new fields **must** `#[serde(default)]`. |
| UI minus-stack | Prints the **number only** (`process.rs` Unaccounted-for `<dd>`). **Open review-ready** disabled when `in_review==0`. |
| Extract all | `extract_all` maps **all** `pst_inventory` into `extract_queue`. Guard `extract_all_should_start(queue_len, snapshot_busy)` **before** queue writes (**0122** frozen). String-lock: `is_orphan_running(&job_for_orphan, &progress.get())`. |
| Extract remaining | **Must not** blindly reuse today’s full-inventory queue. Filter to **unextracted** ids. Same Busy keep-queue / `busy_retry_pending` / drain. |
| Schema / jobs | `Job` has **no** `params_json` column. Checkpoints hold extract cursor (`ExtractCursor.pst_item_id`). |
| MS-PST | **N/A this track.** |

### 2.3 Product locks

Keep **0126 Unaccounted-for frozen** arithmetic. This track **labels** the PST gap and wires **Extract remaining**. Do not auto-start extract on ingest (**D-0116-workflow**). Do not drop Matters / Home / Add folder / Add ZIP / PST / Purview copy / job-grain table.

Named list length = **PST gap**, not necessarily `unaccounted_for`. If `failed_unlogged` contributes, footnote without inventing PST names (“N failed job(s) without item_errors — use job Resume”).

### 2.4 Plan-time crate pins (re-verify at execute)

| Crate | Plan-time | Rule |
|---|---|---|
| `tauri` | **2** | Reject 3.x. |
| `leptos` | **0.8.x** CSR | No major bump. |
| Schema | **41** | No bump. |
| chrome | **0.2.0-rc.1** | Keep. |
| Rust | **stable** | No nightly. |

### 2.5 Tools / last-PR comments (this Ready pass)

- `ai-brains preflight --summary` (inited). Recall: Series V placeholders pin `125cb374`; 0116/0122/0126 unaccounted + extract-all Busy. Sync query: ledger hits on 0116/0122/0126 Process — no new product constraint.
- `ledgerful doctor` readyForPublish; compact **0 pending**. `scan --impact` **LOW** (dirty conductor/deferred + `.claude` junction — **do not `git add` `.claude`**). Temporal couplings on board files are expected.
- PRs **#149 #148 #147 #146**: inline comments **0**; reviews **0**. Prior issue comments Bugbot **usage-limit** only. **Decline.** No new mint.

### 2.6 What we could not verify (closed at fold-in)

HITL lag is **DoD-3**. Live poller (`ui/src/pages/process.rs`) reloads `process_page` only on `finished_ok || missing_job`, and `finished_ok = was_busy && !snapshot_busy`. Fast ingest that completes between polls never sets `was_busy`, so `importing` sticks. **Trigger (locked):** on each progress poll, if any source `status == "importing"` **and** the snapshot is idle/terminal, `reload` — do not rely on `was_busy`. `start_kind`'s immediate reload after `process_start` is **not** sufficient (status is still `importing`).

### 2.7 Fold-in (2026-09-03)

`opencode-review.md` + `agy-review.md`. See `foldin-note.md`. Re-verify at execute: `unaccounted_nonzero_when_pst_inventory_without_extract` and `golden_path_ingest_profile_unaccounted_zero`.

---

## 3. In scope

1. Host: same `process_page_blocking` pass as `unaccounted_for` emits:
   - `unextracted_psts: Vec<ProcessPstRow>` (inventory rows whose ids are not in `extracted_pst_item_ids`)
   - `failed_unlogged: u64` (same `failed_jobs_without_item_errors` value folded into `unaccounted_for`)
   WASM `ui/src/invoke.rs` **must** use these exact names with `#[serde(default)]`. Do not rename.
2. Minus-stack: when `unextracted_psts` is non-empty, list **basenames** (`strip_extended_path`). Footnote iff `failed_unlogged > 0` (not a recompute of `unaccounted_for - names`). Copy: unextracted inventory, not missing messages.
3. **Extract remaining** visible iff `unextracted_psts` is non-empty; queues **those** leaves only. Reuse `extract_all_should_start`, drain, `busy_retry_pending` / `should_clear_busy_retry`, and **`apply_extract_start_err` only** for start failures. **Do not** add a second `extract_queue.set(Vec::new())` (0122 `include_str` lock count == 1). Keep **Extract all**.
4. Source `kind · status` from latest `process_page` after ingest reaches idle/terminal (**DoD-3** poller trigger in §2.6).
5. Tests: two inventory ids, extract none → 2 names; extract one → 1 name; extract both idle → 0 names and `unaccounted_for==0` (when `failed_unlogged==0`). Host formula tests unchanged.

## 4. Out of scope

- Painting unaccounted **0** while leaves remain (never).
- Changing `unaccounted_for` math.
- Per-source GB / drop-zone (**0134**).
- Jobs Source column filenames (**0135**).
- Exception vault (**0136**). Produce buttons (**0137**).
- OST/MBOX, NSRL RDS, ACME fake counts, BCC-default.
- Auto `profile_run` after extract.

## 5. Preconditions

- **P1:** 0126 minus-stack + 0122 extract-all in live chrome.
- *Verified:* `unaccounted_nonzero_when_pst_inventory_without_extract` encodes the number; UI prints count only.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Operator thinks Unaccounted-for is a defect | Copy: unextracted inventory, not missing messages |
| Extract remaining vs Extract all confusion | Remaining = unextracted only; same Busy guards |
| Count > named leaves | Footnote failed-unlogged; do not invent names |

## 7. Definition of Done

- [ ] **DoD-1** `unaccounted_for` formula unchanged (existing host tests still pass). UI names unextracted PST basenames when PST gap > 0.
- [ ] **DoD-2** Extract remaining starts extract-all **drain** under 0122 Busy rules; queues **unextracted** leaves only; two-fixture gap tests pass (2→1→0).
- [ ] **DoD-3** After ingest that finishes between polls (never observed busy), source `status` becomes host `ready` (or live terminal) without a manual refresh. Poller reloads when any source is `importing` and the snapshot is idle/terminal.
- [ ] **DoD-4 Recorded:** `review.md`; registry Completed; ledger FEATURE.

## 8. Verification commands

```powershell
cargo test -p dedupe-chrome --lib unaccounted
cargo test --manifest-path crates\dedupe-chrome\ui\Cargo.toml
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

## 9. Deferred roll

| Row | Disposition |
|---|---|
| D-0133-unaccounted-leaves | **Absorb** (this track) |
| D-0116-drop | Remain **0134** |
| D-0116-workflow | Remain (no auto profile / auto-extract on ingest) |
| D-0126 Unaccounted frozen | **Keep** (do not rewrite) |
| D-0024-01 NSRL RDS | Decline |
| D-0016-05 7z | Decline |
| D-0034-06 password bypass | Decline / never |
| D-0062-codesign | Decline |
| D-0067-embedded-depth | Decline |
| D-0125-dead-css / pad-fallback | Remain (0137 residual polish) |
| D-0134 … D-0137 | Other Series V tracks |
| Last-PR #149–#146 comments | **Decline** (empty / Bugbot usage-limit) |
| Fold-in opencode-M1 / AGY-133-03 | **Fold** — DoD-3 poller trigger locked |
| Fold-in opencode-m1 / AGY-133-01 | **Fold** — remaining reuses `apply_extract_start_err`; no extra queue wipe |
| Fold-in opencode-m2 | **Fold** — host emits `failed_unlogged` on the same pass |
| Fold-in AGY-133-02 | **Fold** — DTO names `unextracted_psts` + `failed_unlogged` |
| Fold-in opencode-O1 | **Fold** — execute re-verifies the two live unaccounted tests |
