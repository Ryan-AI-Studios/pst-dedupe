# 0081 — Unique Export Dep Pins & Operator Docs

> Series L closing track. Structure follows `C:\dev\coordinated\conductor\templates\0000-Description\spec.md`.
> Expanded subsections under §2–§3 are normative design for implementers. DoD is §7.

- **Track ID:** 0081-UniqueExportDepsAndOperatorDocs
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → Series L (unique export hardening) after 0073–0080
- **Cross-repo contract:** n/a
- **Status:** Completed — 2026-07-29
- **Depends on:** 0065 · 0066 · 0071 · 0073–0080 (all Completed on board)
- **Spec authored:** 2026-07-29
- **Revised 2026-07-29:** dual-AI review fold-in; Q1–Q7 locked to robust defaults; accidental mangled-path dirs removed; template-aligned

---

## 1. Objective

Close Series L with (1) a recorded **dependency pin audit** and safe bumps for the unique-export path, and (2) a counsel-grade **eDiscovery operator runbook** covering collection preconditions → process → handoff → disposition — without inventing new export semantics.

## 2. Context (read before starting)

### 2.1 Why this track exists

Tracks 0073–0080 shipped integrity, keep-set, attach ledger, deep attach, policies, CRC/export_risk, exit codes, perf timings, and output QC. Missing:

- A dated **pin audit** vs crates.io/lock with KEEP/PATCH/DECLINE decisions.
- A single **operator narrative** for multi-mailbox INC-style unique-PST (not only the flag encyclopedia in `docs/unique-pst-export.md`).
- Lifecycle bookends: **collection custody** before first scan and **disposition** after handoff.

### 2.2 Unique-export crates

| Crate | Role |
|---|---|
| `pst-reader` | Source open, CRC telemetry |
| `dedup-engine` | keep-set, materialize, report, unique-pst orchestration |
| `pst-writer` | Streaming Unicode production PST write |
| `pst-dedup-cli` | CLI: inspect / scan / keep-set / unique-pst |

### 2.3 Dependency snapshot (verified 2026-07-29 — re-query at implement)

| Dep | Lock | crates.io max | Intent |
|---|---|---|---|
| clap | 4.6.2 | 4.6.4 | **PATCH** |
| serde_json | 1.0.149 | 1.0.151 | **PATCH** |
| thiserror | 2.0.18 (+1.x dual) | 2.0.19 | **PATCH** 2.x |
| camino | 1.2.4 | 1.2.5 | **PATCH** |
| uuid | 1.23.1 | 1.24.0 | **MINOR** |
| sha2 | 0.11.0 (+0.10.9 dual) | 0.11.0 | **KEEP** product; **ACCEPT_DUAL** with invert notes |
| md-5 | 0.10.6 | 0.11.0 | **KEEP** 0.10 product pin (EDRM MIH) |
| rusqlite | 0.40.1 | 0.40.1 | **KEEP** |
| chrono / csv / crc32fast / rfd / tantivy / object_store | current | current | **KEEP** |
| eframe | 0.34.2 | 0.35.0 | **DECLINE_MAJOR** |
| reqwest | 0.12.28 (+0.13 dual) | 0.13.4 | **DECLINE_MAJOR** unless security override |
| aes-gcm | 0.10.3 | 0.11.0 | **DECLINE_MAJOR** unless security override |
| argon2 | 0.5.3 | 0.5.3 (0.6-rc) | **KEEP** |
| rand | 0.8.7 / 0.9.5 / 0.10.2 | 0.10.2 | **KEEP** all lines (RUSTSEC-2026-0097 floors already met) |

**RUSTSEC-2026-0097 (`rand`):** INFO Unsound when `log` + `thread_rng` + custom logger reseed. Patched ≥0.8.6 / ≥0.9.3 / ≥0.10.1. Lock is past floors — record **KEEP** with reasoning; do not force workspace major unify mid-RC.

