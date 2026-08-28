# 0105 — Body-Cloud Window-Edge Normalize

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do not re-open 0097 C+A hybrid policy, caps,
> BCC default, HNBITMAPHDR, or frontend during implementation.

- **Track ID:** 0105-BodyCloudWindowEdgeNormalize
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `docs/unique-pst-export.md` + this track. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-08-28); do **not** chase it at execute.
- **Cross-repo contract:** n/a
- **Status:** Completed
- **Depends on:** 0097 (Completed — C+A hybrid truncation honesty). Series P **0099–0104** Completed are not code dependencies.
- **Spec authored:** 2026-08-28
- **Series:** Q (Unique-export honesty residuals, post-0104)
>
> **Closes:** `D-0097-window-edge-normalize`.
> **HITL:** none. Unit tests in `body_cloud_links.rs` are sufficient. No INC* smoke.
>
> **Last-PR fold-in (2026-08-28):** PRs **#99, #98, #97, #96**. Disposition in §2.8. No Cursor/Bugbot comments in that window. Origin Bugbot is **#88** (0097) — this track.
>
> **Review fold-in (2026-08-28):** `opencode-review.md` + `agy-review.md`. Disposition in §2.9 and `foldin-note.md`. Lock `note_overlength` as the `seen` insert site; evaluate max-links **before** that insert; punctuated fixture is `head+tail == url + "."`.
>
> This ID was unused. It is **not** stolen for Hermes Series O (frontend stays **0106+**).

---

## 1. Objective

Make body-cloud **window-edge** dedupe use the same `normalize_candidate` identity as kept hits, so a duplicate SharePoint/OneDrive URL cut at the 100k scan boundary (trailing sentence punctuation, HTML unescape) does **not** emit a false `BODY_CLOUD_LINK_WINDOW` marker. Also put classified **over-length** URLs into `seen` so a later edge duplicate of an already-noted over-length URL does not add WINDOW.

Today `handle_window_edge_bare` classifies the raw `full_bare_url_from` match and checks `acc.seen` without `normalize_candidate`. Kept hits always store the normalized string. 0097 already tests the exact-string duplicate cut (`body_window_duplicate_cut_url_not_dropped`); the punctuated / unescape / over-length cases still lie.

This advances unique-export **defensibility**: `export_body_cloud_links.csv` honesty markers must mean a document-shaped candidate was actually dropped, not that the window cut a period off a URL we already kept.

---

## 2. Context (read before starting)

### 2.1 Diagnosis (`D-0097-window-edge-normalize`, still live)

**Origin:** PR **#88** (0097) Cursor Bugbot, medium. Parked during Series P as “not Series P.” Series P **0099–0104** is now Completed. This ID absorbs the parked comment.

Bugbot gist (verbatim class): `handle_window_edge_bare` classifies the raw bare match and checks `acc.seen` without `normalize_candidate` (trailing punctuation / HTML unescape). Kept hits are always stored normalized, so a duplicate cut at the 100k boundary with a trailing sentence period misses `seen` and falsely sets `window_dropped`. Over-length drops are never inserted into `seen`, so a later edge duplicate of an already-noted over-length URL incorrectly adds `BODY_CLOUD_LINK_WINDOW`.

Live confirmation 2026-08-28, `main` @ `7d3778c` (re-verify line numbers at execute):

| Surface | State |
|---|---|
| Edge handler | `body_cloud_links.rs` `handle_window_edge_bare` (~249): `full_bare_url_from` → `classify_url(full)` → `acc.seen.contains(&final_url)`. **No** `normalize_candidate`. Bare `.` **is** a URL-continue char (`is_url_continue_char`), so the full original match includes a trailing sentence period. |
| Kept path | `try_keep_candidate` (~370): `normalize_candidate(raw, strip_trailing_punct)` **then** `classify_url`. Bare callers pass `strip_trailing_punct=true`. `seen` stores `final_url`. |
| Over-length | `try_keep_candidate` overlength branch (~389): `note_overlength` then `return` — **does not** `seen.insert`. `note_overlength` (~104) only sets flags + prefix. |
| Existing exact-dup test | `body_window_duplicate_cut_url_not_dropped` (~1066): cut `book.xls`/`x?d=1` with **no** trailing period — green, does **not** cover Bugbot. `max_links_duplicate_cut_url_not_truncated` (~1118) same. |
| Unique cut still WINDOW | `body_window_bare_url_cut_at_boundary_not_kept` (~997) must stay green. |
| Caps / policy | 100k / 2048 / 50; `truncated` := dropped document-shaped candidates; ≤1 marker/message. Unchanged. |
| CSV mapping | `unique_export_report` already emits `BODY_CLOUD_LINK_WINDOW` from `scan.window_dropped`. No CLI schema change. |

