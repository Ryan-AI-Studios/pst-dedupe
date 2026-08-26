# 0097 — Body-Cloud Truncation Honesty

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.

- **Track ID:** 0097-BodyCloudTruncationHonesty
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → Series N
- **Cross-repo contract:** n/a
- **Status:** Completed
- **Depends on:** 0085 · 0088 (all **Completed**)
- **Spec authored:** 2026-08-25
- **Series:** N (Operator fidelity — INC0102784 post-0092)
>
> **Review fold-in (2026-08-26):** dual-AI Ready review (`opencode-review.md` + `agy-review.md`) incorporated below.
> Disposition of each claim is in §2.8. Phase 0 diagnosis is **closed**. Policy is **C+A hybrid**
> (truncated := dropped document-shaped candidates; ≤1 marker per message) plus a split
> `body_scan_window_capped_messages` counter. Caps stay 100k / 2048 / 50.

---

## 1. Objective

Make `export_body_cloud_links.csv` / summary counters **honest** when the body-cloud scanner
hits caps (100k body window, 2048 URL length, 50 links/message): stop emitting large numbers of
`BODY_CLOUD_LINK_TRUNCATED` rows with **empty** `cloud_url` that look like “found links we lost,”
and stop **silently dropping** over-length document-shaped URLs with no marker.

**Promotes / closes:** `D-0097-body-cloud-truncate-honesty` (close on DoD).  
May optionally touch `D-0088-usgovcloud-microsoft-tld` only if evidence shows a sovereign miss (out of primary scope).

---

## 2. Context (read before starting)

### 2.1 Operator evidence (INC0102784)

| Metric | Value |
|---|---|
| `export_body_cloud_links.csv` rows | **65** |
| `BODY_CLOUD_LINK` (real URL) | **3** (2 messages; `deo.sharepoint.com` `:x:` / `:b:`) |
| `BODY_CLOUD_LINK_TRUNCATED` | **62** — **empty** `cloud_url` / `url_source` |
| Summary | `messages_with_body_cloud_links=2`, `body_cloud_links_total=3`, `body_cloud_link_truncated_messages=62` |
| Attach-table cloud | **0** `cloud_provider` / `cloud_url` nonempty |

**Closed diagnosis (not “62 lost SharePoint links”):** the 62 empty rows are **window-hit markers**.
`scan_body_cloud_links` sets `truncated = true` whenever HTML *or* plain exceeds 100k chars,
**before** any candidate is examined (`body_cloud_links.rs` ~90–93, ~100–103). The scanner’s own
test `body_window_100k_truncates_and_misses_past_window_url` asserts that bug. Window-hit is the
**only** cause that fires with zero hits. ~60 of those 62 messages had **no** document-shaped
cloud link in the scanned window. Counsel reading the CSV may believe 62 cloud links were found
and dropped.

### 2.2 Live code snapshot (verified 2026-08-26, `main` @ `483fecd`)

| Surface | State |
|---|---|
| Caps | `MAX_BODY_SCAN_CHARS=100_000`, `MAX_URL_LEN=2_048`, `MAX_LINKS_PER_MESSAGE=50` |
| `BodyCloudScan.truncated` | Set **unconditionally** on window overflow; also set when max-links actually drops a new document-shaped candidate |
| URL-length cap | `try_keep_candidate` ~283 and SafeLinks nested target ~371–373 **`return` / `None` with no `truncated` flag** — silent drop |
| Marker row | `BodyCloudLinkRow::truncated_marker` hardcodes `link_index: 0`, empty `cloud_url`/`url_source`, `reason: BODY_CLOUD_LINK_TRUNCATED` |
| CLI emit | `unique_pst_cmd.rs` ~2517–2527: if `p.body_cloud_truncated` → increment counter **and** push marker (even with 0 hits) |
| Real rows | `truncated: false` always (`~2511`); `truncated` CSV column is a **row-type discriminator**, not per-URL truncation |
| Summary comment | `body_cloud_link_truncated_messages` doc says “caps truncated additional candidates” (`unique_export_report.rs` ~671) — **does not match** current window-hit semantics |
| Reader | `PID_TAG_BODY` / `PID_TAG_BODY_HTML` are read in full; 100k window is the only gate |
| CSV header | `source_id,source_path,folder_path,msg_nid,link_index,cloud_url,url_source,truncated,message_subject,reason` (locked 0085) |
| Tests asserting the bug | `body_window_100k_truncates_and_misses_past_window_url`; `body_window_url_inside_window_still_hits` (padding after a hit still sets `truncated`); `url_longer_than_2048_skipped` (empty hits, **no** truncated assert) |
| CLI coverage | `body_cloud_links_unique_pst_csv_and_count` — small body, `truncated_messages == 0`, query preserved. **No** >100k body through unique-pst; **no** marker-shape / `link_index` collision test |

