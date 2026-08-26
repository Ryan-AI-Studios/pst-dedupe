# 0095 — Unique-PST Folder Tree Normalize

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.

- **Track ID:** 0095-UniquePstFolderTreeNormalize
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → Series N
- **Cross-repo contract:** n/a
- **Status:** Completed
- **Depends on:** 0070 · 0080 · **0093** (all **Completed**)
- **Spec authored:** 2026-08-25
- **Series:** N (Operator fidelity — INC0102784 post-0092)
>
> **Review fold-in (2026-08-25):** dual-AI Ready review (`opencode-review.md` + `agy-review.md`) incorporated below.
> Disposition of each claim is in §2.8. Doubled ToPF is a **layout** defect; it is **not** by itself a proven QC fail (suffix matching absorbs it). Phase 0 **classifies** the real `folder_tree_structure` interaction from existing `qc_findings.csv`.

---

## 1. Objective

Make preserve-layout unique-pst trees **counsel-useful and QC-honest**: stop nesting a second
`Root` / `Top of Personal Folders` under the writer’s IPM root, stop emitting an empty
`Unique Mail` residual in preserve mode, **close `D-0070-multi-source-stream-prefix`** by
pre-seeding known sources, and clear the INC0102784 QC **`folder_tree_structure` defect**
without abandoning preserve-layout or weakening QC to counts-only.

**Closes:** `D-0070-multi-source-stream-prefix` (pre-seed `IncrementalFolderPlan` from the known winner source list).  
**May spawn:** `D-0095-…` only if Phase 0 triage leaves a matcher/metadata mode unfixed.

---

## 2. Context (read before starting)

### 2.1 Operator evidence

QC defect (sampled volume 1, `qc_findings.csv` — operator-local, not committed):

- `property=folder_tree_structure`
- Output includes: `Root/Top of Personal Folders/Unique Mail` **and**
  `Root/Top of Personal Folders/Top of Personal Folders/<mailbox>/…`
- Message **count** matched (4055); tree/count **paths** mismatched → `hard_fail=true` → `VERIFY_FAILED`

The finding `detail` already dumps `out_folders` / `out_counts` / `expected` verbatim
(`unique_pst_qc.rs` ~795–800). **Phase 0 triages that artifact** — no operator re-run required
to classify the QC fail.

`recipient_table` was the other INC* QC defect; that is **0093** (Completed; needs operator
re-smoke). 0095 DoD is **folder_tree only**.

### 2.2 Live code snapshot (verified 2026-08-25)

| Surface | State |
|---|---|
| Preserve default | `FolderLayoutPolicy::PreservePaths { multi_source_prefix: true }` (`unique_pst_cmd.rs` ~2051; writer default too) |
| Writer display name | `"Unique Mail"` residual / flat name |
| Reader loci | `folders()` walks `NID_ROOT_FOLDER` and builds display-name paths → source `folder_path` starts `Root/Top of Personal Folders/…` |
| Writer IPM | `NID_ROOT_FOLDER` `"Root"`; IPM_SUBTREE `"Top of Personal Folders"` |
| `parse_folder_path` | Slash-split + sanitize; **no** leading alias strip (`production.rs` ~2528) |
| Preserve nest | `full_segs = [optional prefix] + parse_folder_path(source_folder_path)` (`assign_message` ~2438–2447) → output `Root/Top of Personal Folders/[prefix/]Root/Top of Personal Folders/<mailbox>/…` |
| Prefix race (D-0070) | `IncrementalFolderPlan.sources_seen` starts empty; prefixes appear only after a **second** source is seen in stream order (~2408–2416). CLI **already knows** the full winner `source_path` set (`store_key_material` ~2064–2070) |
| Residual Unique Mail | Always allocated in `IncrementalFolderPlan::start` even when empty (~2376–2384) |
| QC matcher | Suffix-segment `folder_leaf_matches` (`out == leaf \|\| out.ends_with("/"+leaf)`); per-folder count sums; exclusive longest-first claiming; unclaimed message-bearing output folders fail |
| Unique Mail QC allowance | Only when the **expected** leaf is itself Unique Mail (~2463). Residual-routed winners whose expected keys were skipped as empty do **not** get that allowance |
| `is_system_folder_path` | Filters **output** slots ending `/deleted items` (~2492). Expected keys are **not** filtered (~765–772). Message-bearing Deleted Items → latent hard-fail |
| Expected keys | `normalize_folder_key(export_row.folder_path)`; empty keys skipped |
| Default writer tests | `writer_v1.rs` ~367–380: empty preserve write asserts `Root/Top of Personal Folders/Unique Mail` as IPM child — **will move** if residual is lazy |

