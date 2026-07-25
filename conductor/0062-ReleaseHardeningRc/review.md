# Track 0062-ReleaseHardeningRc — Review / Completion Record

**Status:** **Completed** (engineering DoD)  
**Verdict:** **PASS WITH DEFERRED P3** (Codex gpt-5.6-luna high final gate)  
**Date:** 2026-07-24  
**Product version:** `0.2.0-rc.1`  
**Schema pin:** `SCHEMA_VERSION` = **39**  
**Tag plan:** `v0.2.0-rc.1` after merge to `main`  
**Operator handoff:** **blocked** until Authenticode (**D-0062-codesign**)

---

## Scope

RC freeze / release hygiene only — **no product features**:

- Aligned workspace crate versions to `0.2.0-rc.1`
- `CHANGELOG.md`, golden path, mode matrix, freeze inventory, operator checklist, signing docs
- `deny.toml` (strict licenses; Windows graph; advisories/sources)
- `.cargo/audit.toml` + CI `audit` / `deny` jobs
- `[profile.release] debug = 1` + PDB packaging
- `scripts/package-release.ps1` → exes + multi-binary CycloneDX SBOMs + ZIPs + CLI/GUI smoke
- track011 archival Completed
- Deferred: D-0062-codesign, D-0062-audit-*

---

## Review rounds

| Round | Reviewer | Result |
|---|---|---|
| Internal #1 | general-purpose subagent | **FAIL** — golden-path CLI flags, PDB names, package/SBOM evidence |
| Internal #2 | general-purpose re-review | **PASS WITH DEFERRED P3** after flag/PDB/package fixes |
| Codex #1 | gpt-5.6-luna high | **FAIL** — multi-binary SBOM, ZIP, GUI smoke, README schema, service `--matter` |
| Codex #2 (final gate) | gpt-5.6-luna high | **PASS WITH DEFERRED P3** — no P0–P2 remain |

Raw: `review.codex.md`, `review.codex.round2.md` (local/gitignored under `conductor/`; force-add this canonical file).

---

## DoD matrix

| DoD | Result | Evidence |
|---|---|---|
| 1 Freeze inventory | Met | `docs/rc-freeze-inventory.md` |
| 2 Version + CHANGELOG + tag plan | Met | 35/35 crates `0.2.0-rc.1`; `CHANGELOG.md`; tag `v0.2.0-rc.1` |
| 3 Golden path | Met | `docs/operator-golden-path.md` (CLI flags match clap) |
| 4 Mode matrix | Met | Solo default; service/SSO/cloud opt-in; `service serve --matter` |
| 5 Schema pin | Met | docs + `matter_core::SCHEMA_VERSION = 39` |
| 6 Gates | Met | fmt, clippy `-D warnings`, test workspace, `cargo audit`, `cargo deny check` |
| 7 SBOM | Met | `bom.json` + `bom-cli` / `bom-desk` / `bom-gui` (Desk/GUI include eframe) |
| 8 Symbols | Met | `debug = 1`; underscored PDBs in `symbols/` + symbols ZIP |
| 9 Code signing | Met (policy) | `docs/release-signing.md`; handoff blocked via D-0062-codesign |
| 10 Artifacts | Met | three exes; operator ZIP + symbols ZIP; CLI + 3s Desk/GUI launch smoke |
| 11 Operator checklist | Met | `docs/operator-rc-checklist.md` |
| 12 Archival | Met | track011 **Completed** in conductor |
| 13 Recorded | Met | this `review.md`; deferred rows; unblocks **0063** |

---

## Gates (orchestrator-observed)

```text
cargo fmt --all --check                              PASS
cargo clippy --workspace --all-targets -- -D warnings PASS
cargo test --workspace                               PASS
cargo audit                                          PASS (warnings only; ignores in .cargo/audit.toml)
cargo deny check                                     PASS
scripts/package-release.ps1 -SkipBuild               PASS
  - 3 exes + 3 PDBs
  - bom-cli/desk/gui + bom.json
  - dedupe-0.2.0-rc.1-windows-x64.zip
  - dedupe-0.2.0-rc.1-windows-x64-symbols.zip
  - CLI help + Desk/GUI 3s launch smoke
```

---

## Deferred (validated P3 / handoff)

| ID | Notes |
|---|---|
| D-0062-codesign | Unsigned engineering ZIP OK; **counsel handoff blocked** until Authenticode |
| D-0062-audit-rsa | RUSTSEC-2023-0071 via openidconnect; SSO opt-in; no fixed upgrade |
| D-0062-audit-quickxml | Linux/wayland graph residual |
| D-0062-audit-warnings | ttf-parser unmaintained; anyhow/memmap2 unsound warnings |

---

## Handoff

- **0063** Security red team may start on frozen surface `0.2.0-rc.1` / schema v39.
- **0064** Desk Connect / SSO UX remains Proposed.
- Do **not** distribute unsigned ZIP as official operator RC.

---

## Disposition of Codex round-1 findings

| Finding | Disposition |
|---|---|
| P1 multi-binary SBOM | Fixed — per-binary cyclonedx + eframe assert |
| P1 review.md | Fixed — this file |
| P2 ZIP | Fixed — operator + symbols ZIP |
| P2 Desk/GUI smoke | Fixed — package script 3s launch |
| P2 README schema/CLI | Fixed — v39 + unique-pst/matter surface |
| P2 service `--matter` | Fixed — mode matrix |
