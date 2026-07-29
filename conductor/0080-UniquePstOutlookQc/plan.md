# 0080 Plan — Unique PST Output QC

Spec: [`spec.md`](spec.md). Verified against `ce9cfc8`.

## Locks (do not violate without amending the spec)

1. Sources read-only (§2.3.1)
2. **QC never repairs** — no repair mode, no file, no flag (§2.3.2)
3. External validators run on a **local temp copy**, never the deliverable (§2.3.3)
4. A `.bak` beside the copy is a **hard error**, not a warning (§2.3.4)
5. No new exit integers — fold into 0078 `VERIFY_FAILED` (§2.3.5)
6. QC never lowers an exit another condition set (§2.3.6)
7. `known_gap` never fails; `defect` / `unexplained_loss` always do (§2.3.7)
8. External tiers are skip-safe, never required, never hang (§2.3.8)
9. **No copyleft in the binary** — libpff/libpst as processes only, zero Cargo deps (§2.3.9)
10. **No COM** — declined in §3.9 (no object model in new Outlook; mutates operator env; scanpst is strictly better)
11. Child processes are untrusted: timeout + kill + log-parse; exit codes are corroboration (§2.3.11)
12. Sampling is deterministic given the keep-set (§2.3.12)
13. Attestation is human-signed; the tool never self-attests (§2.3.13)
14. **Contract before default-on** — QC must not flip green pipelines red on a known gap (§2.3.14)
15. **Sidecars are BYOB** — never auto-download/vendor `pffinfo`/`readpst`; path only, never a URL (§2.3.15)

## Phase 0 — Contract first (before any new check)

- [ ] `fidelity_contract_v1` from `docs/pst-writer-fidelity-v1.md`: every property `preserved` / `best_effort` / `dropped_by_design` + reason
- [ ] Allowlist semantics: property absent from the contract ⇒ `unexplained_loss` (§3.2 last row)
- [ ] Contract versioning + the rule that `preserved` → `dropped_by_design` needs a bump and a `review.md` entry
- [ ] Enumerate today's gaps explicitly, **Q7 CC/BCC included** — this is what makes DoD-11 reachable
- [ ] **Q10 cloud/modern attachments**: explicit contract line documenting the blind spot (no named-prop resolution exists); D-0080-cloud-attachments recorded — DoD-22

## Phase 1 — Tier A: source-differential reader QC

- [ ] Promote `export_oracle::structural_digest_pst` out of test-only; **reuse, do not duplicate** (Q5/DoD-7)
- [ ] Folder **tree** comparison, not summed counts (Q3)
- [ ] **Attachment read-back + payload hash vs source** (Q2) — the check that has never existed
- [ ] Compare against **source**, not the run's own report (Q1); reuse 0079 `PstHandleCache`
- [ ] `source_differential: false` path for output-only `qc-pst`; cannot emit `defect` (§3.4)
- [ ] `skipped_source_unavailable` reported, never silently passed
- [ ] Classify every difference through the contract (§3.2)
- [ ] Persist `content_digests.json` at export time (granularity matches `--qc-level`); output-only `qc-pst` uses it for payload-level `defect`-capable re-verification, flagged `content_digest_backed: true`; absent ⇒ structural-only, never silently upgraded (§3.4, DoD-21)

## Phase 2 — Risk-weighted deterministic sampling

- [ ] All §3.3 strata: body extremes **(largest incl. XBLOCK boundary; smallest/0-byte ghost-message floor)**, attach extremes, longest subject (**D-0068-01**), degraded/ledger rows, non-ASCII, **volume seams**, **per source PST**, embedded
- [ ] `--qc-sample-max` default 64
- [ ] Test: two runs over one export select the **identical** set (lock 12)

## Phase 3 — Surfaces + artifact

- [ ] `--qc-level off|structure|sample|full` on `unique-pst`
- [ ] Standalone `pst-dedup qc-pst` — **not** `qc` (`main.rs:467` is matter-produce)
- [ ] `qc_report_v1` + `qc_findings.csv`; `qc_ms` into `PhaseTimings` (data path, not log path)
- [ ] Wire to `classify_export`: hard findings ⇒ `verify_ok = false` ⇒ `VERIFY_FAILED` ⇒ exit 1 (lock 5)