### 2.2 Why the exact-dup test is not proof

`:x:` action-token URLs stay document-shaped with a trailing `.` (`is_document_shaped_cloud` keys off `:x:`, not the `xlsx` suffix). `classify_url("https://…/book.xlsx?d=1.")` therefore returns `Some((url+".", …))`, which is **not** the kept `url` in `seen`. `handle_window_edge_bare` notes `window_dropped`. Counsel sees WINDOW for a URL already in the hit list.

### 2.3 MS-PST / crate APIs (plan-time)

**N/A this track** for MS-PST structures (Learn index still [MS-PST] TOC, published 2026-08-05; PDF rev v11.2 / 2025-02-18). This is the in-crate body-cloud scanner (`dedup-engine`), not writer/reader layout.

Crate-registry API churn: none expected. No new deps. Schema / matter-core version: N/A (scanner-only).

Purview modern-attachment host/path allowlist is **0085/0088** — do not extend hosts here (`D-0088-usgovcloud-microsoft-tld` stays residual).

### 2.4 Tools (plan-time)

Ran from `C:\dev\Dedupe`:

- `ai-brains preflight --summary` (inited; 3857 pinned).
- `ai-brains sync query` / `recall` — Series P 0099–0104 Completed; frontend Series O **if started** uses 0105+; this pass uses **0105** for the parked #88 Bugbot (unique-export honesty), so frontend moves to **0106+**.
- `ledgerful doctor --json` `readyForPublish: true`; `ledger status --compact` 0 pending / 0 unaudited drift (before this planning tx). `scan --impact` **LOW** (HEAD `7d3778c`; dirty tree is skills + `agy-review.md` + `fixtures/keep_set_summary.json`, not product crates). Hotspot `export_exit_0078.rs` is out of scope.
- Ledger tx for this planning pass: `9ac2db75-0a60-496f-b6b9-a03ebbbbc236`.
- `C:\dev\Dedupe-plan.md` absent.

### 2.5 ai-brains decisions absorbed

| Memory | Use here |
|---|---|
| 0097 C+A hybrid; caps 100k/2048/50; umbrella `BODY_CLOUD_LINK_TRUNCATED` gone | Do not reopen. WINDOW still means a dropped document-shaped candidate. |
| Series P 0099–0104 Completed; frontend 0105+ | **0105 is this unique-export honesty residual**, not Tauri. Frontend **0106+**. |
| 0082 BCC opt-in | Unchanged. |
| 0104 HNBITMAPHDR fail-closed | Out of scope. |

### 2.6 How this advances the north star

Counsel-facing unique-PST report packs must be honest. A WINDOW marker on a message whose only “lost” URL is the same `:x:` link already kept (plus a sentence period at the 100k cut) looks like a silent drop. 0097 closed empty-URL truncation; this closes the remaining edge-dedupe lie.

### 2.7 Why not frontend / HNBITMAPHDR / attach-crc

- Hermes Series O (Tauri/Leptos) was reserved at 0105+ **if started**. Unique-pst soak (`D-0094-inc-resmoke`) is still operator HITL, not this track. North star is unique-export honesty, not UI polish. Frontend IDs start at **0106**.
- `D-0100-hn-bitmap-hdr`: do not implement until a corpus hits the fail-closed error.
- `D-0099-attach-crc-job-level`: 0099 already declined per-event writer attribution. Do not reopen.