### 2.3 Product locks

1. Preserve-layout must remain counsel-useful (mailbox/folder provenance visible). Do **not** strip a first segment just because it is first (e.g. `Mailbox - Doe, John` is provenance).
2. **Layout sentinel strip is in scope independent of QC triage.** Doubled `Root`/`Top of Personal Folders` is an objective, counsel-visible layout bug even if suffix matching hides it from QC. See §2.4.
3. **Phase 0 must classify the actual QC failure mode** from `qc_findings.csv` before changing the matcher. Layout-vs-QC as “the” QC fix is premature until that gate. See §2.5.
4. **Close D-0070 here** by pre-seeding known sources — do it even if triage says the QC fail is not the prefix race. See §2.6.
5. Preserve residual `Unique Mail` is **lazy** (allocate on first residual message). Flat layout still always has the display-name folder. See §2.7.
6. Flat layout unchanged (`--folder-layout flat` → all mail under `Top of Personal Folders/<display-name>`).
7. Do not weaken QC to “counts only.”
8. Tree contract (DoD-2) names **sentinel list + case-fold rule + prefix policy** as one rule, not three ad-hoc behaviors.
9. 0095 operator re-smoke may also clear 0093’s `recipient_table`; do **not** credit 0095 for that.
10. Fixtures in CI; INC* evidence in `review.md` only. No production `unwrap`/`expect`.

### 2.4 Leading IPM/root alias strip (locked — layout)

Microsoft Learn [MS-PST] §2.4.4: every store has `NID_ROOT_FOLDER` whose primary child is `IPM_SUBTREE`, typically named `"Top of Personal Folders"` (Outlook also uses `"Top of Outlook Data File"` / information-store aliases).

**Strip a leading consecutive run** of well-known aliases (case-folded) from parsed source segments **before** prefix + ensure_path:

| Alias (case-folded) |
|---|
| `root` |
| `top of personal folders` |
| `top of information store` |
| `top of outlook data file` |
| `ipm_subtree` |

**Stop at the first non-alias segment.** Never strip a later segment that happens to match (user folder `Inbox/Top of Personal Folders` stays). **Never** “strip first segment always.”

Effects:

- Single-source preserve: source `Root/Top of Personal Folders/Inbox` → `["Inbox"]` → output `Root/Top of Personal Folders/Inbox`.
- Multi-source preserve: same + **stable** file-stem prefix (after §2.6) → `Root/Top of Personal Folders/<prefix>/Inbox`.

**Prefix vs inner mailbox names:** INC* may already have a mailbox folder *inside* the source tree. File-stem prefix on top can duplicate provenance. Phase 0 documents the chosen prefix policy in the DoD-2 contract (keep file-stem prefix for colliding `Inbox` across sources; do not invent a second strip of non-alias mailbox names).

### 2.5 QC failure-mode triage (locked Phase 0 gate)

`folder_leaf_matches` accepts any output path that **ends with** the expected leaf. The doubled ToPF sits *ahead of* that leaf → **pure prefix, QC-invisible**. A corpus whose only anomaly were the double root would **pass** `folder_tree_matches`.

The INC* hard_fail therefore comes from an **interaction the original spec did not name**. Classify from the existing `detail` maps:

| Mode | Mechanism | Likely fix locus |
|---|---|---|
| **(a)** Message-bearing output `Unique Mail` | Residual-routed winners; expected rows skipped as empty keys; Unique Mail allowance only fires when *expected* leaf is Unique Mail | Writer residual routing / scan locus capture |
| **(b)** Per-folder count split | D-0070 prefix race: same logical leaf across prefixed and unprefixed slots; exclusive claim mis-counts | Pre-seed sources (§2.6) |
| **(c)** Exclusive-claim starvation | Expected keys share suffixes at different lengths (`inbox` vs `root/top of personal folders/inbox`); longest-first eats the shorter key’s slots | QC matcher |
| **(d)** Metadata-incomplete | Empty expected map + any mail folder → auto-fail (`~2409–2428`) | Metadata / export rows |

**Do not change matcher claiming rules until this gate names the mode.** Independently, if this track touches QC at all (likely for (c) or Deleted Items), also fix:

