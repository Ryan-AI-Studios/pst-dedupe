# 0076 — Content-Hash Tier Hardening — Plan

> **Ledger:** `ledgerful ledger start 0076-contenthashtierhardening --category FEATURE --message "Identity binding: boundary-safe hash, unread/degenerate + cross-MID guards, Tier 2.5, dedupe scope"`

**Status:** **Completed** — Codex luna PASS WITH DEFERRED P3 (2026-07-28).

## Locks (from spec)

1. **Split-only defaults** — every default-on change refines pre-0076 groups; merge-increasing behavior is flag-gated (§2.3.1). Applies to the §3.2 hash change too: its one exception is split-only and counted
11. **Attribute, don't aggregate** — every split and divergence names the component that caused it (header / body / recipients / attachments); an undifferentiated counter is one operators learn to ignore (§3.6, §3.7)
2. **Never bind on a field we failed to read** — absent ≠ empty (§2.3.2)
3. **Tier 1 normalization frozen**; a MID *conflict* blocks a Tier-2 merge, a MID *match* still binds (§2.3.3)
4. **Tier 2.5 subdivides only** — v2 preimage = v1 preimage ⧺ extras, so equal-v2 ⇒ equal-v1 (§2.3.4)
5. **v1 stays the default identity**; `content_hash_hex` stays stable in existing reports (§2.3.5)
6. **CSV append-only, JSON additive, `keep_set_v1` id retained** (§2.3.6)
7. **One grouping semantics** — `DedupIndex` and `group_candidates` proven equivalent, no third reconciliation path (§2.3.7)
8. **Source PSTs read-only**; **no new I/O on the default path** (§2.3.8–9)
9. **No new hashing dependency** — `sha2 0.11` workspace pin (§3.9)
10. **Message size is not an identity input** — declined and recorded (§3.9)

## Phase 0 — Baseline + preconditions → DoD-11 foundation

- [ ] `ledgerful ledger start …`; `ledgerful scan --impact`; read `.ledgerful/reports/latest-impact.json`
- [ ] Confirm `main` clean at/after `f996392`; if 0077 landed first, rebase (shared `scan.rs`)
- [ ] **Capture the pre-0076 baseline before any edit**: groups, group sizes, winners, `content_hash_hex`, unique counts for `fixtures/aspose_outlook.pst`, `promotions_spam.pst`, and a synthetic multi-source set — checked in as the refinement + golden baseline
- [ ] Record the 0075 ASPOSE winner golden as the "must not move silently" reference

## Phase 1 — `GroupingContext` refactor (no behavior change) → DoD-1

- [ ] Introduce `GroupingContext` / `DedupeScope` / `IdentityLevel` / `Tier1Verify` in `dedup-engine::keepset`
- [ ] `group_candidates(items, &ctx)`, `DedupIndex::with_context(ctx)`; migrate `scan.rs`, `keep_set_cmd.rs`, `unique_pst_cmd.rs`, `attach_probe.rs`, `rebuild_dedup_results`, tests
- [ ] All new fields inert; **`cargo test --workspace` must pass unchanged**
- [ ] Refinement-assertion harness added here and passing trivially against the Phase-0 baseline

## Phase 2 — Hash-input safety → DoD-2

- [ ] `hasher.rs`: clamp the preview by **characters** (`chars().take(4096)`), never by bytes — an ASCII mail must not get a 4096-char window while CJK gets ~1365
- [ ] Keep the clamp in `compute_content_hash` (public API invariant); do not delete it
- [ ] `normalize_subject`: guard the `[3..]` / `[4..]` byte slices
- [ ] `stats.tier2_preview_bytes_over_budget` — the population whose v1 hex will not reproduce
- [ ] Delete (preferred) or wire the dead `pst_dedup_gui::app::Config::body_hash_len` setting — today it claims to set the Tier-2 preview length and nothing reads it
- [ ] Tests: `"\u{3042}".repeat(4096)` no panic **and** a difference at char 3000 still splits; ASCII long-body digest equals a checked-in pre-fix value; Cyrillic 4096-char body changes and is proven split-only; multibyte subject cases
- [ ] Record in `review.md`: hash-preserving except for `normalized.len() > 4096` bytes, that exception is split-only, and it is counted

