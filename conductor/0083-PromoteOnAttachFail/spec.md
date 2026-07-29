# 0083 — Promote on Attach Fail (Mode A)

> Structure follows `C:\dev\coordinated\conductor\templates\0000-Description\spec.md`.
> Expanded subsections under §2–§3 are normative design for implementers. DoD is §7.

- **Track ID:** 0083-PromoteOnAttachFail
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → Series M (unique-export fidelity residuals) after 0082
- **Cross-repo contract:** n/a
- **Status:** Completed
- **Depends on:** 0066 · 0071 · 0073 · 0074 · 0075 · 0078 · 0081 · 0082 (all **Completed** on board)
- **Spec authored:** 2026-07-29
- **Revised 2026-07-29:** dual-AI review fold-in — Sedona **cross-custodian de-duplication** disclosure; `dup_sources` post-promote invariant; Mode A × 0080 QC test; identity-tier group fracture honesty; Mode C fallback `decided_by`; cloud-attach predicate ceiling (not invent detection)
- **Series:** M (Unique export fidelity residuals)

---

## 1. Objective

Ship **Mode A pre-write promote-on-attach-fail** for unique export: when a keep-set winner materializes with **incomplete attachments** and a ranked peer exists, **promote the next peer before any PST/EML write commits that family** — so the deliverable prefers a complete alternate copy (often another custodian under default global grouping) over a half-attached winner — while keeping **Mode C ledger-only** as the default, **forbidding Mode B write-time promote**, recording promotions and all-peers-incomplete fallbacks with distinct `decided_by` strings, and documenting the **cross-custodian de-duplication** disclosure obligation for counsel.

## 2. Context (read before starting)

### 2.1 Why this track exists now

0073 shipped the attach failure **ledger** (Mode C) but explicitly residualized promote:

| Deferred | Severity | Claim |
|---|---|---|
| **D-0073-promote** | **P1** | Mode A pre-write promote not shipped; default Mode C ledger-only |
| **D-0073-eml** | P2 | unique-eml attach ledger CSV parity still open (narrowed by 0078 counters) |
| `winner_promoted` column | — | Documented as **always false** in P0 pending Mode A |

0082 closed recipient-table fidelity. The next highest Series M residual named on the board is **D-0073-promote**. Operator reality (INC-class multi-mailbox): first-seen / policy winner may be the copy whose attaches CRC-fail or stream-fail while a peer mailbox holds a complete copy.

### 2.2 Industry / product anchors (researched 2026-07-29)

**eDiscovery / EDRM / Sedona practice (robust defaults; researched 2026-07-29):**

1. **Prefer a complete alternate source copy** over producing a unique item that is known incomplete when the same logical message exists elsewhere in the collection (custodian / path peers).
2. **Exception inventory remains mandatory** — promotion does not erase the failed locus; decisions and attach ledgers must show *why* the export winner moved.
3. **Do not silently rewrite mid-stream** — industry tooling and our own writer model treat a partially written message object as a corruption risk; selection happens **before** commit of that family to the output store.
4. **Determinism** — peer order follows an explicit policy ranking (already `rank_key` / keep-set ladder from 0066/0075), not filesystem race or wall clock.
5. **Opt-in automation** — changing which bytes land in the unique set is a production decision; default off preserves today's reproducible Mode C behavior.
6. **Cross-custodian de-duplication is a term of art** — The Sedona Conference Glossary (6th ed., forthcoming 2026) defines *Cross-Custodian De-Duplication* as suppression/removal of exact copies across multiple custodians for review/production minimization. Default keep-set grouping **unites across custodians** (`keepset.rs` ~1178); `--dedupe-scope per-source` isolates. Mode A peer walks **that same group**, so under default global scope it **is** cross-custodian de-duplication when peers live in different source PSTs. Negotiated ESI protocols often require **disclosing** when cross-custodian dedup is in play and **which custodians held a suppressed copy**. Transparency already exists as `duplicate_sources` / `duplicate_source_count` on Unique decision rows — Mode A must **preserve** that inventory for the full group (including skipped incomplete peers), not shrink it to the final winner only.
7. **Family identity vs attachment bytes** — Mode A only sees peers inside one keep-set group. Live identity levels (`off` / `body` / `body-recip`) do **not** hash attach payloads (CLI rejects `body-recip-attach` → **D-0076-attach-content**). If attach-byte identity ever ships and splits incomplete vs complete into different groups, Mode A cannot recover across that split — document the interaction; do not invent a second cross-group walk.