## Phase 4 — Negative tests (the ones that prove QC works)

- [ ] Corrupted / short-changed output PSTs built **at test time** via `pst-writer` + byte edits (0077 `crc_integrity_0077` pattern) — never derived from real files
- [ ] One negative per finding class: `defect`, `unexplained_loss`
- [ ] Assert `known_gap` alone leaves the exit unchanged (lock 7)
- [ ] Assert QC never lowers an exit set elsewhere (lock 6)

## Phase 5 — Tier B: independent reader sidecar

- [ ] `--qc-external-reader <path>` (`pffinfo` / `readpst`), ocr-plugin sidecar pattern
- [ ] **Counts only, never content** — a content diff becomes noise and gets the check disabled
- [ ] Skip-safe with reason; stub-executable test; licence note in `review.md` (lock 9)
- [ ] **BYOB**: path argument only, never a URL; tool never fetches/installs the binary (lock 15)

## Phase 6 — Tier C: scanpst

- [ ] Copy to **local** temp first (Microsoft: no networked `.pst`) — lock 3
- [ ] **Verify the `-no repair` token against the installed build; skip if unverifiable** (lock 2). Microsoft's page prints `-no repair`; unused/typo'd args silently fall back to the **repairing** legacy path — this is the one bug that damages a deliverable
- [ ] `-silent -log replace`; **parse the log**, not the exit code (lock 11)
- [ ] Hard timeout + kill ⇒ `SCANPST_TIMEOUT`; never block the export
- [ ] `.bak` present ⇒ hard error + quarantine the copy (lock 4)
- [ ] Path discovery (Click-to-Run roots + registry), never hardcode `Office16`
- [ ] Skip on builds < 16.0.10325.20082 (pre-1807 GUI-only would hang) and when only new Outlook is installed
- [ ] Stub-executable tests — CI never needs Outlook

## Phase 7 — CC/BCC (§3.11)

- [ ] `PidTagDisplayCc` on `WriteMessage` + adapter; contract `preserved`; version bump
- [ ] BCC declared `dropped_by_design` with the disclosure reason; **no auto-add** → D-0080-bcc-policy
- [ ] Start counting genuinely-unmappable fields via the adapter's reserved `dropped` return (`production.rs:819`)

## Phase 8 — Default-on gate

- [ ] Zero `defect` / zero `unexplained_loss` at `--qc-level full` across the whole fixture matrix
- [ ] **Only then** make `sample` the default (lock 14)
- [ ] A fixture defect that is really a known gap ⇒ fix the **contract**, never the exit mapping

## Phase 9 — Attestation + docs + registry

- [ ] `qc_attestation_v1` recordable; never self-attested (lock 13)
- [ ] `docs/unique-pst-export.md` client-retirement section (§3.8), **dated** — classic Outlook opt-out-default April 2026, retires Q1–Q2 2028, EOL Q2 2029; new Outlook = import-not-mount, no COM
- [ ] `deferred.md`: close **D-0068-02** / **D-0071-operator-outlook** / **D-0074-e2e-fixture**; narrow D-0068-01, D-0067-embedded-depth, D-0070-multi-source-stream-prefix; add D-0080-*
- [ ] `conductor.md` + `sequencing.md` rows
- [ ] `review.md`: operator scanpst result **or** explicit "absent, reason" — never a silent gap
- [ ] Full gate: `cargo fmt --all --check`; clippy `-D warnings`; `cargo test --workspace`

## Suggested order

**Phase 0 is not optional.** Without the contract, Phase 1 produces a red wall nobody reads.
Phases 1–4 are the track's actual value and are fully CI-testable. Phases 5–6 are
corroboration and can slip without blocking. Phase 8 gates the default flip.

## Handoff

**Do:** treat Tier A as the durable proof — every external validator here is on a retirement
clock or a copyleft licence. Keep the contract an allowlist. Make every skip carry a reason.

**Do not:** run repair on any file; validate the deliverable in place; add a Cargo dep on
libpff/libpst; add a new exit integer; let `known_gap` grow silently; claim Outlook
compatibility from a scanpst pass alone; ship default-on QC before the contract is complete.