## Phase 3 — Split-only guards → DoD-3, DoD-4

- [ ] Tier-2 eligibility: `body_incomplete` / `body_unavailable` ⇒ ineligible; degenerate preimage (no body + <2 weak fields) ⇒ ineligible
- [ ] Group **bound MID** + join-compatibility table (§3.4); adopt a joining item's MID when the group has none
- [ ] `stats.tier2_blocked_unreadable_body`, `tier2_blocked_degenerate`, `cross_mid_blocked` + `_groups` + `_max_group` — JSON **and** human summary on scan / keep-set / unique-pst (`_max_group` is the bulk-mail early warning)
- [ ] `--allow-degenerate-tier2`, `--allow-cross-mid-tier2` restore pre-0076 exactly
- [ ] Run the refinement assertion per guard, individually and combined
- [ ] Tests §3.14.3–7

## Phase 4 — Bind provenance → DoD-5

- [ ] `BoundBy` recorded during grouping; **delete `member_tier`** (do not repair its fallback)
- [ ] `bound_by`, `identity_version`, `tier2_eligible` appended to the decision CSV; free text via the 0073 injection-safe writer
- [ ] `DedupIndex` returns the same enum; equivalence test (§3.11) added here
- [ ] Tests §3.14.12–13

## Phase 5 — Tier 2.5 strong identity → DoD-6, DoD-6b, DoD-6c, DoD-6d

- [ ] `pst-reader`: `body_sha256` + `body_char_len` computed **before** truncation, behind an option so the default path is untouched; `display_cc` (0x0E03) soft-read on the already-loaded PC (0075 pattern)
- [ ] **Componentized preimage** — `header` / `body` / `recipients` / `attachments`, each with a truncated `u64` fingerprint stored for attribution only (never binding); this is what makes DoD-6c and DoD-7 exact
- [ ] Layered `--strong-content-hash off|body|body-recip`; recipient normalization (trim / lowercase / split `;` / sort / rejoin)
- [ ] `stats.tier2_5_splits`, `tier2_5_splits_bcc_only`, `tier2_5_splits_recipients_only`, `x500_recipient_items` (segment starting `/O=`)
- [ ] Refinement property test over ≥1000 generated tuples (no proptest dep — deterministic generator)
- [ ] Inline-attachment detection by **MAPI flag** (`PidTagAttachContentId` 0x3712 / `PidTagAttachFlags` 0x3714 `attRenderedInBody` / `PidTagAttachmentHidden`) on the PC `list_attachments` already loads — surface them through `AttachmentMeta`, which today drops even the `mime_tag`/`attach_method` the reader has; `--identity-ignore-inline-attachments` (opt-in, merge-increasing) + `inline_attachments_ignored`. **Or stop** and record **D-0076-inline-attach** with the counter alone
- [ ] Level `body-recip-attach` over 0074's probe **or stop** and record **D-0076-attach-content**; unread attachment ⇒ fall back a level, never fabricate
- [ ] Record **D-0076-recipient-table** (recipient table is the real fix for display-string variance)
- [ ] Tests §3.14.8–9d

## Phase 6 — Divergence signal + scope + backfill → DoD-7, DoD-8, DoD-9

- [ ] `tier1_divergent_body` / `_metadata` / `_recipients` computed from the Phase-5 component fingerprints; human hint fires on **body only** (a counter dominated by 1-second time drift teaches operators to ignore it); `--tier1-verify content|body` splits
- [ ] `--dedupe-scope global|per-source` partitioning both key maps by `path_compare_key(source_path)`; echo scope in JSON + report pack
- [ ] `--tier1-backfill` (default off) + `tier1_backfill_candidates` always reported
- [ ] Tests §3.14.10–11; All Custodians degeneracy under `per-source`

## Phase 7 — CLI + Desk → DoD-10