### 2.3 Product locks (0085, restated)

1. Caps remain: 100_000 body window, 2048 URL length, 50 links/message. **Do not raise caps to “fix” honesty.**
2. Query strings never stripped on **kept full hits**.
3. No invent attach rows from body links; no hydrate.
4. Summary counters must match ledger semantics after the honesty fix.
5. Body-cloud hits do not set `is_attach_incomplete` / Mode A (0085 known gap — unchanged).
6. No production `unwrap`/`expect`. Fixtures in CI; INC* re-smoke is operator-local.

### 2.4 Phase 0 classification (closed)

| Cause | Fires with 0 hits? | Sets `truncated` today? | INC0102784 62 empty rows? |
|---|---|---|---|
| Body window >100k | **Yes** (unconditional) | **Yes** | **Yes — this is the 62** |
| Max links (>50 document-shaped) | No (only when a new candidate is dropped) | Yes, correctly | No |
| URL length >2048 | Yes (hits stay empty) | **Never** (silent) | No (under-report, opposite gap) |
| Window-edge bare-URL cut | Produces a **garbage real row**, not an empty marker | n/a | No |

Plan Phase 0 “classify A/B/C” is **done**. Do not re-open it during implementation.

### 2.5 Policy (locked) — C+A hybrid + split window counter

**Not B.** Suppressing every empty row and relying on a mislabeled summary hides real drops
and leaves `body_cloud_link_truncated_messages` meaning “body > 100k”.

**Not A-as-originally-stated.** One marker for every windowed message (including 0-candidate
bodies) **is** the 62-row spam.

**Locked: C then A, plus a window-capped summary field.**

| Event | CSV | `BodyCloudScan` | Summary |
|---|---|---|---|
| Body >100k, **no** document-shaped candidate in the window **or** the un-windowed tail | **0 rows** (no phantom) | `window_capped=true`, `truncated=false` | `body_scan_window_capped_messages += 1`; truncated counter **unchanged** |
| Body >100k, document-shaped candidate(s) **in the tail** past the window | **≤1 marker** / message | `window_capped=true`, `truncated=true` | both window-capped **and** truncated counters |
| ≥51 document-shaped candidates (within window) | 50 real `BODY_CLOUD_LINK` rows + **≤1 marker** | `truncated=true` | truncated counter |
| Document-shaped URL >2048 chars (incl. SafeLinks nested target) | **≤1 marker** / message; `cloud_url` = first over-length candidate’s **2048-char prefix** (tenant/path visible) | `truncated=true`; prefix is **not** a kept hit | truncated counter; **not** `body_cloud_links_total` |
| Combination of the above drop causes | still **≤1 marker** / message | `truncated=true` | truncated counter once |

**Kept full hits (real rows):**

- `reason = BODY_CLOUD_LINK`
- `truncated = false` (column remains a **row-type discriminator** for real vs marker)
- Full URL including query (lock 2)
- Count in `messages_with_body_cloud_links` / `body_cloud_links_total`
- Fill the 50-link cap **first**; honesty markers do **not** consume a kept-hit slot

**Honesty marker (when `truncated=true`):**