### 2.8 Last-PR Cursor comments (merged #99, #98, #97, #96)

Skill: last 2–4 merged product PRs.

| PR | Comment | Verdict |
|---|---|---|
| **#99** (0104 docs merge record) | No review / issue / inline comments. | n/a |
| **#98** (0104 attach TC) | No review / issue / inline comments. Check-runs: PR CI success (`ce6228f`); merge-SHA `a35927c` `test` check failed then **#99** push CI success (`7d3778c`). Not a product finding for this track. | n/a — 0104 Completed |
| **#97** (0103 docs) | No comments. | n/a |
| **#96** (0103 SLBLOCK) | No comments. | n/a |
| **#88** Bugbot (origin; not in last-four window) | Window-edge skips `normalize_candidate`; over-length not in `seen`. | **This track.** |

Nothing else to mint. No BCC-default track. No HNBITMAPHDR track. Frontend stays **0106+**.

### 2.9 Dual-AI review disposition (2026-08-28)

Reviews: `opencode-review.md` (Ready; no blocker/major) and `agy-review.md` (PASS). Neither asked to reopen C+A, caps, BCC, HNBITMAPHDR, or frontend.

| Id | Source | Severity | Disposition | Spec landing |
|---|---|---|---|---|
| opencode-m1 | opencode-review.md | Minor | **Agree — fold** | Never insert a URL `classify_url` did not return. `seen` must never contain `""`. Plan Phase 1 guard; optional Phase 2 assert. |
| opencode-m2 | opencode-review.md | Minor | **Agree — partial** | Lock insert in **`note_overlength`** (not `try_keep_candidate`-only). **Reorder** `try_keep_candidate` so the max-links `!seen.contains` check runs **before** `note_overlength` — otherwise `max_links_plus_overlength_sets_both_flags_and_prefix` goes red. DoD-2c **must fail on HEAD**. Decline a dedicated second-edge test: `window_dropped` is a bool (idempotent); dual HTML+plain tail is covered once probe consults `seen`. |
| opencode-m3 | opencode-review.md | Minor | **Agree — fold** | Same choke point heals `note_unseen_in` (~133) / SafeLinks-nested tail. Docs one clause. No extra test. |
| opencode-O1 | opencode-review.md | Opportunity | **Already covered** | Live table @ `7d3778c` matches §2.1. |
| opencode-O2 | opencode-review.md | Opportunity | **Agree — fold** | Punctuated fixture: `head+tail` reconstructs **`url + "."`**, not `url`. `:x:` is path-only so `?d=1.` stays document-shaped. |
| opencode-O4 | opencode-review.md | Opportunity | **Already covered** | Deferred / last-PR / crate boundary. |
| agy-0105-1 | agy-review.md | — | **Already covered** | Normalize-then-classify edge snippet. |
| agy-0105-2 | agy-review.md | — | **Agree — partial** | Over-length must join `seen`, but **not** via a `try_keep_candidate`-only insert (misses `note_unseen_in`). Do not copy agy’s post-`note_overlength` insert without the max-links reorder. Constant is `MAX_URL_LEN` (not `MAX_URL_CHARS`). |
| agy-0105-3 | agy-review.md | — | **Already covered** | Unique unseen cut still WINDOW. |
| agy-0105-4 | agy-review.md | — | **Already covered** | New test names; 2c fail-on-HEAD added via m2. |

**Declined / not locked**

- Docs claiming the fix “prevents redundant `max_links` events” as a product outcome (agy exec). Unique over-length past the 50-hit cap **still** notes `max_links` (check before insert). WINDOW honesty is the lie; do not overclaim.
- A second-edge-only fixture (bool flags; probe uses `seen`).
- Inserting the 2048-char **prefix** into `seen`.

---

## 3. In scope

