# Review — 0091 DigestProbeUnify

- **Status:** **Completed**
- **Branch:** `feat/0091-digest-probe-unify`
- **Ledger TX:** `08b6a451-f2bd-48a7-acf5-ffd543f5074f` (FEATURE / `crates/pst-dedup-cli`)
- **Closes:** `D-0086-digest-probe-unify`

## Summary

When both `--deep-attach-preflight` and `--strong-content-hash body-recip-attach` are enabled, Pass-1 Real **by-value** full-stream digests seed a `ProbeResultCache` Full/ok entry (`charge_pending`). Pass-2 (`probe_scan_items` / `probe_keep_set_groups`) skips second-stream I/O while charging logical probe tallies once (`digest_stream_skips`). Embedded / Unread / DepthLimit / unsupported methods do not seed. Telemetry: `AttachProbePreflight.bytes_probed` + `digest_stream_skips`.

## DoD matrix

| DoD | Status |
|---|---|
| DoD-1 Unify | **Met** |
| DoD-2 Equivalence | **Met** (tallies, winners, recommendation, unique-pst exit) |
| DoD-3 Isolation | **Met** |
| DoD-4 Deferred closed | **Met** |
| DoD-5 Recorded | **Met** (this file + conductor + CHANGELOG + ledger commit) |

## Reviews

| Round | Verdict |
|---|---|
| Internal r1/r2 | PASS (after `bytes_left` charge cap) |
| Codex luna r1 | **FAIL** — method gate, zero-timeout, telemetry, unique-pst DoD-2, governance mid-cycle |
| Codex luna r2 | **FAIL** — DoD-2 winners/recommendation/exit proof incomplete |
| Codex luna r3 | **PASS** — no P0–P3 findings |

## Documented exception

Positive mid-stream `deep_attach_max_probe_time_ms` during a prior digest is not re-simulated on seeded hits (digest already proved Full readability). Entry-expired / `max_probe_time_ms == 0` is honored (spec §2.3).

## Gates (orchestrator)

- `cargo fmt --all --check` — PASS
- `cargo clippy -p pst-dedup-cli -p dedup-engine --all-targets -- -D warnings` — PASS
- `cargo test -p pst-dedup-cli --test digest_probe_unify_0091` — 7 passed
- `cargo test -p pst-dedup-cli --lib digest_probe` — PASS

## Residual / deferred

None for this track. No hard lows.