**Prior art already in this repo:**

- Hard **materialize** failure already walks ranked peers and sets `promoted_from_failure` + `decided_by=promoted_after_materialize_fail` (`finalize_with_materialize` in `keepset.rs`).
- Soft attach fails during **write** still write the message (best-effort) and ledger rows (0073 Mode C) — **no** mid-write promote.
- **0074** deep attach preflight can mark `stream_available` / probe failures **before** export write when opted in.
- Decision rows carry `duplicate_sources` / `duplicate_source_count` for Unique winners (suppressed peer inventory).
- 0080 source-differential QC keys off the **export winner** `source_id` / locus from the report — Mode A must leave QC comparing against the **final** promoted source, not a pre-promote pick.

Mode A is the soft-attach analogue of the existing hard-materialize promote loop — not a second policy engine.

### 2.3 Live code snapshot (verified 2026-07-29)

| Surface | State |
|---|---|
| `finalize_with_materialize` | On `MaterializeError::Hard`, try next ranked peer; soft `Ok(msg)` always accepted even if attaches incomplete |
| `promoted_from_failure` / `promoted_after_materialize_fail` | Shipped for **hard** materialize only |
| `--promote-on-attach-fail` | **Not present** (docs point at residual) |
| `export_attachments.csv` / histogram | Mode C inventory shipped (0073) |
| `winner_promoted` on attach ledger | Always `false` (documented) |
| `--deep-attach-preflight` | Opt-in probe path (0074); feeds incomplete signals when used |
| `stream_available` on attach DTOs | Exists; exporters soft-skip when false |
| Exit 64 PartialFidelity | Still fires when `attachments_failed > 0` after write (0078) |
| unique-eml | Soft-skip missing attaches; no full ledger CSV (**D-0073-eml**) |

### 2.4 Dependency currency (re-queried crates.io 2026-07-29)

No dependency work is required for this track. Pins match post-0081/0082 lock unless a High/Critical advisory appears (0081 security override rule).

| Dep | Lock | crates.io max | 0083 decision |
|---|---|---|---|
| clap | 4.6.4 | 4.6.4 | KEEP |
| serde_json | 1.0.151 | 1.0.151 | KEEP |
| thiserror | 2.0.19 (+1.x dual) | 2.0.19 | KEEP |
| camino | 1.2.5 | 1.2.5 | KEEP |
| uuid | 1.24.0 | 1.24.0 | KEEP |
| rusqlite | 0.40.1 | 0.40.1 | KEEP |
| sha2 | 0.11.0 (+0.10 dual) | 0.11.0 | KEEP |
| md-5 | 0.10.6 product | 0.11.0 | KEEP product |
| eframe | 0.34.2 | 0.35.0 | DECLINE_MAJOR |
| reqwest | 0.12.28 (+0.13 residual) | 0.13.4 | DECLINE_MAJOR |
| aes-gcm | 0.10.3 | 0.11.0 | DECLINE_MAJOR |
| argon2 | 0.5.3 | 0.6.0-rc.8 | KEEP stable |
| rand | multi past RUSTSEC-2026-0097 floors | 0.10.2 | KEEP |
| tantivy | 0.26.1 | 0.26.1 | KEEP |

Re-query at implement if >7 days after this date.

### 2.5 Locked product rules