**Dual-version sample invert:** `sha2@0.10.9` ← openidconnect/oauth2/ed25519-dalek/p256/p384 (SSO) + lopdf/pdf-extract. Audit must use `cargo tree -i <crate>@<ver>`.

**deny.toml:** live ignores RUSTSEC-2023-0071 (rsa), RUSTSEC-2026-0192 (ttf-parser). Dead `advisory-not-detected` ignores (0186, 0190, 0194, 0195) → **prune**.

### 2.4 Docs landscape

| Exists | Role |
|---|---|
| `docs/unique-pst-export.md` | Flag encyclopedia (canonical for flags) |
| `docs/operator-golden-path.md` | Day-1 RC; Path B is thin |
| `docs/deferred.md` | D-0073-basename, D-0077-repair-diff, D-0078-retryable, … |
| **Missing** | `docs/unique-pst-ediscovery-runbook.md` |

### 2.5 Locked product rules

1. **No drive-by majors mid-RC** except rule 2.
2. **Security override:** High/Critical RUSTSEC (or deny hard-fail) on the locked version where the **only** fix is a major → may take that major; record advisory ID + blast radius. INFO/unsound/unmaintained alone do not force majors.
3. **Unique-export semantics freeze** (keep-set, CRC accept, exit ints, fidelity, writer). Basename mode is presentation only.
4. **Sources read-only**; ScanPST **on a copy only**.
5. **One risk vocabulary:** `ok` \| `re_export_recommended` \| `not_export_ready`.
6. **No new exit integers** (0078 table).
7. **Never blanket-retry exit 5** in the runbook.
8. **Real case PSTs out of git.**
9. **Runbook narrative; flag encyclopedia stays canonical.**
10. **deny/audit green**; prune dead ignores; new ignores need a deferred D-row.
11. **Report-dir sensitive**; basename is **partial** path redaction only.
12. **Optional features default inert** (`ledger-path-mode` default `full`).
13. **Tool accepts any readable Unicode PST** (incl. Permute). Collection guidance **recommends** Purview for M365 — does **not** hard-block other sources.
14. **No product secure-wipe binary** — disposition is operator procedure.

### 2.6 Deferred roll-in

| ID | Disposition |
|---|---|
| **D-0073-basename** | **Ship** `--ledger-path-mode full\|basename` (locked decision Q1) + Matter Archive mapping mandate |
| **D-0077-repair-diff** | **Docs-close** via two-command `scan --json` count-diff; no `compare-counts` CLI |
| **D-0078-retryable** | **Constraint only** — runbook forbids blanket retry of exit 5; code residual remains |
| GUI / promote / eml-ledger / identity residuals | Out of scope |
| D-0062-codesign | Out of scope |

### 2.7 Locked decisions (Q1–Q7 → robust defaults)

| # | Decision | Choice | Rationale |
|---|---|---|---|
| **Q1** | Basename path mode | **Ship** | Industry handoff often strips absolute workstation paths; join via `source_id` + mandatory archive mapping is defensible |
| **Q2** | Timing script | **Ship** `scripts/unique-pst-timing.ps1` | Cheap operator evidence; complements `phase_timings` |
| **Q3** | Cite redacted INC numbers | **Yes**, as historical evidence **not SLA** | Grounds expectations without warranties |
| **Q4** | uuid 1.24 + camino 1.2.5 | **Yes** if tests green | Semver-compatible hygiene |
| **Q5** | md-5 product → 0.11 | **No** | Digest major for MIH-only; dual residual acceptable |
| **Q6** | Purview-only hard prose | **No** — preferred only | Matches product (any Unicode PST); industry: prefer defensible collection, don’t brick tools |
| **Q7** | Name firm wipe tool | **No** | Point to firm DLP/secure-delete policy; avoid tool-of-the-day coupling |

### 2.8 Hygiene already done (this prep)

