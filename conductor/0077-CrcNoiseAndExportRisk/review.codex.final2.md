FAIL

The two claimed P1 fixes are present, but the final sweep found one remaining P1 and one P2.

[P1] Final attachment CRC count is lost after the fidelity-event cap  
Confidence: High  
Location: `crates/pst-writer/src/production.rs:742-748`; `crates/pst-dedup-cli/src/unique_pst_cmd.rs:1867-1872`  
Problem: `ATTACH_STREAM_CRC` is counted only from the capped first-1000 event vector. If it occurs after 1000 prior events, `attach_stream_crc_events` remains zero and `export_risk` stays falsely lower.  
Correction: Add an exact per-kind stream-CRC counter or count through the uncapped sink; test CRC after the event cap.  
Deferrable: No

[P2] Poly reclassification does not consistently reach report rows  
Confidence: High  
Location: `crates/pst-dedup-cli/src/scan.rs:504`, `1062-1078`, `1671-1679`  
Problem: Normal CSV rows are streamed before the end-of-source poly decision, so they retain `CRC_SUSPECT`. Retained rows are also matched only by basename, allowing a poly source to clear a different source with the same PST filename.  
Correction: Defer/reconcile rows before writing, and match by full source identity/index.  
Deferrable: No

Prior P1 verification:

- Poly dual-rate handling now reclassifies by clearing taint; it no longer auto-allows suspect Tier-2 items. Evidence: `scan.rs:1107-1127`, `1592-1609`; Tier-2 override is explicit in `scan.rs:311-317`.
- Final stream CRC events emit `ATTACH_STREAM_CRC` and elevate risk when observed: `production.rs:2682-2688`, `unique_pst_cmd.rs:2180-2187`, `unique_export_report.rs:309-313`.

Requirement status: DoD 1–11, 14–15, 19–21, and 23 are implemented in the reviewed paths. DoD-12 is met for the tested clean/Aspose identity behavior, with the documented P3 poly-heuristic residual. DoD-13 and DoD-16 remain P3-level partials. DoD-22 is partial because capped final stream events can disappear from risk inputs.

Fresh sweep found no new placeholders or stubs in the changed production paths. `git diff --check` passed.

Verification evidence:

- Reported by orchestrator: fmt, clippy, and workspace tests green.
- Not independently rerun because this review is read-only.
- Ledgerful status/impact unavailable: database/report writes failed under the sandbox.
- `output\` remains populated, including a large generated export; cleanup is pending.
- Canonical `review.md` and conductor completion remain orchestrator-owned process residuals, as noted in the request.