1. `handle_window_edge_bare`: run `normalize_candidate(full, true)` (bare trailing-punct + HTML unescape) **before** `classify_url`. Check `seen` on the classified `final_url`. Never insert unclassified / empty strings.
2. Classified over-length URLs join `seen` **inside `note_overlength`** (classified `final_url`, not the 2048-char prefix). That one site covers `try_keep_candidate`, the edge handler, and `note_unseen_in` (HTML tail, plain tail, SafeLinks-nested over-length). Duplicate edge of an already-noted over-length URL must **not** set `window_dropped`.
3. `try_keep_candidate` overlength: evaluate the max-links `hits >= 50 && !seen.contains` **before** calling `note_overlength`. Existing `max_links_plus_overlength_sets_both_flags_and_prefix` stays green.
4. Non-overlength unique window-edge drops still `note_window_drop` (existing unique-cut test stays green). Cut prefix is never a kept hit. After a classified unseen edge, insert `final_url` (HashSet dup-insert is fine if `note_overlength` already did).
5. Tests in `crates/dedup-engine/src/body_cloud_links.rs` covering punctuated duplicate cut (`head+tail == url + "."`) and over-length-then-edge duplicate (§10.2). Both **must fail on HEAD**.
6. Docs: one sentence on `docs/unique-pst-export.md` body-cloud residual (edge normalize + `note_overlength`→`seen`, including nested-tail); CHANGELOG; close `D-0097-window-edge-normalize` on implement.

---

## 4. Out of scope (do NOT do here)

- 0097 C+A hybrid (`truncated` := dropped candidates; ≤1 marker; split `body_scan_window_capped_messages`). Caps 100k / 2048 / 50.
- Query-string stripping on kept hits.
- Host allowlist / `D-0088-usgovcloud-microsoft-tld`.
- Rewriting `PstFile::list_attachments`, attach-table Strategy A, recipient TC, BCC default.
- HNBITMAPHDR (`D-0100-hn-bitmap-hdr`).
- Per-event attach CRC (`D-0099-attach-crc-job-level`).
- Unique-eml nested MIME (`D-0067-embedded-depth` residual).
- Frontend / Hermes Series O (**0106+**).
- COM Outlook; client PSTs in git; in-tool ScanPST / CRC repair.

---

## 5. Preconditions & dependencies

- **P1 (blocking):** 0097 Completed on `main` (`scan_body_cloud_links`, `handle_window_edge_bare`, C+A flags). Verified @ `7d3778c`.
- *Verified to date:* exact-dup cut test exists and does **not** append a trailing `.`; `try_keep_candidate` overlength does not insert `seen`.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Implementer only adds a test with the existing exact URL | DoD-2 requires a trailing-`.` (or `&amp;` unescape) duplicate that **fails on HEAD**. |
| Stripping query `?` / `.` inside the path | `normalize_candidate` already forbids query delimiters. Do not change that helper’s strip set. Trailing **sentence** `.` only. |
| Inserting over-length into `seen` hides a *different* tail URL | `seen` is exact `final_url`. Unique tail URLs still probe. Existing `body_window_100k_truncates_and_misses_past_window_url` stays green. |
| Changing CSV schema / reason strings | Mapping stays `window_dropped` → `BODY_CLOUD_LINK_WINDOW`. No new reason. |
| Touching `export_exit_0078.rs` | Out of scope (hotspot). |
| Insert inside `note_overlength` without reordering max-links | `try_keep_candidate` today checks `!seen.contains` **after** `note_overlength`. Insert-first would skip `note_max_links`. Reorder: max-links check **then** `note_overlength`. Keep `max_links_plus_overlength_sets_both_flags_and_prefix` green. |
| `try_keep_candidate`-only `seen.insert` | Misses `note_unseen_in` / SafeLinks nested tail (m3). Locked site is `note_overlength`. |

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Edge identity:** `handle_window_edge_bare` normalizes the full original bare match with `normalize_candidate(..., true)` before `classify_url`. `seen` is updated only with strings `classify_url` returned (never `""`). Over-length classified URLs join `seen` **inside `note_overlength`**. `try_keep_candidate` evaluates max-links **before** `note_overlength`. Cut prefixes are still never kept hits. Unique unseen document-shaped cuts still set `window_dropped`. No new reason code. Caps unchanged.
- [ ] **DoD-2 — Tests:** (a) existing `body_window_duplicate_cut_url_not_dropped` / `max_links_duplicate_cut_url_not_truncated` / `body_window_bare_url_cut_at_boundary_not_kept` / `body_window_150k_zero_candidates_not_truncated` / `max_links_plus_overlength_sets_both_flags_and_prefix` / query-preserve tests stay green; (b) **new** punctuated duplicate cut (`head+tail` reconstructs **`url + "."`**, not `url`): kept `:x:` URL in-window, same URL cut at 100k with a trailing sentence `.` → `hits.len()==1`, `window_capped`, **`!window_dropped`**, **`!truncated`**. **Must fail on HEAD**; (c) **new** over-length then edge: in-window over-length `:x:` URL (not a kept hit) plus a later window-edge duplicate of that same classified URL → `url_truncated`, **`!window_dropped`**. **Must fail on HEAD** (today `window_dropped=true`); (d) optional unescape twin (`&amp;` in the cut match vs unescaped kept hit) if cheaper than (b) — not a substitute for (b). No client PSTs in git.
- [ ] **DoD-3 — Docs:** `docs/unique-pst-export.md` body-cloud paragraph: window-edge dedupe uses the same `normalize_candidate` as kept hits; classified over-length URLs join `seen` via `note_overlength` (in-window, edge, and `note_unseen_in` / SafeLinks nested tail). Do **not** claim the fix suppresses `max_links` for a unique over-length URL past the 50-hit cap. CHANGELOG Unreleased one-liner. `D-0097-window-edge-normalize` **closed / 0105**.
- [ ] **DoD-4 — Recorded:** `review.md`; registry **Completed**; ledger commit (`BUGFIX` on `crates/dedup-engine` at implement). No HITL required.

