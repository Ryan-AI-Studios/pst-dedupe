# 0091 — Attach Digest + Probe Unify

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.

- **Track ID:** 0091-DigestProbeUnify
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → Series M continuation
- **Cross-repo contract:** n/a
- **Status:** Completed — internal r1 (2026-08-25); Codex `review.md` pending orchestrator
- **Depends on:** 0074 · 0086 · **0090 Completed preferred** (digest API stable)
- **Spec authored:** 2026-08-24
- **Series:** M (Unique export fidelity residuals — continuation)
>
> **Review fold-in (2026-08-24):** dual-AI Ready review (`opencode-review.md` + `agy-review.md`) incorporated below.
> Disposition of each claim is in §2.6. Snapshot correction: both consumers live in `pst-dedup-cli`, not `dedup-engine`.

---

## 1. Objective

When both **0074 Full (L3) deep attach preflight** and **0086 attach-content identity digest** are enabled on the same run, **do not re-stream** attachments the digest pass already fully read. Preferred win: **zero extra attach reads on the dual-enabled path** (probe I/O deleted for attaches already digested), with keep-set / probe outcomes equivalent to the two-pass baseline.

**Closes:** `D-0086-digest-probe-unify`.

---

## 2. Context (read before starting)

### 2.1 Why this track exists now

| Deferred / ceiling | Severity | Claim |
|---|---|---|
| **D-0086-digest-probe-unify** | P3 | Identity digest and L3 probe are separate passes → double I/O when both on |
| Operator scale | — | Multi-GB unique-pst with deep preflight + `body-recip-attach` pays 2× attach stream cost |

### 2.2 Live code snapshot (verified 2026-08-24) — **corrected**

| Surface | State |
|---|---|
| L2/L3 probe | `crates/pst-dedup-cli/src/attach_probe.rs` (`probe_scan_items`). `dedup-engine/src/integrity.rs` holds reason codes/tallies, **not** the streamer |
| Attach SHA-256 digest | `crates/pst-dedup-cli/src/attach_content_hash.rs` + `scan.rs` (~802–863) during **message walk, before grouping** |
| Probe phase | **After** the walk, over candidates / keep-set peers (`scan.rs` ~1350+), worker-thread + `recv_timeout`, own budgets, peer caps |
| CLI flags | `--deep-attach-preflight` and `--strong-content-hash body-recip-attach` independently |

A `dedup-engine`-private tee **cannot** unify these without moving code. Unify in **CLI scan pipeline**.

### 2.3 Product locks

1. **Shape: record, don't tee (default).** Persist digest-pass per-attach outcomes (read-ok / bytes / CRC-suspect / digest). Probe pass **consults and skips** already-full-streamed attaches (0074 `stream_available` cache precedent). Literal single-walk tee is **declined as default** because it would probe attaches the two-pass baseline never probes and cannot honor `deep_attach_max_probe_time_ms` the same way.
2. **Win condition:** **zero extra reads** when the digest already proved L3 readability — not “one tee instead of two.”
3. **Equivalence:** Dual-enabled unified path must match two-pass results for keep-set winners, probe reason codes, preflight recommendation, and exit (within documented budget accounting). **Timeout exception (DoD-1):** entry-expired / `max_probe_time_ms == 0` is honored on seeded hits; positive mid-stream wall-clock during a prior digest is not re-simulated (digest already proved Full readability).
4. **Single-feature paths unchanged.**
5. **Budget precedence:** apply **stricter-of** digest vs probe byte/count caps; preserve the *loser* truncation semantics (document in Phase 0 table). Telemetry: `strong_hash_attach_bytes` and `attach_probe.bytes_probed` / `digest_stream_skips` may both reflect the **same** physical bytes **without double-charging wall-clock / physical I/O counters**.
6. **No `--jobs`.** No `PstFile` BufReader redesign (`D-0079-reader-buffer`).
7. Synthetic fixtures; optional operator multi-GB note only.
8. Soft-dep: **after 0090** so embedded digest results are the API being cached.

### 2.4 Isolation matrix

| Flags | Behavior |
|---|---|
| Deep preflight only | Unchanged Pass 2 L2/L3 + peer caps |
| `body-recip-attach` only | Unchanged Pass 1 digest |
| **Both** | Pass 1 digest records probe-useful outcomes; Pass 2 **skips** those attaches |

### 2.5 Dual-AI review disposition (2026-08-24)

| # | Claim | Source | Disposition | Spec landing |
|---|---|---|---|---|
| O1 | Snapshot wrong; both consumers in CLI | opencode | **Agree** | §2.2 |
| O2 | Phase mismatch; prefer record-don’t-tee over literal tee | opencode | **Agree** | lock 1–2 |
| O3 | Budget precedence unspecified | opencode | **Agree** | lock 5 |
| O4 | Keep 0090-before-0091 | opencode | **Agree** | Depends on |
| O5 | Targeted `attach_probe` / `attach_content_hash` tests in §8 | opencode | **Agree** | §8 |
| O6 | Reframe win as zero extra reads | opencode | **Agree** | §1 |
| A1 | Pass 1 populate probe stats; Pass 2 skip | agy | **Agree** | §2.4 (same as O2) |
| A2 | Equivalence: winners, preflight, exit, probe tallies | agy | **Agree** | DoD-2 |
| A3 | Dual telemetry without double-charging physical I/O | agy | **Agree** | lock 5 |
| A4 | DoD: prove `open_attachment_data` call count halved | agy | **Partial** | Prove **skip / no second stream** via counters; raw call-count hooks are brittle — not a required spy |

---

## 3. In scope

1. Digest-pass outcome cache consumed by probe pass when both flags set.
2. Budget precedence table + honest telemetry.
3. Equivalence tests vs sequential baseline; single-feature isolation tests.
4. Close `D-0086-digest-probe-unify`.

## 4. Out of scope

- Shipping `--jobs` / parallel materialize.
- `PstFile` BufReader redesign.
- Changing default scan flags.
- Embedded-msg recursive hash design (**0090**).
- Moving probe/digest into `dedup-engine` unless a tiny shared type is clearly cheaper (default: stay in CLI).

## 5. Preconditions & dependencies

- **P1:** 0074 + 0086 Completed.
- **Soft:** 0090 Completed.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Probe/digest divergence | Equivalence oracle mandatory |
| Budget double-charge | Stricter-of + telemetry honesty |
| Perf theater | If dual path does not drop second stream, residual honestly — do not ship |

## 7. Definition of Done

- [x] **DoD-1 — Unify:** Dual-enabled path does **not** re-stream attaches already fully digested (zero extra reads on that set). Document any unavoidable exception with a test.
- [x] **DoD-2 — Equivalence:** Fixture `keep_set.winners`, preflight recommendation, exit_code, and attach-probe tallies match two-pass baseline.
- [x] **DoD-3 — Isolation:** Deep-preflight-only and body-recip-attach-only unchanged.
- [x] **DoD-4 — Deferred:** `D-0086-digest-probe-unify` closed.
- [ ] **DoD-5 — Recorded:** `review.md`; conductor **Completed**; ledger TX committed. *(conductor/plan/CHANGELOG + review.internal.r1 done; Codex review.md + ledger commit pending)*

## 8. Verification commands

```powershell
cargo test -p pst-dedup-cli -- attach_probe
cargo test -p pst-dedup-cli -- attach_content_hash
cargo test -p pst-dedup-cli -- unique
cargo test -p dedup-engine
cargo fmt --all --check
cargo clippy -p pst-dedup-cli -p dedup-engine --all-targets -- -D warnings
ledgerful verify
```
