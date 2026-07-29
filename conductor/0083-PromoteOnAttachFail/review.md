# 0083-PromoteOnAttachFail — Completion Review

**Track:** 0083-PromoteOnAttachFail  
**Branch:** `feat/0083-promote-on-attach-fail`  
**Status:** **Completed**  
**Final cross-model gate:** Codex `gpt-5.6-luna` high — **PASS** (fresh re-review after fix round)  
**Internal review:** PASS WITH DEFERRED P3 → fixes absorbed; no blocking residuals  
**Ledger tx:** `57a52649-7274-489e-a3fe-e768cb4f7d49` (FEATURE)

## Summary

Shipped **Mode A pre-write promote-on-attach-fail** for unique export:

- CLI `--promote-on-attach-fail` (default **off**) on `unique-pst` and `unique-eml`
- Centralized `is_attach_incomplete` (stream unavailable or fail-severity attach fidelity; not body soft / parents_only omit / CRC noise / zero-byte success)
- Extended `finalize_with_materialize_opts` peer walk with three-way `decided_by`:
  - `promoted_after_attach_incomplete`
  - `promoted_after_materialize_fail` (hard path unchanged)
  - `mode_c_fallback_all_peers_incomplete` (highest-ranked materializable; not group drop)
- Soft skips stay `DupOf` (not `MaterializeFailed`); `duplicate_sources` full-group invariant
- Attach ledger `winner_promoted` + peer locus honesty
- Summary counters + cancel-summary flag echo
- **Mode B** permanently declined; cloud detector not invented; no least-incomplete re-rank
- Closes **D-0073-promote**; **D-0073-eml** full ledger CSV remains residual

## DoD matrix (engineering)

| DoD | Result | Evidence |
|---|---|---|
| 1 Flag | Met | unique-pst + unique-eml; default off; help/runbook |
| 2 Predicate | Met | `is_attach_incomplete` + table tests; optimistic listed attach |
| 3 Mode A promote | Met | unit + dual-PST QC proof |
| 4 Mode C default | Met | flag-off test |
| 5 All incomplete fallback | Met | all-soft + soft+hard post-loop fallback tests |
| 6 Hard promote | Met | preserved string + test |
| 7 Ledger honesty | Met | soft-skip rows + winner_promoted unit |
| 8 Exit honesty | Met | complete promote clears family attach fails; fallback partial |
| 9 Mode B absent | Met | declined docs + no rewrite path |
| 10 dup_sources | Met | multi-source after promote test |
| 11 Mode A × QC | Met | `unique_pst_mode_a_promote_qc_sample_keys_final_winner` |
| 12 Docs | Met | export.md, runbook §2a Sedona, deferred, CHANGELOG |
| 13 Deps | Met | no majors |
| 14 Gates | Met | fmt, clippy -D warnings, workspace test, deny |
| 15 Recorded | Met | this file; conductor Completed; ledger commit |

## Review loop

1. **Implement** subagent — core Mode A + docs  
2. **Internal review** FAIL — weak DoD-11 QC test  
3. **Fix** — dual-PST Mode A + QC final-winner proof; least-incomplete anti-regression  
4. **Internal re-review** PASS WITH DEFERRED P3  
5. **Codex luna high** FAIL — soft+hard fallback drop; zero-byte stream_available; cancel summary flag  
6. **Fix** — post-loop Mode C fallback; optimistic listed attach; CancelledSummaryCtx flag  
7. **Codex luna high fresh re-review** **PASS** (no findings) — `review.codex.md`

## Verification (orchestrator-observed)

```text
cargo fmt --all --check                          PASS
cargo clippy --workspace --all-targets -- -D warnings  PASS
cargo test --workspace                           PASS
cargo deny check                                 PASS
cargo test -p dedup-engine -- mode_a             7 passed
cargo test -p pst-dedup-cli --test unique_pst -- mode_a promote  PASS
```

## Deferred disposition

| ID | Disposition |
|---|---|
| **D-0073-promote** | **Closed / 0083** |
| Mode B write-time promote | Permanently declined (this track) |
| **D-0073-eml** | Residual (full attach ledger CSV); Mode A flag threaded |
| **D-0080-cloud-attachments** | Residual (no invent detector) |
| **D-0076-attach-content** | Residual (identity fracture note documented) |

## Dual-AI fold-in (spec)

Sedona cross-custodian de-duplication naming, `duplicate_sources` invariant, Mode A × QC final-winner keying, Mode C fallback `decided_by`, identity-tier / cloud honesty ceilings — all shipped.

## Next Series M candidates

Named props / cloud attach (D-0080-cloud-attachments), D-0076-attach-content, D-0079-deterministic-key, D-0073-eml full ledger.
