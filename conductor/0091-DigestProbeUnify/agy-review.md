# Antigravity Review — Track 0091: Attach Digest + Probe Unify

- **Track ID:** `0091-DigestProbeUnify`
- **Reviewer:** Antigravity (Advanced Agentic Pair Programmer)
- **Date:** 2026-08-24
- **Review Scope:** Review only (no implementation) — plan audit, pipeline phase analysis, I/O efficiency, and equivalence invariants.
- **Spec / Plan Reference:** [`spec.md`](file:///C:/dev/Dedupe/conductor/0091-DigestProbeUnify/spec.md), [`plan.md`](file:///C:/dev/Dedupe/conductor/0091-DigestProbeUnify/plan.md)

---

## 1. Executive Summary

Track 0091 optimizes multi-GB scan performance by unifying the 0074 L3 deep attach probe (`--deep-attach-preflight`) and the 0086 attach-content identity digest (`--strong-content-hash body-recip-attach`) into a single streaming read per attachment when both are enabled. This closes `D-0086-digest-probe-unify`.

Currently, when both features are enabled, attachments are streamed twice: first during candidate extraction for identity hashing, and second during post-scan integrity probing.

This review identifies the **pipeline phase coupling** in `scan.rs`, the **exact equivalence requirements**, and **budget accounting rules** necessary to prevent regression.

---

## 2. Blind Spots & Technical Findings

### Finding 0091-1: Phase Ordering & Pipeline Coupling in `scan.rs`
- **Live Code Architecture:**
  - **Pass 1 (Candidate Collection, lines 790–870):** Iterates over folders and messages. When `identity.includes_attach_content()` is true, calls `hash_attachment_stream` to compute per-attach SHA-256 digests so `compute_dedup_keys_ex` can generate the strong content hash.
  - **Pass 2 (Attach Probe Pass, lines 1370–1560):** Executes *after* all candidates are collected. It loops through candidates and calls `probe_scan_items` to evaluate L2/L3 stream integrity, applying peer group caps (`max_peer_probes_per_group`).
- **The Challenge:** Pass 1 runs *before* keep-set grouping is resolved, while Pass 2 was designed to leverage group knowledge for peer-capping.
- **Solution / Unified Path Architecture:**
  - When `body-recip-attach` is enabled, *every* identity-relevant attachment must be digested regardless of peer status (because the digest is required to determine grouping in the first place).
  - Therefore, streaming the attachment during Pass 1 can **simultaneously** record the probe result (checking `reader.crc_suspect()`, `bytes_read == declared_size`, and streaming I/O errors).
  - The unified pass in Pass 1 can directly populate both the attachment's `content_sha256` AND the candidate's `fidelity.degraded_reasons` / attach probe statistics.
  - Pass 2 can then completely skip any attachment that was already full-stream probed/digested during Pass 1!

### Finding 0091-2: Strict Equivalence Invariant & Single-Feature Isolation
- **Isolation Rule:**
  - If `--deep-attach-preflight` alone is set: remains L2 (1 MiB head read) by default, running in Pass 2 with peer-capping.
  - If `--strong-content-hash body-recip-attach` alone is set: full-stream digest in Pass 1.
  - If **both** are set: full-stream digest in Pass 1 satisfies L3 probe criteria in a single pass.
- **Equivalence Oracle:**
  - Automated tests must run identical synthetic PST fixtures through:
    1. Sequential baseline (Pass 1 hash + Pass 2 L3 probe).
    2. Unified path.
  - Assert that `keep_set.winners`, `summary.preflight.recommendation`, `exit_code`, and `summary.preflight.attach_probe` outcome tallies match 100%.

### Finding 0091-3: Metrics and Budget Telemetry Honesty
- **Telemetry Accounting:**
  - When the unified path streams 500 MB of attachments once, `summary.json` must not record 1 GB of I/O, but it must honestly report that 500 MB was digested and 500 MB was probed.
  - Both `strong_hash_attach_bytes` and `attach_probe.bytes_probed` should accurately reflect the single unified I/O stream without double-charging the wall-clock or I/O counters.

---

## 3. Recommended Spec & Plan Amendments

1. **Update Plan §Phase 0:** Clarify that unification takes place by having Pass 1 candidate extraction attach-hashing populate probe outcome data, allowing Pass 2 to skip re-probing.
2. **Update §7 Definition of Done (DoD-1 & DoD-2):**
   - Mandate an automated test proving that the number of physical `open_attachment_data` calls is halved when both features are active on a fixture.
   - Assert exact equivalence between unified and legacy two-pass execution.

---

## 4. Verdict & Risk Rating

- **Track Rating:** **PASS (Ready with pipeline phase architecture locked)**
- **Complexity / Risk:** Low-Medium (internal control flow refactoring in `pst-dedup-cli::scan`).
- **Execution Estimate:** 1 – 1.5 days (recommend running after 0090).