- **One per message**, never for window-only zero-candidate
- `reason` is one or more of (pipe-join if multiple):
  - `BODY_CLOUD_LINK_WINDOW`
  - `BODY_CLOUD_LINK_MAX_LINKS_EXCEEDED`
  - `BODY_CLOUD_LINK_URL_TRUNCATED`
- Stop emitting the umbrella `BODY_CLOUD_LINK_TRUNCATED` (CHANGELOG + export docs must say the string is gone so counsel greps update)
- `link_index = u32::MAX` (`4294967295`) — **must not** collide with real `link_index: 0`
- `url_source` empty on markers (cause lives in `reason`)
- `cloud_url` empty except URL-over-length, where it holds the 2048-char prefix
- `truncated = true`

**Do not** treat the 2048-char prefix as a kept `BODY_CLOUD_LINK` hit: it is not query-complete,
must not increment `body_cloud_links_total`, and must not occupy a slot of the 50.

**CSV header:** keep the 0085 header. Distinct `reason` strings carry the taxonomy. Do **not**
require a new `truncate_reason` column (MAY append on the right later; not DoD).

**New summary field:** `body_scan_window_capped_messages: u64` with `#[serde(default)]` so older
`summary.json` still loads. Correct the `body_cloud_link_truncated_messages` doc comment to
“messages where document-shaped candidates were actually dropped (window tail / max-links / url-len).”

### 2.6 Scanner + CLI wiring (locked)

| Crate | Change |
|---|---|
| `dedup-engine` (`body_cloud_links.rs`) | Add `BodyCloudScan.window_capped: bool` (independent of `truncated`). Set `truncated` **only** when a document-shaped candidate was actually dropped. On window fire: rescan the **un-windowed tail** (reuse / extract the `more_document_candidates_beyond` probe as `has_document_candidates`). Over-length document-shaped candidates set `truncated` and keep the first 2048-char prefix for the marker — **do not** `return` silently; same for SafeLinks nested targets. Invert tests that assert window ⇒ `truncated` with zero tail candidates. |
| `pst-dedup-cli` | `PreparedWinner`: keep `body_cloud_truncated`; add `body_cloud_window_capped` + truncate-reason bits/prefix as needed. Emit real rows from kept hits only. Emit **at most one** marker when `truncated`. Stop pushing `truncated_marker` for window-only zero-hit. Marker `link_index = u32::MAX`. Wire `body_scan_window_capped_messages`. |

**Window-edge guard (bare URL):** `body_window_str` cuts at exactly 100k chars. `bare_url_re` has
no closing delimiter, so a bare URL straddling the boundary can match a prefix that
`ends_with(".xls")` etc. and become a **garbage real row**. Hrefs are safe (closing quote required).

**Lock:** when the body is windowed, do **not** keep a bare-URL hit whose match is cut by the
window (match ends at the window boundary and the next original char would continue the URL,
or equivalently: reject bare matches that end at `text.len()` on a windowed surface). Do **not**
extend the 100k cap by `MAX_URL_LEN`. Add a unit test: document-shaped URL straddling 100k
(`…/book.xls` cut from `…/book.xlsx?d=…`) must **not** emit a real `BODY_CLOUD_LINK` row;
`truncated` may be true if the full URL is document-shaped.

**Tail rescan:** extract a shared `has_document_candidates(text, seen)` (today’s
`more_document_candidates_beyond`) and call it on the remainder when the window fires. Set
`truncated` only if it returns true. HTML and plain windows OR into one `window_capped` /
one `truncated` per message — do not double-count.

**Out of wiring:** unique-eml, GUI, attach-table NPMAP, hasher / keep-set identity (body-cloud
never enters those).

### 2.7 Tests (locked)

**Scanner (`cargo test -p dedup-engine`):**

