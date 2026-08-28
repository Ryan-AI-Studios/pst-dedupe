# 0105 — Body-Cloud Window-Edge Normalize — Plan

> Phased checklist mapped to `spec.md` §7. Planning-only Phase 0 is **closed**. Do not implement until the user says Implement.
>
> **Ledger (implement):** `ledgerful ledger start crates/dedup-engine --category BUGFIX --message "0105 body-cloud window-edge normalize"`
>
> **Fold-in (2026-08-28):** `opencode-review.md` + `agy-review.md` → spec §2.9 / `foldin-note.md`. Lock `note_overlength` insert; max-links check **before** that insert; punctuated fixture is `url + "."`.

---

## Phase 0 — Spec expand → Ready (closed 2026-08-28)

- [x] Live `handle_window_edge_bare` classifies raw `full` and checks `seen` without `normalize_candidate` (`body_cloud_links.rs` ~249 @ `7d3778c`).
- [x] Exact-dup tests exist; trailing-`.` / over-length-seen do **not**.
- [x] Locked fix: normalize then classify; over-length joins `seen` **inside `note_overlength`**; max-links check **before** that insert.
- [x] Deferred §9; last-PR comments (#99–#96 none; origin #88 is this track). Frontend **0106+**.
- [x] Status **Ready — not started**.
- [x] Fold-in: never insert unclassified/`""`; `head+tail == url + "."`; DoD-2c fail-on-HEAD; `note_unseen_in` heals via the same choke point.

Re-verify at execute: `handle_window_edge_bare` still has no `normalize_candidate`; `try_keep_candidate` overlength still skips `seen.insert`.

---

## Phase 1 — Scanner → DoD-1

File: `crates/dedup-engine/src/body_cloud_links.rs` (re-verify line numbers at execute; plan-time `main` @ `7d3778c`).

- [ ] In `handle_window_edge_bare`, after `full_bare_url_from(original, m_start)`:

  ```rust
  let cand = normalize_candidate(full, true);
  if cand.is_empty() {
      return true;
  }
  if let Some((final_url, _, overlength)) = classify_url(&cand) {
      if acc.seen.contains(&final_url) {
          return true;
      }
      acc.note_window_drop();
      if overlength {
          acc.note_overlength(&final_url);
      }
      acc.seen.insert(final_url);
  }
  true
  ```

  Shape may vary; **must** normalize with `strip_trailing_punct=true` before classify. Empty cand → treat as handled (cut prefix still not kept). **Never insert** a URL `classify_url` did not return; `seen` must never contain `""`. The `seen.insert` stays **inside** the `Some` arm (covers non-overlength unique edges; HashSet dup-insert is fine if `note_overlength` already inserted).
- [ ] Over-length joins `seen` **inside `note_overlength`** (classified `final_url`, not the 2048-char prefix). That one site must also cover `try_keep_candidate` (~390) and `note_unseen_in` (~133 / SafeLinks nested tail). A `try_keep_candidate`-only insert leaves the nested-tail path unhealed.
- [ ] In `try_keep_candidate` overlength: evaluate `hits.len() >= MAX_LINKS_PER_MESSAGE && !acc.seen.contains(&final_url)` → `note_max_links` **then** `note_overlength(&final_url)`. Do **not** insert first — that would skip `note_max_links` (`max_links_plus_overlength_sets_both_flags_and_prefix`).
- [ ] Do **not** change `normalize_candidate`’s strip set (no `?` `:` `=` `&` `%`).
- [ ] Do **not** change `classify_url` punctuation policy (href / SafeLinks nested stay as-is).
- [ ] Do **not** edit `pst-writer`, `pst-reader`, GUI, `export_exit_0078.rs`, or unique-pst CSV mapping.
- [ ] Caps and C+A flags stay.

---

## Phase 2 — Tests → DoD-2

Same file `#[cfg(test)]`. Follow existing `body_window_duplicate_cut_url_not_dropped` pad math.

- [ ] `body_window_duplicate_cut_url_trailing_period_not_dropped` — kept
  `https://contoso.sharepoint.com/:x:/s/L/book.xlsx?d=1`; `head+tail` reconstructs **`url + "."`** (not `url`; e.g. `tail = "x?d=1."`, keep the `.` outside the padded `head`). Assert `hits.len()==1`, hit URL **without** trailing `.`, `window_capped`, `!window_dropped`, `!truncated`. **Must fail on unpatched HEAD.**
- [ ] `body_window_overlength_then_edge_duplicate_not_window` — in-window over-length `:x:` URL (reuse `a.repeat(3000)` query pattern from `url_longer_than_2048_truncated_not_kept`) plus a later window-edge duplicate of the same classified URL. `hits` empty, `url_truncated`, `window_capped`, `!window_dropped`. **Must fail on unpatched HEAD** (today `window_dropped=true` + `truncated=true`).
- [ ] Keep green: `body_window_duplicate_cut_url_not_dropped`, `max_links_duplicate_cut_url_not_truncated`, `body_window_bare_url_cut_at_boundary_not_kept`, `body_window_150k_zero_candidates_not_truncated`, `max_links_plus_overlength_sets_both_flags_and_prefix`, `query_preserve_on_href` / plain query test.
- [ ] Optional: `!scan` path / accum never contains `""` in `seen` after the new fixtures. Not a substitute for the trailing-`.` test.
- [ ] Optional unescape twin is **not** a substitute for the trailing-`.` test.

No temp PST. No `unique_pst` integration test required.

---

## Phase 3 — Docs → DoD-3

- [ ] `docs/unique-pst-export.md` **`export_body_cloud_links.csv`** bullet (~588): after 0097 marker vocabulary, add: window-edge bare dedupe uses the same `normalize_candidate` (trailing sentence punct + HTML unescape) as kept hits; classified over-length URLs join `seen` via `note_overlength` so in-window, edge, and `note_unseen_in` (including SafeLinks nested tail) share identity and do not emit an extra `BODY_CLOUD_LINK_WINDOW`. Do **not** claim unique over-length URLs past the 50-hit cap skip `max_links`. Residual `D-0097-window-edge-normalize` **closed / 0105** (on implement).
- [ ] `CHANGELOG.md` Unreleased: body-cloud window-edge duplicate with trailing punctuation no longer emits a false WINDOW marker. Closes **D-0097-window-edge-normalize**.
- [ ] `docs/deferred.md`: mark `D-0097-window-edge-normalize` **closed / 0105** on implement complete; this planning pass only notes the owner is Ready.

---

## Phase 4 — Finalize → DoD-4

- [ ] `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test -p dedup-engine body_window` (filter re-verify); workspace tests before publish.
- [ ] Write `review.md` in this track dir: results, evidence, no new deferred (row closed).
- [ ] Update `../conductor.md`: this track **Completed**. Light `sequencing.md` / `ROADMAP.md`.
- [ ] Commit the implement ledger transaction (`BUGFIX` on `crates/dedup-engine`).
- [ ] Notify: frontend Series O, if started, uses **0106+**. No BCC track. No HNBITMAPHDR.

---

## Handoff notes

- Planning-only until Implement. Product crates unchanged in this pass.
- Single-exe / no-daemon constraint unchanged (library scanner only).
- Rollback: revert `handle_window_edge_bare` + over-length `seen` insert + tests + docs. No on-disk CSV schema change.
- Do not “fix” WINDOW by dropping unique unseen cuts.
- Do not chase `C:\dev\Dedupe-plan.md` (absent).
- Hotspot `crates/pst-dedup-cli/tests/export_exit_0078.rs` is **out of scope**.
- Do not mkdir Tauri/frontend under this ID.