- [ ] Flags on `scan`, `dups`, `keep-set`, `unique-eml`, `unique-pst` with identical names/help
- [ ] Update **both** parsers (`main.rs`, `unique_pst_cmd.rs`) and their error messages together
- [ ] Desk wizard: single "Strong content hash" checkbox → `body`; arg-mapping unit test; `cargo check -p pst-dedup-gui`

## Phase 8 — Compatibility, determinism, performance → DoD-11..15

- [ ] Refinement assertion green for defaults and each split-only flag
- [ ] 0075 winner golden holds, or every diff maps to a non-zero stat and is re-baselined **in the same commit** with the reason in `review.md`
- [ ] Pre-0076 `keep_set_v1` JSON deserialize; CSV header prefix; `--help` snapshot
- [ ] Integration: multi-source temp-dir run; **full-file SHA-256 of every source unchanged**
- [ ] Timings: default vs `--strong-content-hash body` on fixtures → `review.md` (≤ +2% target, +10% ceiling)

## Phase 9 — Docs → DoD-16

- [ ] `docs/unique-pst-export.md` "Identity and binding": tier table; **named divergences from Relativity's four components**; `per-source` vs `global`; BCC consequence at level ≥ `body-recip` and its 0075 `--prefer-bcc-copy` interaction; Purview edited-but-unsent / copy-on-write as the reason the `tier1_divergent_*` stats exist; closed vocabularies
- [ ] **Bulk-mail warning**: cross-MID blocking inflates newsletters/HR templates most; read `cross_mid_blocked_max_group`; `--allow-cross-mid-tier2` is the culling lever, with the reasoning for both choices
- [ ] **Recipient warning**: display names, not addresses; `"Smith, John"` vs `"John Smith"` vs `/O=EXCHANGELABS/…`; check `tier2_5_splits_recipients_only` + `x500_recipient_items` before trusting `body-recip`
- [ ] **Inline-attachment** guidance and the reproducibility note for non-Latin bodies >~2048 chars (§3.2)
- [ ] State plainly that unique counts **rising** after 0076 is the guards working, not a regression
- [ ] Cross-link 0080 (QC sampling per bind tier), 0081 (runbook)

## Phase 10 — Gate + finalize → DoD-17, DoD-18

- [ ] Targeted tests, then `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace`
- [ ] `ledgerful impact`; `ledgerful verify`
- [ ] Purge anything written under `output\`
- [ ] `review.md`; `D-0076-*` rows in `docs/deferred.md`; **mark D-0075-scope closed / 0076**
- [ ] `conductor.md` + `sequencing.md` → **Completed**
- [ ] `ledgerful ledger commit <tx-id> --summary "…" --reason "…"`

## Suggested order

1. Baseline **first** (worthless after an edit)
2. `GroupingContext` refactor with zero behavior change
3. Panic fix (independently shippable; hash-preserving)
4. The two split-only guards — the actual correctness payload
5. Bind provenance + equivalence test (kills the D5/D6 defect class)
6. Tier 2.5 (skippable at the `body-attach` level)
7. Divergence signal → scope → backfill → CLI/Desk → compat → docs → gate

## Handoff notes

- **Do not** ship anything merge-increasing on by default — that is the one-line reviewer test for this track.
- **Do not** clamp the preview by bytes — it shrinks non-Latin comparison windows. Character clamp only; no other change to the v1 preimage.
- **Do not** gate Tier-2 equality on `PidTagMessageSize`, or drop attachments from identity by a size threshold (declined, §3.9) — inline detection is by MAPI flag.
- **Do not** treat `display_to`/`cc`/`bcc` as addresses; they are display strings that vary between copies of one message.
- **Do not** fabricate a digest for an unread body or attachment.
- **Do not** repair `member_tier` — delete it.
- **Do not** re-baseline the 0075 winner golden without a stat and a written reason.
- **Do not** add a hashing dependency or touch Tier-1 normalization.
- **Rollback:** unregister the flags and flip the two `GroupingContext` guards to `false` — pre-0076 grouping byte-for-byte, panic fix retained.
