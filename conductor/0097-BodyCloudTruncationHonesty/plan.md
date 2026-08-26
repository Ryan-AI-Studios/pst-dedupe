# 0097 — Body-Cloud Truncation Honesty — Plan

> Phased checklist; map to `spec.md` §7. Execute in `C:\dev\Dedupe`.
>
> **Review fold-in (2026-08-26):** Phase 0 **closed** (62 empty rows = window-hit, not lost
> links). Policy **C+A hybrid** + split `body_scan_window_capped_messages`. Silent 2048 drop
> and window-edge bare-URL guard in scope. Prefix is a marker, not a kept hit. See `spec.md` §2.8.

> **Ledger:** `ledgerful ledger start crates/dedup-engine --category BUGFIX --message "0097 body-cloud truncation honesty"`
> (commit summaries must also name `pst-dedup-cli`: CSV emit + summary field).

---

## Phase 0 — Diagnose → DoD-1

- [x] Trace `REASON_BODY_CLOUD_LINK_TRUNCATED` emission sites.
- [x] Classify INC0102784 empty rows: **window-hit** (unconditional `truncated` on >100k body). URL-length is silent; links/message only fires on real drops; window-edge produces garbage **real** rows, not empty markers.
- [x] Lock CSV policy: **C+A hybrid** (`spec.md` §2.5). Not B. Not A-for-every-window.

## Phase 1 — Scanner → DoD-1, DoD-3 (engine)

- [x] `BodyCloudScan.window_capped: bool` independent of `truncated`.
- [x] Set `truncated` **only** when a document-shaped candidate was actually dropped.
- [x] On window fire: rescan un-windowed tail via extracted `has_document_candidates` (today’s `more_document_candidates_beyond`).
- [x] Over-length document-shaped URL (and SafeLinks nested target): set `truncated`; stash first 2048-char prefix; **do not** keep as a hit; **do not** silent-`return`.
- [x] Window-edge: reject cut bare-URL matches on a windowed surface (`spec.md` §2.6). Do not extend the 100k cap.
- [x] Invert/replace: `body_window_100k_truncates_and_misses_past_window_url`, `body_window_url_inside_window_still_hits`, `url_longer_than_2048_skipped`. Add 0-link 150k + straddling-URL tests (`spec.md` §2.7).

## Phase 2 — CLI CSV + summary → DoD-1, DoD-2, DoD-3 (cli)

- [x] `PreparedWinner`: thread `window_capped` + truncate reasons + over-length prefix.
- [x] Real rows: kept hits only; `reason=BODY_CLOUD_LINK`; `truncated=false`; full query.
- [x] Stop `truncated_marker` for window-only zero-hit.
- [x] ≤1 marker when `truncated`: reasons `BODY_CLOUD_LINK_WINDOW` / `BODY_CLOUD_LINK_MAX_LINKS_EXCEEDED` / `BODY_CLOUD_LINK_URL_TRUNCATED` (pipe-join); `link_index=u32::MAX`; prefix only on url-len.
- [x] Stop emitting umbrella `BODY_CLOUD_LINK_TRUNCATED`.
- [x] `body_scan_window_capped_messages` with `serde(default)`; fix truncated-counter doc comment.
- [x] unique_pst tests: 0-link window, tail-drop marker, collision, query preserve (`spec.md` §2.7).

## Phase 3 — Docs + deferred → DoD-4

- [x] `docs/unique-pst-export.md` + `docs/unique-pst-ediscovery-runbook.md`: marker = dropped document-shaped candidates; `truncated` column is a discriminator; reason strings; split counters; 2048 prefix is not a live URL.
- [x] CHANGELOG 0085 follow-up line (0097).
- [x] Close `D-0097-body-cloud-truncate-honesty`.

## Phase 4 — Finalize → DoD-5

- [x] `review.md`; conductor **Completed**; ledger commit.

---

## Handoff notes

- Do not treat empty truncated rows as proof of 62 SharePoint links — that was the bug.
- Do not raise 100k / 2048 / 50 to “fix” honesty.
- Do not count a 2048-char prefix as `body_cloud_links_total`.
- Do not put body-cloud fields in hasher / attach identity.
- Hygiene: untracked root `agy-review.md` — do **not** commit.