Two empty accidental directories created by mangled path joins (not git content) were verified empty and **deleted**:

- `C:\dev\Dedupe\C…devdedupeconductortrack011-pst-writer-eml-import`
- `C:\dev\Dedupe\C…devdedupecratespst-writersrc`

Do not recreate. Implementer should confirm they remain absent.

### 2.9 Design — dependency audit (§ implements DoD-1,2,8)

**Commands:**

```powershell
cargo tree -p pst-dedup-cli --depth 1
cargo tree -p dedup-engine --depth 1
cargo tree -p pst-reader --depth 1
cargo tree -p pst-writer --depth 1
cargo tree -i sha2@0.10.9 --depth 3
cargo tree -i thiserror@1.0.69 --depth 3
cargo tree -i rand@0.8.7 --depth 2
cargo tree -i rand@0.9.5 --depth 2
cargo tree -i rand@0.10.2 --depth 2
cargo tree -i reqwest@0.13.4 --depth 3
cargo deny check
```

**Decision classes:** KEEP · PATCH · MINOR · DECLINE_MAJOR · TAKE_MAJOR_SECURITY · ACCEPT_DUAL · PRUNE_IGNORE.

**Audit table columns in `review.md`:**  
`crate | workspace pin | lock | crates.io max | decision | notes (invert / RUSTSEC)`

**deny.toml:** prune 0186/0190/0194/0195; keep live rsa + ttf-parser rows.

### 2.10 Design — operator runbook `docs/unique-pst-ediscovery-runbook.md`

**Audience:** counsel/ops. **Not:** flag dump (link `unique-pst-export.md`).

**Required sections (in order):**

0. **Collection & chain-of-custody** — Prefer Microsoft Purview eDiscovery exports for M365 collections; document risks of manual Outlook/legacy Exchange dumps (metadata, cloud attaches, audit continuity). Soft preference only (rule 13). Local disk; originals read-only; matter labels recorded before basename redaction.
1. **Honesty / ships-vs-not** — Sources read-only; no source repair. **Outlook re-verify at write time (DoD-11):** new Outlook can open/add `.pst` per current Microsoft Support (classic side-by-side same bitness often required); **no COM** still true; do **not** copy stale “import-only” claims without re-check; cite article + access date. `scanpst.exe` is classic Outlook.
2. **Golden flow** — inspect → scan --json → optional deep-attach → unique-pst → exit/report → QC/scanpst-on-copy → handoff → disposition.
3. **Policy cookbook** — multi-file `--source-rank`; Recoverable Items `--prefer-folder-class`; BCC flags; `earliest_date` caveats; `first_seen` path-order honesty.
4. **Integrity table (numeric thresholds)** — use product defaults, note configurability:

   | Signal | Default |
   |---|---|
   | `max_skip_rate` | **0.05** |
   | `max_crc_skip_rate` | **0.01** |
   | `max_attach_fail_rate` | **0.05** |
   | `block_crc_read_rate` escalate class | **≥ 0.15** (0077 / unique-pst-export) |
   | dual-rate poly heuristic | page ≥ **0.50** AND block ≥ **0.50** (0077) |
   | `export_risk` | `ok` \| `re_export_recommended` \| `not_export_ready` |

5. **Remediation** — Prefer Purview re-export; unindexed ≠ CRC; ScanPST copy-only + §2.11 count-diff.
6. **Exit codes** — 0 / 64 / 65 / 130 / 1 / 2 / 3–5; **no blanket retry exit 5**; PowerShell switch example.
7. **Artifact inventory & handoff** — report-dir file list; sensitivity; basename + Matter Archive mapping.
8. **Disposition & secure purge** — what accumulates; when (after hold release); Matter Archive retain vs workstation purge; firm secure-delete policy pointer; no product wipe claim.
9. **QC (0080)** short — sample default; scanpst env honesty; BYOB external reader; no COM.
10. **Timings** — `phase_timings`; optional INC historical numbers (not SLA); timing script.
11. **Links** — unique-pst-export, golden-path, deferred, fidelity, Microsoft citations with dates.