---

## 8. Verification commands (reference)

```powershell
Set-Location C:\dev\Dedupe
$env:CARGO_TARGET_DIR = 'C:\dev\Dedupe\target'
cargo test -p dedup-engine body_window
cargo test -p dedup-engine --lib body_cloud
cargo fmt --all --check
cargo clippy -p dedup-engine --all-targets -- -D warnings
# before implement-track publish:
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
ledgerful verify
```

Filter names re-verify at execute. No operator INC* command. No unique-pst binary run required for DoD.

---

## 9. Deferred roll (mandatory)

Entire `docs/deferred.md` scanned 2026-08-28. Related open rows:

| Row | Disposition |
|---|---|
| **D-0097-window-edge-normalize** | **Absorb and close** on implement. This track. |
| **D-0097-body-cloud-truncate-honesty** | **Already closed in 0097.** Do not reopen C+A. |
| **D-0088-usgovcloud-microsoft-tld** | **Decline.** Do not guess `.microsoft` TLD paths. |
| **D-0094-inc-resmoke** | **Decline.** Operator HITL. Not CI. |
| **D-0100-hn-bitmap-hdr** | **Decline.** Fail-closed until a corpus hits it. |
| **D-0099-attach-crc-job-level** | **Decline.** 0099 declined per-event split. |
| **D-0077-poly-fingerprint** | **Decline.** Later reader track. |
| **D-0079-reader-buffer** | **Decline.** pst-reader buffer polish. |
| **D-0067-embedded-depth** | **Decline.** unique-eml MIME / matter children residual. |
| **D-0071-also-eml** | **Decline.** |
| **D-0062-codesign** | **Decline.** Release ops. |
| Other `docs/deferred.md` rows | **Decline** — not window-edge identity. |

Med/high never parked here. No BCC-default track. Frontend **0106+**.

---

## 10. Product locks (do not reopen)

1. Never mutate source PST / Purview files.
2. Never commit client PSTs, `output/`, `evidence/`, or matter folders with client mail.
3. No `unwrap` / `expect` in production.
4. Crate boundary: scanner in `dedup-engine`. Do not teach `pst-writer` / `pst-reader` body-cloud policy. CLI mapping of `window_dropped` stays.
5. Unique-export: no silent recipient/attach/count drops. This track does **not** add `known_gap`. False WINDOW is the lie being removed; unique unseen cuts still WINDOW.
6. No in-tool ScanPST / CRC repair of evidence.
7. `--include-bcc-recipients` default **off**.
8. Do not change 0097 caps or C+A hybrid.
9. Do not strip query content (`?`, `:`, `=`, `&`, `%`) as trailing punctuation.
10. Do not implement HNBITMAPHDR.
11. Do not start Hermes Series O in this folder.

