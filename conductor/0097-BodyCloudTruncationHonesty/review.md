# Track Completion Review — 0097-BodyCloudTruncationHonesty

## Verdict: PASS (implementer gates; Codex r1 P2s fixed)

## Scope

Make `export_body_cloud_links.csv` and unique-pst summary counters **honest** when the body-cloud scanner hits caps (100k / 2048 / 50):

- Split `window_capped` from `truncated` (`truncated` := dropped document-shaped candidates).
- Stop empty-URL `BODY_CLOUD_LINK_TRUNCATED` spam for window-only zero-candidate bodies.
- ≤1 honesty marker per message (`link_index = u32::MAX`) with reason taxonomy.
- Over-length document-shaped URLs (incl. SafeLinks nested targets) are no longer silent drops; 2048-char prefix is marker-only, not a kept hit.
- Window-edge bare-URL guard against garbage `.xls`-from-`.xlsx` hits.

Closes **D-0097-body-cloud-truncate-honesty**.

Branch: `track/0097-BodyCloudTruncationHonesty` (from `483fecd`).

## Reviewers / rounds

| Round | Reviewer | Result |
|---|---|---|
| Spec | Dual-AI Ready (`opencode-review.md` + `agy-review.md`) | Folded into spec §2.8 before implementation |
| Internal | Implementer gates | `cargo fmt --all --check`; clippy workspace `-D warnings`; `cargo test -p dedup-engine` (234); `cargo test -p pst-dedup-cli --test unique_pst` (34); `cargo test --workspace` (exit 0). `ledgerful verify` fmt+clippy ok; test step exceeds the 300s configured timeout (workspace tests passed independently). |
| Codex r1 | gpt-5.6-luna (read-only audit) | **FAIL** — two P2s (`review.codex.r1.md`): probe discarded over-length prefix on tail/post-cap; window-edge ignored `seen` and marked duplicates dropped. **Fixed** in follow-up: probe returns over-length metadata; edge handler suppresses drop for already-seen URLs. |

## DoD matrix

| DoD | Status | Evidence |
|---|---|---|
| DoD-1 | Met | `body_window_150k_zero_candidates_not_truncated`; `body_cloud_window_only_zero_candidates_no_csv_rows` (0 CSV rows, `body_scan_window_capped_messages=1`, truncated_messages=0). Tail/max-links/url-len emit ≤1 marker (`honesty_marker`, `link_index=u32::MAX`). |
| DoD-2 | Met | `body_cloud_links_unique_pst_csv_and_count` still preserves query; over-length prefix is not in `hits` / not counted in `body_cloud_links_total` (`url_longer_than_2048_truncated_not_kept`). |
| DoD-3 | Met | Scanner §2.7 tests inverted/added in `body_cloud_links.rs`; CLI §2.7 in `unique_pst.rs` (window 0-link, tail-drop marker, max-links collision, query preserve). |
| DoD-4 | Met | `D-0097-body-cloud-truncate-honesty` closed in `docs/deferred.md`. Export docs + runbook + CHANGELOG 0085 follow-up name marker semantics, discriminator column, reason strings, split counters. |
| DoD-5 | Met | This `review.md`; conductor **Completed**; ledger `BUGFIX` on `crates/dedup-engine` (commit summary names `pst-dedup-cli`). |

## Key locks honored

- Caps stay 100_000 / 2048 / 50
- Query strings never stripped on kept full hits
- No invent attach rows; no hydrate
- Body-cloud does not set `is_attach_incomplete` / Mode A
- Prefix of over-length URL is marker-only (not a kept hit, does not consume a slot of 50)
- ≤1 honesty marker per message; never for window-only zero-candidate
- Umbrella `BODY_CLOUD_LINK_TRUNCATED` no longer emitted
- Marker `link_index = u32::MAX`
- `body_scan_window_capped_messages` with `serde(default)`
- No production `unwrap`/`expect`

## Deferred

- Operator re-smoke INC0102784 unique-pst: expect `body_scan_window_capped_messages` ≈ 62 and `body_cloud_link_truncated_messages` only where a tail/cap actually dropped a document-shaped candidate (not CI).
- `D-0088-usgovcloud-microsoft-tld` unchanged (no sovereign miss proven here).
- unique-eml / GUI / attach NPMAP / hasher identity / perf rewrite of remainder probe — out of scope.

## Codex r1 dispositions

| Finding | Disposition |
|---|---|
| P2 tail/post-cap probe discarded over-length metadata | **Fixed.** `probe_unseen_document_candidates` returns `found` + first over-length URL; callers set `url_truncated` + prefix. Tests: `body_window_tail_overlength_sets_url_truncated_and_prefix`, `max_links_plus_overlength_sets_both_flags_and_prefix`, CLI `body_cloud_window_tail_overlength_marker_prefix`, `body_cloud_max_links_plus_overlength_marker_prefix`. |
| P2 window-edge guard marked deduplicated URLs as dropped | **Fixed.** `handle_window_edge_bare` skips the drop flag when `acc.seen` already has the classified URL; cut prefix is still rejected as a real hit. Test: `body_window_duplicate_cut_url_not_dropped`. |

## Operator note

The INC0102784 62 empty truncated rows were **window-hit markers**, not 62 lost SharePoint links. After 0097 those messages produce 0 CSV rows unless a document-shaped candidate was actually dropped past a cap.