1. **Sources remain read-only.** Never mutate source PSTs.
2. **Mode A only when flag on.** CLI: `--promote-on-attach-fail` (default **off**). When off → today's Mode C (write best-effort, ledger fails, exit honesty unchanged).
3. **Mode B (write-time mid-message promote) is out** — permanently for this track and residualed as declined, not "later free win." Soft-fail mid-stream keeps message atomicity as shipped in writer tests; do not abort-and-rewrite.
4. **Pre-write only:** incomplete detection runs on the **materialized** `CanonicalMessage` (and/or 0074 probe cache already applied into attach DTOs) **before** the unique-pst / unique-eml writer commits that winner family.
5. **Incomplete definition (normative):** a materialized message is **attach-incomplete** when **any** of:
   - one or more attachments have `stream_available == false`; or
   - materialize/preflight already attached fail-severity attach reasons / fidelity flags that would produce fail-severity ledger rows if written as-is; or
   - deep-preflight results already bound onto the winner mark attach stream not exportable  
   **Not** incomplete solely for: body soft flags, CRC page noise, zero-byte by-value success, or `parents_only` policy omit (omit ≠ fail — 0073).  
   **Honesty ceiling — cloud / modern attachments:** `pst-reader` still has **no** named-property resolution (**D-0080-cloud-attachments**). Link-only OneDrive/SharePoint attaches that present as “parsed” with no fail-severity stream error **cannot** be detected as incomplete in this track. Do **not** invent a modern-attach detector here; document that Mode A will not promote away from an undetected cloud-link-only “complete” copy. When named-prop detection lands later, extend the predicate there — not by smuggling incomplete logic into 0083 without the reader capability.
6. **Peer walk:** same ranked order as hard materialize promote (`rank_key` / `rank_ctx` from keep-set; respects 0075 policies). Deterministic. **No second ladder** — including **no** re-rank by “fewest attach fails.” Mode C fallback always takes the **highest-ranked materializable** peer even if a lower-ranked peer is less incomplete (intentional; not an oversight).
7. **Accept rules when flag on:**
   - Prefer first ranked peer that materializes **and** is **not** attach-incomplete.
   - On hard materialize fail → skip peer (existing behavior).
   - If **all** peers that materialize are attach-incomplete → **Mode C fallback**: export the **highest-ranked peer that materialized** (first successful materialize in rank order), ledger attach fails, `ok`/exit honesty as today. Do **not** drop the group solely for soft attach incompleteness.
   - If no peer materializes → existing all-failed path (`group_dropped` / materialize_failed).
8. **Honesty / decided_by vocabulary (fixed strings):**
   - `promoted_from_failure == true` when the accepted winner was not the first ranked peer attempted (hard **or** soft-attach promote).
   - **`promoted_after_attach_incomplete`** — Mode A successfully selected a later complete peer after skipping incomplete earlier peer(s).
   - **`promoted_after_materialize_fail`** — existing hard-fail promote string (unchanged).
   - **`mode_c_fallback_all_peers_incomplete`** — flag **on**, every materializable peer was attach-incomplete, exported highest-ranked materializable with attach fails still ledgered. Distinct so operators can filter spreadsheet rows where Mode A **failed to recover** a complete copy.
   - Attach ledger: `winner_promoted` true for fail rows on **skipped** incomplete winners when a later peer was accepted; `peer_source_id` / `peer_msg_nid` when known.
   - Skipped incomplete candidates remain visible (decision role or ledger rows) — do not invent success for them.
   - **`duplicate_sources` / `duplicate_source_count` on the Unique (export winner) row MUST still enumerate the group’s other sources** after Mode A (including skipped incomplete peers and the original higher-ranked incomplete locus). This is the field ESI protocol review reaches for suppressed-custodian inventory. Unit/integration assert: promote does not shrink `duplicate_sources` to empty or winner-only when group size > 1.