### 10.1 Locked fix (closed)

**Option: normalize then classify; over-length joins `seen` inside `note_overlength`.**

1. In `handle_window_edge_bare`, after `full_bare_url_from`:
   `let cand = normalize_candidate(full, true);`
   then `classify_url(&cand)` (skip empty cand). Never insert unless `classify_url` returned `Some`.
2. Keep the existing `seen.contains(&final_url)` short-circuit (now compares normalized identity) **before** `note_window_drop`.
3. **`note_overlength` inserts `url` into `seen`** (classified `final_url`, not the 2048-char prefix). Single choke point for `try_keep_candidate` (~390), edge overlength, and `note_unseen_in` (~133).
4. In `try_keep_candidate` overlength: run the max-links check (`hits.len() >= MAX_LINKS_PER_MESSAGE && !seen.contains(&final_url)` → `note_max_links`) **then** `note_overlength(&final_url)`. Do not invert that order.
5. Non-overlength unseen edge: still `note_window_drop`; insert that `final_url` into `seen` after (dup-insert OK). Bool flags stay idempotent; dual HTML+plain tail probes skip once `seen` has the URL.

**Declined:** changing `classify_url` to strip punctuation internally (href path must not strip query/path; SafeLinks nested already uses `normalize_candidate(..., false)`).

**Declined:** a second `seen_raw` set.

**Declined:** `try_keep_candidate`-only insert (leaves nested-tail unhealed).

**Declined:** weakening unique-cut WINDOW (the `.xls` / `xlsx` straddle fixture must still drop).

### 10.2 Tests (minimum)

| Test | Assert |
|---|---|
| Existing `body_window_duplicate_cut_url_not_dropped` | Unchanged exact-string dup. |
| New `body_window_duplicate_cut_url_trailing_period_not_dropped` | Kept `https://contoso.sharepoint.com/:x:/s/L/book.xlsx?d=1`. `head+tail` reconstructs **`url + "."`** (e.g. `tail = "x?d=1."`). `:x:` is path-only so `?d=1.` stays document-shaped; `normalize_candidate` strips the sentence `.`. `hits.len()==1`, `hits[0].url` has **no** trailing `.`, `window_capped`, `!window_dropped`, `!truncated`. **Must fail on HEAD.** |
| New `body_window_overlength_then_edge_duplicate_not_window` | In-window over-length `:x:` URL (`url_truncated`, empty hits) plus later window-edge duplicate of the same classified URL. `url_truncated`, `!window_dropped`. **Must fail on HEAD** (`window_dropped=true` today). |
| Existing `max_links_plus_overlength_sets_both_flags_and_prefix` | Still both `max_links_exceeded` and `url_truncated` (guards the max-links-before-insert order). |
| Existing `body_window_bare_url_cut_at_boundary_not_kept` | Unique unseen cut still `window_dropped`. |
| Existing query-preserve / zero-candidate window | Unchanged. |

Do **not** require `cargo test -p pst-dedup-cli --test unique_pst` for DoD. Mapping is already `window_dropped` → reason string.

Construct the punctuated fixture like the existing duplicate-cut test (`lead` kept URL, pad, `head`+`tail` that reconstructs **`url + "."`**, not `url`). Keep the trailing `.` in `tail` (outside the padded `head`). Re-verify char counts against `MAX_BODY_SCAN_CHARS` at execute.

### 10.3 Arithmetic (plan-time; re-verify)

`MAX_BODY_SCAN_CHARS = 100_000`. Trailing `.` is one Unicode scalar. Pad math: `pad = 100_000 - lead.chars() - head.chars()` (same pattern as ~1073). If execute’s lead/head lengths differ, recompute **before** asserting `window_capped`.
