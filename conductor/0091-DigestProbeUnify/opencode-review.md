# opencode-review — 0091 DigestProbeUnify (spec/plan review, review only)

- **Series context / verdict summary:** see `../../opencode-review.md` — verdict: **Sound intent, wrong snapshot**.
- **Method:** code snapshot claims verified against `main` @ `c5437d0`; no code edits made.

**Findings / blind spots:**

1. **The §2.2 snapshot is wrong on both rows.** L2/L3 probes live in `crates/pst-dedup-cli/src/attach_probe.rs` (`dedup-engine/src/integrity.rs` holds reason codes/tallies), and the streaming SHA-256 lives in `crates/pst-dedup-cli/src/attach_content_hash.rs` (not `hasher`/`grouping`). The plan's risk mitigation "Keep API private to dedup-engine unless CLI needs it" is **inverted** — both consumers are in the CLI crate today, so a dedup-engine-private API cannot unify them without first moving code. Fix the snapshot, the plan's API lock, and the ledger category before start.
2. **Phase mismatch makes "one tee" the wrong shape.** The digest runs *during the message walk*, before grouping (`scan.rs:836`); the deep-attach probe runs *after* the walk over `candidates` (degraded groups / keep-set peers, `scan.rs:1350-1376`), with its own budgets, peer caps, and a worker-thread + `recv_timeout` design that takes `PstFile` ownership (`attach_probe.rs:838-860`). The two consumers want different attach subsets at different times. Two realistic designs:
   - (a) **record, don't tee:** persist digest-pass per-attach outcomes (read-ok/bytes/CRC-suspect) and have the probe consult them — 0074 already has a `stream_available` cache precedent; or
   - (b) literal single-pass, which forces probe work to run on attaches the two-pass baseline would never probe, changing budget accounting and the per-attach timeout story (an in-walk read cannot honor `deep_attach_max_probe_time_ms` the way the isolated worker thread does).
   Phase 0 should evaluate (a) vs (b) explicitly; DoD-1's "or documents an unavoidable exception with test" escape hatch is honest and should survive — with these findings, expect (a) to win.
3. **Budget precedence is unspecified.** Digest budgets (`strong_hash_attach_max_bytes`, `strong_hash_attach_per_attach_max_bytes`) and probe budgets (`deep_attach_max_probe_bytes`, time, count) differ. A unified pass must define which governs (suggest: stricter-of, with the loser's truncation semantics preserved) — the plan's "no double charge" test is good but needs this rule first.
4. **Sequencing is right:** 0090 changes what "the digest" is for embedded attaches; running 0091 first would freeze an API mid-refactor. Keep the soft dependency.
5. **Verification gate:** §8 omits the probe/digest unit surfaces (`attach_probe`, `attach_content_hash`) — add targeted test filters so the equivalence oracle actually runs in CI, not just the broad `-- unique` filter.

**Opportunity (reframe the win condition):** a successful full-stream digest *already proves* L3 readability. The best outcome is not "one read instead of two" but "**zero extra reads when the digest already read it**" — probe I/O deleted entirely on dual-enabled runs. Phase 0 should cost both framings; the second has strictly better operator math on multi-GB sets (the actual driver of D-0086-digest-probe-unify).

**Strengths:** equivalence-oracle mandate, single-feature isolation tests, explicit no-`--jobs`/no-buffer-redesign fences, "no perf theater" honesty note in the plan.