9. **Deep preflight is optional enrichment, not a hard dependency.** Mode A **must** work with materialize-level `stream_available` / soft flags alone. When `--deep-attach-preflight` ran, use its results (already on DTOs) — do not require a second probe pass inside promote unless cache miss and budget allows (prefer no extra I/O in v1).
10. **No new exit integers.** Exit 64 still means artifact exists with partial fidelity; promoting to a complete peer may **clear** attach fails for that family (exit 0 path when no other partials).
11. **No new `export_risk` values.**
12. **Default inert optional features.** Flag off does not change keep-set winners vs today.
13. **No `unwrap`/`expect` in production** — `miette` + `Result`.
14. **Synthetic fixtures only in git.**
15. **unique-eml:** if it shares `finalize_with_materialize` (or equivalent), Mode A applies when the same flag is threaded; full attach-ledger CSV for eml remains residual unless a **minimal** shared reason emission is free — do not block Mode A on full D-0073-eml.
16. **Cross-custodian disclosure (docs + help):** runbook and unique-pst-export MUST name Mode A under default global scope as **cross-custodian de-duplication** (Sedona term), state that suppressed peers remain in `duplicate_sources`, and note that `--dedupe-scope per-source` confines peers (and thus Mode A) to a single source — no cross-custodian promote when isolation is on.
17. **Identity-tier precondition:** Mode A operates only **within** a keep-set group. Document that live levels `off|body|body-recip` keep incomplete/complete peers groupable; residual **D-0076-attach-content** (`body-recip-attach`) would hash attach payloads and **can fracture** incomplete vs complete into separate groups — Mode A will not see them as peers. If that level is ever enabled later, product must document incompatibility or add a warning; **0083 does not enable attach-content hashing**.

### 2.6 Deferred roll-in matrix

| ID | Disposition in 0083 | Why |
|---|---|---|
| **D-0073-promote** | **Ship / close** | Core deliverable |
| **winner_promoted / peer_* columns** | **Ship** (wire honesty) | Were placeholders waiting on Mode A |
| **D-0073-eml** | **Partial / optional** | Mode A flag on shared materialize path helps eml completeness; full ledger CSV parity **stays residual** unless implementer finds ≤1 day shared sink reuse — document either close-narrow or residual |
| **D-0074-timeout-join** | **Decline** | Probe worker join residual; not promote-shaped |
| **D-0074-gui** / **D-0073-gui** | **Decline** | CLI flag + UniquePstCliArgs pass-through only |
| **D-0076-attach-content** | **Decline** | Identity hash attach bytes — different product surface |
| **D-0080-cloud-attachments** | **Decline** | Named-prop resolution track |
| **D-0079-deterministic-key** | **Decline** | Product record-key change |
| **D-0079-stream-prepare** / `--jobs` | **Decline** | Perf Phase C |
| Mode B write-time promote | **Decline permanently** (this track) | Atomicity / half-message risk |

### 2.7 Design sketch (normative)

#### 2.7.1 Incomplete predicate

```text
fn is_attach_incomplete(msg: &CanonicalMessage) -> bool {
    // any attach with !stream_available
    // OR any fail-severity attach outcome already known pre-write
    // NOT parents_only omit, NOT body-only soft flags
}
```

Centralize in `dedup-engine` (or shared export helper) so unique-pst and tests share one definition. Unit-test the predicate exhaustively.

#### 2.7.2 Promote loop (extend hard path)

Current hard path (simplified):

```text
for peer in ranked:
  match materialize(peer):
    Ok(msg) -> accept (always)
    Hard -> continue
fallback: all failed
```

Mode A when `promote_on_attach_fail`:

```text
for peer in ranked:
  match materialize(peer):
    Hard -> mark failed; continue
    Ok(msg) if is_attach_incomplete(msg) && more_peers_remain:
      record skipped_incomplete(peer); continue
    Ok(msg) if is_attach_incomplete(msg) && !more_peers_remain:
      accept Mode C fallback; decided_by = mode_c_fallback_all_peers_incomplete
      (only when flag on and ≥1 incomplete skip occurred; if flag on but sole peer incomplete, same string or document single-peer incomplete as Mode C with attach fails — prefer the fixed string whenever flag on and accepted winner is still incomplete)
    Ok(msg) complete -> accept; if attempt > 0: decided_by = promoted_after_attach_incomplete
```