- **Deleted Items asymmetry:** output slots drop `…/deleted items`; expected keys do not. A Deleted-Items winner fails `folder_tree_structure` forever. Deleted Items is a real content folder (`FolderClass::DeletedItems` is a keep-set rank, not “not mail”). Stop treating **message-bearing** Deleted Items as a system folder on the output side (or apply the same rule to expected keys — prefer keeping the mail). Exact-match arms (`top of personal folders`, `ipm_subtree`, …) are dead for output paths that always start `root/…` under this digest — opportunistic cleanup OK.

Agy’s “strip aliases in `normalize_folder_key` so QC correlates” is **not** locked as the QC fix (suffix matching already absorbs extra leading aliases). Phase 0 may add symmetric expected-key normalization **only if** triage shows mixed path shapes / claiming starvation.

Cheap diagnostic while touching the matcher: log which expected key starved (state is already in `folder_tree_matches` at failure).

### 2.6 Close D-0070 — pre-seed sources (locked)

The CLI already iterates every winner `source_path` before write. `IncrementalFolderPlan` does not get that list.

**Lock:** pass the known source list into the writer (e.g. `WritePstOpts` pre-seed) so `unique_source_prefixes` is stable from message 1. Flat policy unaffected. Collect-based `plan_folder_tree` remains for unit comparison.

This is in scope **regardless of which QC mode Phase 0 names.** If the fail is mode (b), it is the whole QC fix; otherwise it still removes an entire class of “same leaf, two paths” interactions.

### 2.7 Lazy Unique Mail in preserve (locked)

Preserve: do **not** pre-allocate `"Unique Mail"` in `start`. Allocate on first `PathParseOutcome::Residual`. Empty preserve trees have no ghost residual folder.

Flat: the display-name folder **is** the destination — still created up front (agy lock / original lock 6).

`writer_v1` empty-preserve hierarchy test currently requires Unique Mail as an IPM child — **update it** to IPM + Deleted Items (and Unique Mail only when a residual message exists). Do not “keep Unique Mail as a marker” unless Phase 0 explicitly documents that as the contract (default: suppress-when-empty).

### 2.8 Dual-AI review disposition (2026-08-25)

| # | Claim | Source | Disposition | Spec landing |
|---|---|---|---|---|
| O1 | Doubled ToPF is QC-invisible (suffix match); Phase 0 must classify (a–d) from `qc_findings.csv`; layout-vs-QC as *the* QC fix is premature | opencode | **Agree** | §2.5; Phase 0 |
| O2 | Strip only leading sentinel aliases, not first-segment-always; name sentinel list + case-fold + prefix policy as one contract | opencode | **Agree** | §2.3 lock 1–2, 8; §2.4 |
| O3 | D-0070 is cheaply closable via pre-seed; do it here and close, not “conflate later” | opencode | **Agree — close D-0070** | §2.6; DoD-3 |
| O4 | Deleted Items QC asymmetry is a latent hard-fail; fix or prove unhittable; add fixture | opencode | **Agree — fix if/when QC is touched; fixture required either way** | §2.5; DoD-1 |
| O5 | Empty preserve Unique Mail is counsel-visible noise; contract must say suppress-when-empty vs marker | opencode | **Agree — default lazy/suppress-when-empty** | §2.7 |
| O6 | DoD-4 needs `cargo test -p pst-writer`; ledger entity after triage; DoD-1 fixture matrix (dual-source collide, Deleted Items, recoverable, non-sentinel root, residual) + per-folder counts | opencode | **Agree** | §7; §8; plan Phase 0 entity |
| O7 | Scope operator re-run: folder_tree is 0095; recipient_table is 0093. Hygiene `.claude/` | opencode | **Agree** | lock 9; Phase 0 hygiene |
| A1 | `parse_folder_path` strip leading root/IPM aliases | agy | **Agree** as **consecutive leading aliases** (Root *and* ToPF), not a single first segment | §2.4 |
| A2 | Lazy Unique Mail in preserve `start` | agy | **Agree** (flat still eager) | §2.7 |
| A3 | QC `normalize_folder_key` must strip aliases so suffix matching works | agy | **Decline as required QC fix** (matcher already suffix-tolerant). May add after triage if claiming starvation needs it | §2.5 |
| A4 | Only strip the first matching segment; never later user folders named ToPF | agy | **Partial:** consecutive leading alias **run**, then stop. Never later segments | §2.4 |
| A5 | Flat layout isolation | agy | **Agree** | lock 6 |

