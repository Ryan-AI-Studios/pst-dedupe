# Operator golden paths (RC)

**Audience:** counsel / ops who need a day-1 run without reading the conductor board.  
**Version:** product RC **0.2.0-rc.1** (all crates aligned).  
**Schema pin:** matter store `SCHEMA_VERSION` = **39** (`matter_core::SCHEMA_VERSION`).  
Older matters open and migrate through the existing migration path in `matter-core`; do not hand-edit `matter.db`.

This page is the **single stand-alone** day-1 runbook. Deep docs are linked at the end.

---

## What ships vs what does not

| Ships in this RC | Does **not** ship (deferred / residual) |
|---|---|
| Offline Desk matter workflow (ingest → extract → reduce → review → produce) | Desk “Connect to service” multi-user UX (**0064**) |
| Headless CLI matter automation (`pst-dedup matter` / `job` / …) | Security red-team fix campaign (**0063**) |
| Clean unique-PST CLI (`unique-pst`) + optional GUI wizard | Bundled Tesseract / Whisper installers |
| Opt-in matter service / platform SSO / cloud CAS (documented, off by default) | “Outlook production-ready” claim without operator scanpst |
| CycloneDX SBOM (`bom.json`) in release ZIP | FedRAMP / multi-cloud fleet |

Full residual inventory: [`docs/deferred.md`](deferred.md). Freeze notes: [`docs/rc-freeze-inventory.md`](rc-freeze-inventory.md).

---

## Mode matrix (honesty table)

| Mode | Default? | Entry point | Notes / residual |
|---|---|---|---|
| Desk solo local matter | **Yes** | `dedupe-desk.exe` | Primary counsel UI |
| CLI headless matter | Yes | `pst-dedup.exe matter …` / `job …` | Agent / script path |
| Series K unique-pst | Yes (CLI) | `pst-dedup.exe unique-pst` | GUI wizard optional (`pst-dedup-gui.exe`) |
| Matter service multi-user | **Opt-in** | `pst-dedup.exe service serve --matter <MATTER_DIR>` | Desk Connect UX residual **0064** |
| Platform SSO | **Opt-in** | `service serve --matter <MATTER> --platform <platform.db>` + OIDC | Browser SSO UX residual **0064** |
| Cloud CAS / job backend | **Opt-in** | storage / job config (0061) | Admin UI residual; never required |

**Rule:** Offline Desk + CLI is the golden story. Service, SSO, and cloud backends are never required to complete a local matter.

---

## Path A — Offline matter (Desk)

**Goal:** Create a matter, ingest a package, extract, reduce, promote to review, code, produce.

1. **Install** the signed operator ZIP (see `README-RELEASE.txt` in the package). Engineering unsigned builds are for development only.
2. **Start Desk:** `dedupe-desk.exe`
3. **Create or open** a matter directory on local disk (not a network share for `matter.db`).
4. **Add source** (Purview ZIP / export package or PST path as supported by the UI).
5. **Ingest** → wait for job completion (progress in Desk).
6. **Extract** PST/Office/PDF stages as needed (or run a processing profile / workflow).
7. **Reduce:** dedupe → (optional) thread / near-dup / cull → **Promote to review**.
8. **Review:** open Review list; code; notes/privilege/redaction as needed.
9. **QC** (recommended) then **Produce** to a local output folder (Concordance DAT + natives + text).

**CLI equivalent (headless):**

```powershell
# Matter path is always --path (not --matter). Create once, then run jobs.
.\pst-dedup.exe matter create --path $Matter --name "day1"
.\pst-dedup.exe matter info --path $Matter

.\pst-dedup.exe job run --path $Matter --kind ingest --json
.\pst-dedup.exe job run --path $Matter --kind extract_pst --json
# Further kinds / profiles / workflows: .\pst-dedup.exe job --help , profile --help , workflow --help

# Produce requires Bates start; profile is optional (default pack applies when omitted).
.\pst-dedup.exe produce run --path $Matter --bates-start 1 --json
```

See also: crate READMEs under `crates/dedupe-desk`, `crates/matter-core`, `ARCHITECTURE.md`, and `.\pst-dedup.exe --help`.

---

## Path B — Clean unique PST (CLI)

**Goal:** From one or more source PSTs, write keep-set unique messages into production-shaped Unicode PST volume(s) plus a report pack.

```powershell
# --out is the primary volume path (not --out-dir). Report pack defaults beside the stem.
.\pst-dedup.exe unique-pst a.pst b.pst `
  --out output\unique.pst `
  --report-dir output\unique_report `
  --policy first_seen

# Inspect report pack (unique_export_report_v1) under --report-dir
# Optional content rehash after write:
.\pst-dedup.exe unique-pst a.pst b.pst `
  --out output\unique.pst `
  --overwrite `
  --verify-hash
```

**Optional GUI:** `pst-dedup-gui.exe` → Unique PST Export wizard (same `run_unique_pst` path as CLI; track 0072).

**Honest limits:**

- Source PSTs are **read-only**.
- Multi-GB / multi-volume runs are supported on the streaming writer path; operator disk/time scale is residual (see checklist).
- Opening volumes in **Outlook** or running **scanpst.exe** is an **operator residual** — this RC does **not** claim Outlook production-ready without that smoke.

Deep docs: [`docs/unique-pst-export.md`](unique-pst-export.md), writer fidelity notes under `docs/`.

---

## External tools (optional, not bundled)

| Tool | Used for | Install |
|---|---|---|
| Tesseract OCR | `ocr` job on image/PDF-needing-OCR items | Operator installs; path in Settings |
| Whisper / STT CLI | `transcribe` job | Operator installs; see stt-plugin docs |
| Outlook / scanpst | Operator structural smoke of unique-pst volumes | Windows / Office residual |

---

## Schema pin

| Item | Value |
|---|---|
| Constant | `matter_core::SCHEMA_VERSION` |
| RC pin | **39** |
| Migrations | Applied automatically on matter open/create |

---

## Related docs

| Doc | Purpose |
|---|---|
| [`docs/operator-rc-checklist.md`](operator-rc-checklist.md) | Operator smoke checklist (scanpst, multi-GB, fixture) |
| [`docs/rc-freeze-inventory.md`](rc-freeze-inventory.md) | What ships vs deferred |
| [`docs/release-signing.md`](release-signing.md) | Authenticode / handoff policy |
| [`CHANGELOG.md`](../CHANGELOG.md) | RC notes |
| [`README.md`](../README.md) | Build and CLI surface |
| [`ARCHITECTURE.md`](../ARCHITECTURE.md) | Design map |
| [`docs/deferred.md`](deferred.md) | Full residual ledger |