1. 150k body, 0 cloud links → `window_capped=true`, `truncated=false`, `hits` empty.
2. Prefix fills 100k + document-shaped URL **past** the window → `hits` empty, `truncated=true`, `window_capped=true` (today’s `body_window_100k_truncates_and_misses_past_window_url` keeps truncated **because the tail has a candidate**, not because the window fired).
3. Hit inside window + padding past 100k, no further candidates → `hits.len()==1`, `truncated=false`, `window_capped=true` (invert `body_window_url_inside_window_still_hits`).
4. 60 distinct document-shaped hrefs → 50 hits, `truncated=true` (existing max-links test stays).
5. 2500-char document-shaped SharePoint URL → 0 kept hits, `truncated=true`, first-2048 prefix available for the marker (replace `url_longer_than_2048_skipped` silent-empty).
6. Bare URL straddling the 100k boundary (window-edge) → no garbage real hit.

**CLI (`cargo test -p pst-dedup-cli --test unique_pst`):**

1. >100k body, 0 cloud links → 0 CSV rows; `body_scan_window_capped_messages=1`; `body_cloud_link_truncated_messages=0`.
2. >100k body, document-shaped URL **past** the window → 1 marker (`BODY_CLOUD_LINK_WINDOW`, `link_index=u32::MAX`), 0 real rows; both counters = 1.
3. Small body with a real hit + max-links or window-drop → real `link_index=0` **and** marker `link_index=u32::MAX` (no collision).
4. Existing `body_cloud_links_unique_pst_csv_and_count` still: real rows preserve query; small body truncated_messages=0.

### 2.8 Dual-AI review disposition (2026-08-26)

| # | Claim | Source | Disposition | Spec landing |
|---|---|---|---|---|
| O1 | 62 empty rows are window-hit, not lost links; Phase 0 classifiable from code | opencode | **Agree** | §2.1; §2.4 |
| O2 | Policy C+A hybrid, not B: `truncated` := dropped document-shaped candidates; ≤1 marker | opencode | **Agree** | §2.5 |
| O3 | Silent 2048-char URL cap is an honesty gap (under-report) | opencode | **Agree** | §2.5 url-len row |
| O4 | Window-edge bare URL can become a garbage real row; guard + test | opencode | **Agree** | §2.6 |
| O5 | Marker `link_index: 0` collides with real rows | opencode | **Agree** — use `u32::MAX` | §2.5 |
| O6 | §8 omits `dedup-engine`; ledger entity is the scanner crate | opencode | **Agree** | §8; plan ledger |
| O7 | Docs/CHANGELOG must describe marker semantics; fix summary doc comment | opencode | **Agree** | DoD-4 |
| O8 | `truncated` CSV column is a row-type discriminator | opencode | **Agree** | §2.5; docs |
| O9 | Reuse `more_document_candidates_beyond` for tail rescan | opencode | **Agree** as the impl approach | §2.6 |
| O10 | Add a `truncate_reason` CSV column | opencode | **Decline as DoD** — encode taxonomy in `reason`; MAY append later | §2.5 |
| O11 | Option (b): document 2048 cap as a known miss and leave it silent | opencode | **Decline** — silent drop is in-scope to fix | §2.5 |
| A1 | Empty-URL phantom rows must go; 0-link windowed body → 0 CSV rows | agy | **Agree** | §2.5 first row |
| A2 | Split `body_scan_window_capped_messages` from `body_cloud_link_truncated_messages` | agy | **Agree** | §2.5 |
| A3 | Over-length URL: truncate to 2048, **keep in `hits`**, count as a real row | agy | **Decline as kept hit.** Prefix on the **marker** only; not `body_cloud_links_total`; does not consume the 50 | §2.5 |
| A4 | Distinct max-links / url-len reason strings | agy | **Agree** | §2.5 |
| A5 | DoD: 150k / 0 links → 0 CSV rows, window_capped=1, truncated_messages=0 | agy | **Agree** | §2.7 |
| A6 | DoD: 2500-char SP URL captured as a truncated **hit** | agy | **Partial** — honesty marker + prefix, not a kept hit | §2.5; §2.7.5 |
| A7 | `more_document_candidates_beyond` full-text rescan is quadratic / inefficient | agy | **Decline as DoD** (perf, not honesty). Helper extract is in-scope for C | — |

**Declined / not locked**