### 2.11 Design — ScanPST count-diff (closes D-0077-repair-diff)

No new CLI. Runbook must include:

```powershell
.\pst-dedup.exe scan C:\evidence\original.pst --json | Set-Content -Encoding utf8 before.json
# ScanPST on a COPY only
.\pst-dedup.exe scan C:\work\repaired-copy.pst --json | Set-Content -Encoding utf8 after.json
# Compare total_messages + per-folder counts; disclose any drop
```

### 2.12 Design — `--ledger-path-mode` (ships; Q1)

| Item | Spec |
|---|---|
| Flag | `--ledger-path-mode full\|basename` |
| Default | `full` |
| Applies to | path columns in **both** `export_messages.csv` and `export_attachments.csv` |
| Join key | `source_id` |
| Tests | full vs basename synthetic multi-path; basename non-empty when full had path |

**Honesty (runbook mandatory):** basename is not full de-identification (custodian filenames remain). Using basename **requires** preserving `source_id` → absolute path (and preferably source SHA-256) in a non-produced **Matter Archive**. Losing that mapping breaks origin proof.

### 2.13 Design — timing script (ships; Q2)

`scripts/unique-pst-timing.ps1` — parameterized inputs/out/report-dir; Measure-Command stages; optional `timing.json`; no hardcoded client paths; PowerShell-native.

### 2.14 Surprising constraints

- PowerShell: no bashisms (`&&`, `[[`, …).
- No `.unwrap()` / `expect()` in production Rust if code lands.
- Optional code must not change export exit semantics or keep-set winners.

## 3. In scope

1. Dependency audit write-up in track `review.md` (table + dual invert + rand KEEP + research date).
2. Safe pin bumps: PATCH clap/serde_json/thiserror/camino; MINOR uuid (if green); no undeclared majors.
3. `deny.toml` prune of dead advisory ignores.
4. New `docs/unique-pst-ediscovery-runbook.md` (§2.10 sections 0–11).
5. Links from `README.md`, `docs/operator-golden-path.md` Path B, `docs/unique-pst-export.md`.
6. Deferred hygiene in `docs/deferred.md` (basename closed; repair-diff closed docs; retryable constraint noted).
7. Ship `--ledger-path-mode` with tests + runbook custody text.
8. Ship `scripts/unique-pst-timing.ps1`.
9. Outlook client claims re-verified with citation date in runbook.
10. Confirm accidental mangled-path directories remain absent.
11. Conductor status flip + `review.md` on completion.

## 4. Out of scope (do NOT do here)

- Full workspace major upgrades without security override.
- Codesign (D-0062-codesign).
- New exit codes or risk vocabularies.
- Writer/reader fidelity features (recipient table, Mode A promote, etc.).
- New `compare-counts` CLI (declined).
- GUI wizard polish residuals.
- Committing client/case PSTs.
- Product secure-wipe feature.
- Hard-blocking non-Purview inputs.
- Re-opening Outlook COM automation (D-0080-com-declined).

## 5. Preconditions & dependencies

- **P1 (blocking):** 0071 unique-pst, 0077 export_risk, 0078 exit contract on `main` (board: Completed).
- **P2:** `deny.toml` present; cargo-deny available for DoD-8.
- **P3:** Multi-GB operator smoke **not** required for DoD.
- *Verified to date:* lock/crates.io snapshot §2.3 (2026-07-29); RUSTSEC-2026-0097 floors; dead deny ignores; IntegrityThresholds defaults 0.05/0.01/0.05; empty accident dirs deleted; Microsoft Support documents new Outlook PST open with classic side-by-side (re-check at runbook write).

## 6. Risks

