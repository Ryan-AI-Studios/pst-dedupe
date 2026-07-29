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
| Offline Desk matter workflow (ingest → extract → reduce → review → produce) | Full remote feature parity (jobs/produce/FTS/AI over HTTP) |
| Desk **Connect** to matter-service (password + SSO loopback handoff; thin remote review) | LAN mTLS (**D-0058-02**); multi-matter host |
| Headless CLI matter automation (`pst-dedup matter` / `job` / …) | Bundled Tesseract / Whisper installers |
| Clean unique-PST CLI (`unique-pst`) + optional GUI wizard | “Outlook production-ready” claim without operator scanpst |
| Opt-in matter service / platform SSO / cloud CAS (documented, off by default) | FedRAMP / multi-cloud fleet |
| Solo produce **production profile** dropdown + required Bates start | Service-side produce HTTP API |
| CycloneDX SBOM (`bom.json`) in release ZIP | Clipboard bearer paste (banned; SSO uses loopback code) |

Full residual inventory: [`docs/deferred.md`](deferred.md). Freeze notes: [`docs/rc-freeze-inventory.md`](rc-freeze-inventory.md).

---

## Mode matrix (honesty table)

| Mode | Default? | Entry point | Notes / residual |
|---|---|---|---|
| Desk solo local matter | **Yes** | `dedupe-desk.exe` | Primary counsel UI |
| CLI headless matter | Yes | `pst-dedup.exe matter …` / `job …` | Agent / script path |
| Series K unique-pst | Yes (CLI) | `pst-dedup.exe unique-pst` | GUI wizard optional (`pst-dedup-gui.exe`) |
| Matter service multi-user | **Opt-in** | Host: `pst-dedup.exe service serve --matter <MATTER_DIR>`; clients: Desk **Connect** or HTTP API | Thin remote review (list/body/codes); jobs/produce remain host Solo/CLI |
| Platform SSO | **Opt-in** | Host with `--platform`; Desk **Sign in with SSO** (loopback handoff) | IdP redirect stays on service callback; no clipboard paste |
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
   - Choose a **production profile** (built-in or matter-local) or leave **Default (engine)**.
   - Set **Bates start** (required integer ≥ 1; job-time only — never stored in the profile).
   - Pre-flight blocks Start on unresolved/invalid profile, bad Bates start, or QC soft-gate.

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

**Counsel-grade lifecycle** (collection → handoff → disposition, exit codes, ScanPST-on-copy, basename custody):  
[`docs/unique-pst-ediscovery-runbook.md`](unique-pst-ediscovery-runbook.md).

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

Prefer a prior `inspect` + `scan --json` for multi-mailbox matters; optional timing harness:  
`scripts/unique-pst-timing.ps1` (parameterized inputs/out/report-dir; no client paths).

**Optional GUI:** `pst-dedup-gui.exe` → Unique PST Export wizard (same `run_unique_pst` path as CLI; track 0072).

**Honest limits:**

- Source PSTs are **read-only**.
- Multi-GB / multi-volume runs are supported on the streaming writer path; operator disk/time scale is residual (see checklist).
- Opening volumes in **Outlook** or running **scanpst.exe** is an **operator residual** — this RC does **not** claim Outlook production-ready without that smoke. ScanPST only on a **copy**.
- Exit **64** = partial fidelity with retained artifact (disclose); **do not blanket-retry exit 5**.

Deep docs: [`docs/unique-pst-export.md`](unique-pst-export.md) (flags), [`docs/unique-pst-ediscovery-runbook.md`](unique-pst-ediscovery-runbook.md) (ops narrative), writer fidelity notes under `docs/`.

---

## Path C — Multi-user host + Desk Connect (opt-in)

**Goal:** One host process owns the matter; operators review from Desk over loopback HTTP (track **0064**).

### Host

```powershell
# Multi-user matter + bootstrap (once)
.\pst-dedup.exe service bootstrap-admin --matter $Matter --name admin --password <pass>
.\pst-dedup.exe service serve --matter $Matter
# Default bind: http://127.0.0.1:7749
# Platform SSO (optional): add --platform <platform.db> and IdP config (see matter-service README)
```

**Do not** open the same matter for write in Solo Desk while the service holds the exclusive lock.

### Desk client

1. Start **dedupe-desk** with **no local matter open**.
2. Home → **Connect to matter-service…**
3. Base URL (default `http://127.0.0.1:7749`), username/password → **Connect**
   - Or **Sign in with SSO** when the host is in platform/OIDC mode (system browser + loopback one-time code; **no** clipboard bearer paste).
4. Banner shows `Connected to {url} as {name} ({role})`.
5. **Review (remote):** list items, load body, apply codes with OCC `expected_version`. On **409**, draft codes are retained — re-apply or discard (never silent wipe).
6. **Disconnect** returns to Solo. Local matter open is refused while Connected (and Connect is refused while a local matter is open).

| Connected supports (P0) | Solo / host only |
|---|---|
| List / body / codes (session actor + OCC) | Ingest, extract, reduce, produce, QC jobs |
| Password login; SSO loopback handoff | FTS/semantic index, AI, notes/privilege UI parity |
| `read_only` role disables mutates | Production profile produce dialog |

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
