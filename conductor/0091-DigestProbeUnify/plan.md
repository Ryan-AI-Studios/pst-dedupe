# 0091 — Attach Digest + Probe Unify — Plan

> Phased checklist; map to `spec.md` §7. Execute in `C:\dev\Dedupe`.
>
> **Review fold-in (2026-08-24):** CLI-owned consumers; record-don’t-tee; zero extra reads; budget stricter-of — see `spec.md` §2.5.

> **Ledger:** `ledgerful ledger start crates/pst-dedup-cli --category FEATURE --message "0091 digest+probe unify (scan cache, not engine tee)"`
> **TX:** `08b6a451-f2bd-48a7-acf5-ffd543f5074f` (open for orchestrator commit after Codex)

---

## Phase 0 — Design lock → DoD-1 (partial)

- [x] Confirm 0090 Completed (or freeze digest entrypoints if overlapping).
- [x] Trace `hash_attachment_stream` vs `probe_scan_items` / peer caps / timeout worker.
- [x] Lock cache key (source + msg_nid + attach_nid) and stored fields.
- [x] Lock budget stricter-of table (digest vs probe bytes/count/time).
- [x] Lock telemetry: logical probe-bytes vs physical I/O (no double-charge).
- [x] **Decline** default in-walk tee unless Phase 0 proves peer-cap/timeout can be preserved (expected: no).

## Phase 1 — Implement cache + skip → DoD-1, DoD-3

- [x] Record digest-pass outcomes during Pass 1.
- [x] Pass 2 skip when outcome satisfies L3.
- [x] Leave single-feature paths on old code.
- [x] No production `unwrap`/`expect`.

## Phase 2 — Equivalence tests → DoD-2

- [x] Fixture: two-pass vs unify → same winners, preflight, exit, probe tallies.
- [x] Isolation: deep-preflight alone; body-recip-attach alone.
- [x] Assert second stream skipped (counter / `stream_available`, not fragile call spies).
- [x] Budget accounting test.

## Phase 3 — Finalize → DoD-4, DoD-5

- [x] Close `D-0086-digest-probe-unify`.
- [ ] `review.md`; conductor **Completed**; ledger commit. *(orchestrator / Codex — internal r1 written)*

---

## Handoff notes

- After 0090.
- Do not sneak in reader buffer or `--jobs`.
- If unify does not drop the second stream on fixtures, document and residual honestly.

## Status notes (2026-08-25)

- Shape: **record-don’t-tee** (seed Full/ok from `AttachDigestResult::Real` only).
- `ProbeResultCache::seed_from_digest_stream` + `charge_pending` + `digest_stream_skips`.
- Pass 2: `probe_scan_items` / `probe_keep_set_groups` accept `seed_cache`; first hit charges logical probe bytes (Head capped at `per_attach_max_bytes`).
- Tests: unit `digest_seed_full_satisfies_head_charges_once`; integration `digest_probe_unify_0091.rs`.