- `attempt > 0` on accept → `promoted_from_failure = true` (including Mode C fallback after skips).
- Soft skip does **not** set `DecisionRole::MaterializeFailed` (that role is hard-fail only). Prefer additive notes + fixed `decided_by` strings rather than a breaking role enum without migration note.
- After accept: rebuild/preserve Unique-row **`duplicate_sources`** from the full group membership (all non-winner peers), not from “accepted peer only.”

#### 2.7.3 CLI / report

```text
--promote-on-attach-fail
```

- Default: **off**.
- Surface on `unique-pst` (required). Thread through shared `UniquePstCliArgs` for GUI pass-through (checkbox residual).
- If unique-eml has a parallel flags struct and shared finalizer, thread the same flag name.
- Summary JSON (additive): `promote_on_attach_fail: bool`, `promoted_after_attach_incomplete_count` (u64), `mode_c_fallback_all_peers_incomplete_count` (u64).
- decisions.csv: existing promote columns + honest `decided_by` / notes (three-way vocabulary in rule 8).
- export_attachments.csv: `winner_promoted`, peer locus columns when applicable.
- Help text one-liner: Mode A may perform **cross-custodian de-duplication** under default global scope; see runbook.

#### 2.7.4 Interaction matrix

| Flag combo | Behavior |
|---|---|
| promote off, deep off | Mode C today |
| promote off, deep on | Better incomplete *detection* / preflight risk; still Mode C at write |
| promote on, deep off | Mode A using materialize `stream_available` / soft flags only |
| promote on, deep on | Mode A with richer pre-marked incomplete attaches (preferred operator path) |
| promote on + `--dedupe-scope per-source` | Mode A only among peers **within** one source — no cross-custodian promote |
| promote on + global scope (default) | Mode A may select another custodian’s complete copy → **cross-custodian de-duplication** |
| future `body-recip-attach` (not live) | May split incomplete/complete into different groups → Mode A ineffective across that split |

#### 2.7.5 Non-goals (honesty)

- Mode A does **not** repair corrupt attaches.
- Mode A does **not** expand DLs or resolve cloud attach links (0082/0080); does **not** detect modern/cloud attaches as incomplete without named-prop resolution.
- Mode A does **not** change keep-set **grouping** (who is a peer) — only **which peer wins export** after grouping.
- Mode A does **not** walk peers outside the keep-set group (no cross-group “find any complete copy by Message-ID”).
- Mode A does **not** re-rank by least-incomplete attach count (rule 6).

### 2.8 Affected crates / docs

| Path | Change |
|---|---|
| `crates/dedup-engine` | Incomplete predicate; Mode A loop in materialize finalizer; stats; decided_by strings; tests |
| `crates/pst-dedup-cli` | `--promote-on-attach-fail`; wire into unique-pst (and eml if shared); summary fields |
| `crates/pst-writer` | Only if ledger event needs promote context already available — prefer no writer change |
| `crates/pst-dedup-gui` | Pass-through default off via UniquePstCliArgs — no required new UI |
| `docs/unique-pst-export.md` | Flag, modes A/B/C; Sedona cross-custodian naming; identity-tier interaction; residual close |
| `docs/unique-pst-ediscovery-runbook.md` | When to enable; deep preflight; **cross-custodian disclosure**; `duplicate_sources`; Mode C fallback filter; cloud-attach ceiling |
| `docs/deferred.md` | Close D-0073-promote; note eml / cloud / attach-content interactions |
| CHANGELOG `[Unreleased]` | Tier-1 entry |

### 2.9 Product decisions locked (do not re-litigate at implement)