- Raising 100k / 2048 / 50 caps.
- Option B (summary-only, no markers when candidates were dropped).
- Option A for every windowed message including 0-candidate bodies.
- Treating a 2048-char prefix as a kept `BODY_CLOUD_LINK` / filling a slot of the 50.
- Required new CSV column.
- Rewriting the max-links remainder probe for performance.
- unique-eml / GUI / attach-table NPMAP / hasher identity.
- Sovereign `.microsoft` TLD (`D-0088-usgovcloud-microsoft-tld`) unless a concrete miss is proven here.

---

## 3. In scope

1. Split `window_capped` vs `truncated` in the scanner; tail-rescan so `truncated` means dropped document-shaped candidates.
2. Stop emitting empty-URL markers for window-only zero-candidate messages.
3. One honesty marker per message when drops occurred, with distinct reasons; `link_index = u32::MAX`.
4. Honest handling of >2048-char document-shaped URLs (marker + prefix, not silent drop).
5. Window-edge bare-URL guard.
6. Summary field `body_scan_window_capped_messages`; align truncated counter + docs.
7. Tests in §2.7; close `D-0097-body-cloud-truncate-honesty`.

## 4. Out of scope

- Attach-table NPMAP / PermissionType (`0096`, Completed).
- Sovereign `.microsoft` TLD expansion (`D-0088-usgovcloud-microsoft-tld`) unless a concrete miss is proven here.
- Hydration / inventing Attachment Table rows.
- Raising scanner caps.
- unique-eml / GUI body-cloud surfaces.
- Performance rewrite of `more_document_candidates_beyond` beyond extracting the shared helper.

## 5. Preconditions & dependencies

- **P1:** 0085/0088 body-cloud pack exists.
- *Verified:* INC0102784 CSV shows 62 empty truncated vs 3 real links; code proves window-hit.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Hiding true truncations | Marker still emitted when candidates were actually dropped; split window counter |
| Raising caps “to fix” honesty | Forbidden (lock 1); semantic fix only |
| Counsel still greps `BODY_CLOUD_LINK_TRUNCATED` | CHANGELOG + export docs + runbook name the new reason strings |
| Prefix row mistaken for a live URL | `truncated=true` + `BODY_CLOUD_LINK_URL_TRUNCATED`; not counted as a kept hit |
| Header / summary.json back-compat | No required new CSV column; new summary field `serde(default)` |
| Existing unit tests assert the bug | Invert them in the same PR (DoD-3) |

## 7. Definition of Done

Complete only when ALL hold:

- [x] **DoD-1 —** Empty-URL truncate spam gone: window-only 0-candidate bodies emit **0** CSV rows. When document-shaped candidates were dropped, **≤1** honesty marker per message (`link_index = u32::MAX`). Summary: `body_cloud_link_truncated_messages` counts those messages only; `body_scan_window_capped_messages` counts bodies that exceeded 100k. INC0102784-class input must not show ~62 empty truncated URL rows.
- [x] **DoD-2 —** Real `BODY_CLOUD_LINK` rows still emitted with **full query**. Prefix honesty rows are not kept hits.
- [x] **DoD-3 —** §2.7 scanner + CLI tests (window 0-link, tail-drop marker, max-links, over-length, window-edge, `link_index` collision). Tests that encoded the old window⇒truncated semantics are inverted.
- [x] **DoD-4 —** `D-0097-body-cloud-truncate-honesty` **closed**. unique-pst export docs + runbook + CHANGELOG 0085 follow-up: marker semantics, reason strings, discriminator column, split counters.
- [x] **DoD-5 — Recorded:** `review.md`; conductor **Completed**; ledger commit (`BUGFIX`).

## 8. Verification commands (reference)

```powershell
cargo fmt --all --check
cargo clippy -p dedup-engine -p pst-dedup-cli --all-targets -- -D warnings
cargo test -p dedup-engine
cargo test -p pst-dedup-cli --test unique_pst
# operator: unique-pst INC0102784; body_cloud CSV should not show ~62 empty truncated URL rows
#           expect body_scan_window_capped_messages ≈ 62 and body_cloud_link_truncated_messages
#           only where a tail/cap actually dropped a document-shaped candidate
```
