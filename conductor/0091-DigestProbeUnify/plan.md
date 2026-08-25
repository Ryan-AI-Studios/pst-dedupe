# 0091 — Attach Digest + Probe Unify — Plan

> Phased checklist; map to `spec.md` §7. Execute in `C:\dev\Dedupe`.
>
> **Review fold-in (2026-08-24):** CLI-owned consumers; record-don’t-tee; zero extra reads; budget stricter-of — see `spec.md` §2.5.

> **Ledger:** `ledgerful ledger start crates/pst-dedup-cli --category FEATURE --message "0091 digest+probe unify (scan cache, not engine tee)"`

---

## Phase 0 — Design lock → DoD-1 (partial)

- [ ] Confirm 0090 Completed (or freeze digest entrypoints if overlapping).
- [ ] Trace `hash_attachment_stream` vs `probe_scan_items` / peer caps / timeout worker.
- [ ] Lock cache key (source + msg_nid + attach_nid) and stored fields.
- [ ] Lock budget stricter-of table (digest vs probe bytes/count/time).
- [ ] Lock telemetry: logical probe-bytes vs physical I/O (no double-charge).
- [ ] **Decline** default in-walk tee unless Phase 0 proves peer-cap/timeout can be preserved (expected: no).

## Phase 1 — Implement cache + skip → DoD-1, DoD-3

- [ ] Record digest-pass outcomes during Pass 1.
- [ ] Pass 2 skip when outcome satisfies L3.
- [ ] Leave single-feature paths on old code.
- [ ] No production `unwrap`/`expect`.

## Phase 2 — Equivalence tests → DoD-2

- [ ] Fixture: two-pass vs unify → same winners, preflight, exit, probe tallies.
- [ ] Isolation: deep-preflight alone; body-recip-attach alone.
- [ ] Assert second stream skipped (counter / `stream_available`, not fragile call spies).
- [ ] Budget accounting test.

## Phase 3 — Finalize → DoD-4, DoD-5

- [ ] Close `D-0086-digest-probe-unify`.
- [ ] `review.md`; conductor **Completed**; ledger commit.

---

## Handoff notes

- After 0090.
- Do not sneak in reader buffer or `--jobs`.
- If unify does not drop the second stream on fixtures, document and residual honestly.