| Risk | Mitigation |
|---|---|
| Patch bump breaks CLI/JSON | Targeted + workspace tests |
| Runbook duplicates flag encyclopedia | Narrative only; link flags |
| Basename treated as full redaction | Explicit honesty + Matter Archive mandate |
| Basename without mapping loses custody | Runbook DoD-12; default `full` |
| Exit 64 misread as hard fail | Bold + PowerShell switch |
| Mid-RC major creep | Rules 1–2; audit table |
| Stale Outlook claims | DoD-11 re-verify at write time |
| Collection over-promises Purview-only | Rule 13 preferred language |
| Disposition over-promises crypto erase | Rule 14 operator procedure only |
| Stale crates.io/RUSTSEC at implement | Re-query same day |

## 7. Definition of Done

Complete only when **ALL** hold:

- [ ] **DoD-1 — Dep audit:** `review.md` audit table with research date; decisions for §2.3 rows; dual invert notes; **rand KEEP + RUSTSEC-2026-0097**.
- [ ] **DoD-2 — Safe bumps:** Approved PATCH/MINOR applied or declined with reason; no undeclared majors; security majors only under rule 2 with advisory citation.
- [ ] **DoD-3 — Runbook:** `docs/unique-pst-ediscovery-runbook.md` covers §2.10 sections **0–11**.
- [ ] **DoD-4 — Links:** README, `operator-golden-path.md` Path B, `unique-pst-export.md` link the runbook.
- [ ] **DoD-5 — Deferred:** D-0077-repair-diff closed (docs); D-0078 constraint in runbook; D-0073-basename closed as shipped.
- [ ] **DoD-6 — Exit honesty:** No blanket “retry exit 5”; correct exit table + switch example.
- [ ] **DoD-7 — ScanPST:** Copy-only + two-command count-diff (§2.11).
- [ ] **DoD-8 — deny/audit:** `cargo deny check` green; dead ignores pruned; live D-0062 ignores retained or upgraded.
- [ ] **DoD-9 — Basename shipped:** `--ledger-path-mode` + tests; default `full`.
- [ ] **DoD-10 — Timing script:** `scripts/unique-pst-timing.ps1` present and parameterized (no client paths).
- [ ] **DoD-11 — Outlook re-verified:** Runbook cites current Microsoft docs with **access date**; accurate open/mount vs classic/COM claims.
- [ ] **DoD-12 — Basename custody:** Runbook mandates Matter Archive `source_id` → absolute path mapping; states basename ≠ full de-identification.
- [ ] **DoD-13 — Numeric thresholds:** Integrity table uses product defaults and notes configurability.
- [ ] **DoD-14 — Disposition:** Runbook §2.10.8 present without product wipe claim.
- [ ] **DoD-15 — Accident dirs:** Mangled-path accident folders remain absent from repo root.
- [ ] **DoD-16 — Tests:** `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace` green after code/lock changes.
- [ ] **DoD-17 — Recorded:** outcome in `review.md`; `../conductor.md` status **Completed**; ledger transaction committed in the execution repo (category `DOCS` and/or `INFRA`).

## 8. Verification commands (reference)

```powershell
# Dep surface + dual provenance
cargo tree -p pst-dedup-cli --depth 1
cargo tree -p dedup-engine --depth 1
cargo tree -p pst-reader --depth 1
cargo tree -p pst-writer --depth 1
cargo tree -i sha2@0.10.9 --depth 3
cargo deny check

# Accident-dir hygiene (should print nothing matching)
Get-ChildItem C:\dev\Dedupe -Directory | Where-Object { $_.Name -match 'devdedupe' }

# After lock bumps / basename code
cargo test -p dedup-engine
cargo test -p pst-reader
cargo test -p pst-writer
cargo test -p pst-dedup-cli

# Full gate
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
ledgerful verify
```

**Docs checks (manual):** §2.10 sections 0–11 present; numeric thresholds; no blanket retry exit 5; ScanPST on copy; Outlook dated; basename custody; disposition; links resolve.