| # | Decision | Default |
|---|---|---|
| Q1 | Ship Mode A pre-write promote | **Yes** (flagged) |
| Q2 | Default flag | **off** |
| Q3 | Mode B write-time promote | **Never** (this track) |
| Q4 | All peers incomplete | **Mode C fallback** on highest-ranked materializable peer |
| Q5 | Incomplete definition | §2.5 rule 5 (attach only; **no** invent cloud detector) |
| Q6 | Peer order | Existing keep-set `rank_key` only — **no** least-incomplete re-rank |
| Q7 | Require deep preflight | **No** |
| Q8 | Full unique-eml ledger CSV | **Not required** for track complete |
| Q9 | New exit codes | **No** |
| Q10 | Cross-custodian under global scope | **Yes** (name Sedona term; disclose via runbook + `duplicate_sources`) |
| Q11 | All-peers-incomplete `decided_by` | **`mode_c_fallback_all_peers_incomplete`** |
| Q12 | Mode A × 0080 QC | Final winner locus only; integration test required |
| Q13 | Detect cloud/modern attach as incomplete | **Out** until D-0080-cloud-attachments / named props |

---

## 3. In scope

1. **`--promote-on-attach-fail`** CLI (default off) on unique-pst; shared args wiring.
2. **Attach-incomplete predicate** + unit tests (rule 5 + cloud honesty ceiling documented).
3. **Mode A peer walk** integrated with existing hard-fail promote in materialize finalization.
4. **Honesty:** three-way `decided_by` vocabulary; `promoted_from_failure`; summary counters incl. Mode C fallback count; attach ledger `winner_promoted` / peer locus; **`duplicate_sources` full-group invariant** after promote.
5. **Mode C fallback** when all materializable peers are incomplete (highest-ranked, not least-incomplete).
6. **Docs + deferred close** for D-0073-promote; runbook **cross-custodian de-duplication** disclosure; identity-tier fracture note; cloud ceiling.
7. **Tests:** promote to complete peer; flag off; all incomplete fallback + decided_by string; hard-fail promote; `duplicate_sources` membership; **Mode A + QC sample clean (no spurious unexplained_loss)**; exit/summary honesty.
8. **Verification gate** (fmt / clippy / test / deny).

## 4. Out of scope (do NOT do here)

- Mode B write-time promote / message rewrite after partial attach write.
- Full **D-0073-eml** attach ledger CSV (unless trivial shared sink — still not DoD-blocking).
- **D-0076-attach-content** body-recip-attach identity level (document interaction only).
- Named properties / cloud-attachment **detection** (**D-0080-cloud-attachments**) — honesty ceiling only.
- Cross-group Message-ID search outside keep-set.
- Least-incomplete secondary ranking.
- Deterministic store record key (**D-0079-deterministic-key**).
- Stream-prepare / `--jobs` / multi-GB soak.
- Changing CRC thresholds, export_risk vocabulary, or exit integer set.
- Desk UI for the flag (CLI + inert GUI pass-through only).
- Keep-set ranking redesign (0075) or new winner policies.
- ScanPST / Outlook COM.

## 5. Preconditions & dependencies

- **P1 (blocking):** 0073 Mode C ledger + reason taxonomy **Completed**.
- **P2 (blocking):** 0066/0075 keep-set rank + hard materialize promote path exists.
- **P3 (blocking):** 0078 exit/fidelity contract (exit 64 semantics) stable.
- **P4 (soft):** 0074 deep preflight for richer incomplete signals; Mode A must not require it.
- **P5 (soft):** 0082 completed (board hygiene; no hard code dependency).
- *Verified to date (2026-07-29):*
  - Hard promote loop + `promoted_from_failure` live in `finalize_with_materialize`.
  - No `--promote-on-attach-fail` flag.
  - docs claim Mode A residual **D-0073-promote**.
  - crates.io pins match §2.4.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Accepting incomplete winner when a complete peer exists (flag on) | Mode A walk; tests with two-peer fixture (incomplete first, complete second) |