**Declined / not locked**

- Treating doubled ToPF as the proven QC defect without Phase 0 classification (contradicted by matcher code).
- Required QC alias-strip as the primary matcher change (A3).
- “Strip first segment always.”
- Weakening QC to counts-only.
- Keeping empty Unique Mail as a default marker.
- Crediting 0095 for `recipient_table`.

---

## 3. In scope

1. Phase 0: classify INC* `folder_tree_structure` mode (a–d) from existing `qc_findings.csv` detail; lock prefix policy into the tree contract.
2. Layout: leading consecutive alias strip; lazy Unique Mail in preserve; pre-seed sources (close D-0070).
3. QC: only the matcher/expected-key changes Phase 0 names, **plus** Deleted Items asymmetry if QC is touched (fixture either way).
4. Regression fixtures per DoD-1 matrix; writer tests that currently assert Unique Mail on empty preserve.
5. Docs: `docs/unique-pst-export.md` tree contract (DoD-2).
6. Deferred: close `D-0070-multi-source-stream-prefix`; spawn `D-0095-…` only if a mode is deferred.

## 4. Out of scope

- Nested export (`0094` Completed), heap (`0093` Completed), cloud props (`0096`/`0097`).
- Outlook COM (declined).
- Changing default folder-layout policy to flat.
- Matter / Relativity folder mapping.
- Cycle detection, cloud hydrate.

## 5. Preconditions & dependencies

- **P1:** 0080 folder-tree compare exists.
- **P2:** 0093 Completed — operator re-smoke may still show `recipient_table` until that corpus is re-run; not 0095 scope.
- *Verified:* INC0102784 QC hard_fail included folder_tree + recipient_table. Message count already matched — path-shape / claiming, not data loss.

## 6. Risks

| Risk | Mitigation |
|---|---|
| “Fix” breaks operators who scripted Unique Mail / doubled ToPF paths | Document migration in unique-pst-export; update writer goldens |
| Prefix race conflated with double-root | Pre-seed anyway (close D-0070); triage QC from CSV |
| Strip eats real mailbox display names | Sentinel allowlist only; stop at first non-alias |
| QC matcher tweak collapses distinct leaves | No claiming-rule change until Phase 0 names mode (c) |
| Deleted Items silently hard-fails after layout looks “fixed” | Fixture + asymmetry fix |
| Empty-preserve tests still require Unique Mail | Update `writer_v1` hierarchy test |

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 —** Preserve fixtures (not just path strings — **matcher verdict + per-folder counts**):
  1. Dual-source, shared subfolder names (claim-collision / prefix).
  2. Deleted-Items winner.
  3. Recoverable-items (or similarly non-Inbox) winner.
  4. Source whose first **non-sentinel** segment is a real mailbox name (provenance preserved).
  5. Empty / unparseable `folder_path` → residual Unique Mail **only then**; no empty Unique Mail on a fully-preserved tree.
  Output has a **single** IPM `Top of Personal Folders` (no nested duplicate alias). QC `folder_tree_structure` is not a `defect` (match, or intentional explained/known_gap **only** if Phase 0 product-locks that class).
- [ ] **DoD-2 —** Tree contract in `docs/unique-pst-export.md`: sentinel list, case-fold, prefix policy, Unique Mail lazy-in-preserve, flat isolation.
- [ ] **DoD-3 —** `D-0070-multi-source-stream-prefix` **closed**. `D-0095-…` only if a classified mode is deferred.
- [ ] **DoD-4 —** `cargo test -p pst-writer` **and** `unique_pst_qc_0080` **and** targeted `unique_pst` tests green; clippy `-D warnings` on touched crates. `cargo fmt --all --check`.
- [ ] **DoD-5 — Recorded:** `review.md`; conductor **Completed**; ledger commit (`BUGFIX`). Operator re-smoke: folder_tree not defect (recipient_table is 0093).

## 8. Verification commands (reference)

```powershell
cargo fmt --all --check
cargo clippy -p pst-writer -p pst-dedup-cli --all-targets -- -D warnings
cargo test -p pst-writer
cargo test -p pst-dedup-cli --test unique_pst_qc_0080
cargo test -p pst-dedup-cli --test unique_pst
# operator: unique-pst INC0102784; expect folder_tree_structure not defect
# (recipient_table is 0093 re-smoke, not this track's DoD)
```
