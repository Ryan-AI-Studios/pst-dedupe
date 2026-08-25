# Track Completion Audit — 0088-SovereignCloudHosts

## Verdict: PASS WITH DEFERRED P3

## Scope Reviewed

- Branch: `feat/0088-sovereign-cloud-hosts`
- Working tree, staged and unstaged changes
- Full `spec.md` and `plan.md`
- Engine, reader, CLI fidelity contract, report wiring, docs, deferred registry, and governance
- Prior Codex #1 findings and post-finalize disposition
- Read-only review; no files or Git state modified

## Requirement and DoD Matrix

| Requirement | Status | Evidence | Tests / verification | Gap |
|---|---|---|---|---|
| GCC High `*.sharepoint.us`, including `-my.` | Met | `body_cloud_links.rs:425-460` | Sovereign action-token tests; orchestrator reports 33 body-cloud tests passed | None |
| GCC High `admin.onedrive.us` | Met | Exact-host allowlist and bare-root exclusion | `admin_onedrive_us_host_allowed_but_needs_document_shape` | None |
| DoD `*.sharepoint-mil.us`, including `-my.` | Met | `body_cloud_links.rs:432-434` | DoD action-token tests | None |
| DoD `*.dps.mil` | Met | `body_cloud_links.rs:434` | Document-shaped and exclusion tests | None |
| Sovereign SafeLinks unwrap | Met | `body_cloud_links.rs:411-415` | `gcc_high_safelinks_unwrap_to_sharepoint_us` | None |
| Document-shaped gate and `:f:` exclusions | Met | `body_cloud_links.rs:462-513` | Bare-root / folder tests enumerate new suffixes | None |
| Reader suffix tightening | Met | `attachment.rs:86-179`, `215-241` | Lookalike rejection tests | Separate local table is intentional per spec |
| Production CSV/report wiring | Met | `unique_pst_cmd.rs:2387-2436`, `2680-2681`, `3256`; report writer paths | Existing report-path tests and supplied CLI test | None |
| Offline/no synthetic attachments | Met | Engine remains detection/ledger-only; fidelity remains `BestEffort` | Code and docs inspection | None |
| DoD-1 Hosts | Met | All required host classes implemented | Orchestrator-reported targeted tests passed | Not rerun by reviewer because test execution writes artifacts |
| DoD-2 Proportionality | Met | Bare roots and `:f:` excluded | Sovereign negative tests | None |
| DoD-3 Regression | Met, reported | Existing commercial tests retained | 33 body-cloud tests reported passed | Reported rather than independently rerun |
| DoD-4 Honesty | Met | `docs/deferred.md:857-858`; runbook/export docs | Only `D-0088-usgovcloud-microsoft-tld` remains | None |
| DoD-5 Recorded | Met, with ledger limitation | `review.md`, checked `plan.md`, active governance all Completed; canonical review records TX | Ledger commit is orchestrator-reported; live Ledgerful DB unavailable read-only | Not independently queryable here |

## Findings

### [P3] Future `.microsoft` sovereign content hosts remain intentionally unsupported

Confidence: High

Requirement: Spec §2.4 and DoD-4 explicitly require this residual to remain recorded rather than guessed.

Location: `docs/deferred.md:858` — `D-0088-usgovcloud-microsoft-tld`

Problem: `*.usgovcloud.microsoft`, `*.usgovcloud-static.microsoft`, and `*.usgovcloud-usercontent.microsoft` are not in the shipped allowlists.

Evidence: The residual is explicitly recorded, while the required documented US GCC High/DoD suffixes are implemented.

Failure scenario: Future or historical GCC High document links using these `.microsoft` hosts remain undetected.

Correction: Research exact path shapes and add dedicated fixtures in a future track.

Verification: Valid existing deferral; no speculative matching was added.

Deferrable: Yes

No P0–P2 findings were identified.

## Completeness Sweep

- No new production placeholders, stubs, fake values, no-op paths, or silent-success paths found.
- No production `.unwrap()` or `.expect()` introduced in the changed implementation paths; test-only `expect` usage remains.
- Commercial host behavior remains present.
- SafeLinks nested targets are revalidated against the document-shaped allowlist.
- Lookalike domains such as `notsharepoint.attacker.com` are rejected by the reader helper.
- No `pst-reader` → `dedup-engine` dependency was introduced.
- No network hydration or synthetic Attachment Table rows were added.
- Only one new deferred ID exists: `D-0088-usgovcloud-microsoft-tld`.

## Wiring and Regression Review

The body path remains reachable:

`message body → scan_body_cloud_links → PreparedWinner hits → body_cloud_link_count / export_body_cloud_links.csv`

The attachment path remains reachable:

`attachment properties → local suffix-safe reader helper → classify_attach_pc → cloud attachment classification`

The fidelity contract remains honest: cloud payloads are not marked `Preserved`, body-only hits do not trigger Mode A promotion, and the `.microsoft` residual is named.

Prior findings were dispositioned:

- DoD-5 finalization: closed in active `review.md`, `spec.md`, `plan.md`, `conductor.md`, `ROADMAP.md`, and `sequencing.md`.
- Dual host-table drift: synchronization comments are present in both tables; accepted by the explicit no-shared-crate design.
- `.microsoft` residual: remains the sole intentional hard P3.

Historical `implementation-notes.md` and prior review files retain pre-finalization “In Progress” wording. These are archival records and do not contradict the active governance state.

## Verification Evidence

### Observed now

- Correct branch and working tree state.
- Full spec and plan reviewed.
- `cargo fmt --all --check`: passed.
- `cargo metadata --no-deps --format-version 1`: passed.
- Active 0088 governance entries consistently show Completed.
- `git diff --check`: failed only on trailing whitespace in review/governance prose; style-only, not a completion finding.
- `ledgerful scan --impact`: produced the scan summary but could not persist its report under read-only enforcement.
- `ledgerful ledger status` / live verification: unavailable because the Ledgerful SQLite database could not be opened read-only.
- `ledgerful verify --dry-run`: displayed the expected fast plan; no commands executed.

### Reported by orchestrator

- `cargo test -p dedup-engine -- body_cloud`: 33 passed.
- `cargo test -p pst-reader -- attachment`: 9 passed.
- `cargo test -p pst-dedup-cli -- cloud_attachments`: 1 passed.
- Targeted clippy with `-D warnings`: passed.
- Full workspace gate and Ledgerful verification reportedly ran during finalization.

## Deferred Candidates

No new deferral is proposed.

The existing `D-0088-usgovcloud-microsoft-tld` is a valid, difficult, non-blocking P3 and is the sole remaining finding.

## Completion Decision

The implementation satisfies the engineering DoD and the finalize/governance requirements. The prior DoD-5 finding is closed, no regression or improper new deferral was found, and the only remaining issue is the intentional `.microsoft` residual.