| Dropping group when all incomplete | Mode C fallback rule 7; `mode_c_fallback_all_peers_incomplete`; test |
| Counsel misses cross-custodian implication | Sedona term + disclosure in runbook; `duplicate_sources` invariant tests |
| QC compares pre-promote source → false unexplained_loss | DoD-15 integration: promote + qc sample clean |
| Attach-byte identity fractures peers (future) | Document D-0076-attach-content interaction; do not enable attach-content here |
| Cloud-link copy treated complete | Honesty ceiling in rule 5; residual D-0080-cloud-attachments |
| Non-determinism / different winners across runs | rank_key only; no least-incomplete ladder; flag default off |
| Double-counting promote vs hard-fail vs fallback | Three distinct `decided_by` strings; unit tests all paths |
| Extra PST opens / perf | Reuse materialize path; no mandatory deep re-probe |
| Writer half-message if Mode B sneaks in | Spec forbids; code review checklist |
| CSV consumers break on new decided_by | Additive strings; document; columns append-only |
| Flag on changes INC keep-set winners vs historical Mode C runs | Document in CHANGELOG + runbook; default off preserves old path |

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Flag:** `--promote-on-attach-fail` exists on unique-pst; default **off**; help text states Mode A pre-write only and points at cross-custodian disclosure in runbook.
- [ ] **DoD-2 — Predicate:** `is_attach_incomplete` (or named equivalent) implemented and unit-tested per §2.5 rule 5 (cloud ceiling documented in code comment + docs).
- [ ] **DoD-3 — Mode A promote:** With flag on, incomplete first peer + complete second peer → second is export winner; `promoted_from_failure` true; `decided_by=promoted_after_attach_incomplete`.
- [ ] **DoD-4 — Mode C default:** Flag off → same winner as today for incomplete attaches (no promote); ledger still records fails.
- [ ] **DoD-5 — All incomplete fallback:** All peers incomplete → highest-ranked materializable peer exported (not group drop); **`decided_by=mode_c_fallback_all_peers_incomplete`** when flag on; attach fails still counted; summary counter increments.
- [ ] **DoD-6 — Hard promote preserved:** Existing hard materialize fail promote tests remain green (`promoted_after_materialize_fail`).
- [ ] **DoD-7 — Ledger honesty:** `winner_promoted` / peer locus / summary promote counters accurate on Mode A path (tests).
- [ ] **DoD-8 — Exit honesty:** Complete peer after promote can yield zero attach fails for that family; incomplete fallback still partial-fidelity honest (64 when fail-on-partial).
- [ ] **DoD-9 — Mode B absent:** No write-time promote path; residual documented as declined.
- [ ] **DoD-10 — dup_sources invariant:** After Mode A promote in a multi-source group, Unique-row `duplicate_sources` / count still reflects **other group members** (including skipped incomplete), not empty/winner-only.
- [ ] **DoD-11 — Mode A × QC:** Integration test: `--promote-on-attach-fail` with QC sample (or equivalent in-process QC) on a fixture where promotion occurs → **no** spurious `unexplained_loss` / hard defect from comparing against the pre-promote source; QC keys final winner locus.
- [ ] **DoD-12 — Docs:** unique-pst-export + eDiscovery runbook (Sedona **cross-custodian de-duplication**, `duplicate_sources`, Mode C fallback filter, identity-tier fracture, cloud ceiling) + deferred.md (D-0073-promote closed); CHANGELOG `[Unreleased]`.
- [ ] **DoD-13 — Deps:** No unapproved majors (default: none).
- [ ] **DoD-14 — Tests gate:** `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace`; `cargo deny check` green.
- [ ] **DoD-15 — Recorded:** `review.md` with evidence + deferred disposition + dual-AI fold-in summary; `../conductor.md` **Completed**; ledger commit (`FEATURE`).

## 8. Verification commands (reference)

```powershell
# Targeted
cargo test -p dedup-engine -- promote
cargo test -p dedup-engine -- materialize
cargo test -p pst-dedup-cli -- promote
cargo test -p pst-dedup-cli -- attach

# Full gate
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
ledgerful verify

# Optional operator (local PSTs only)
# unique-pst --promote-on-attach-fail --deep-attach-preflight ...
```
