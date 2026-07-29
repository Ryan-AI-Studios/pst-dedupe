# Unique-PST eDiscovery operator runbook

**Audience:** counsel and litigation-support ops running multi-mailbox clean unique PST export.  
**Scope:** narrative lifecycle only — collection → process → handoff → disposition.  
**Canonical flag encyclopedia:** [`unique-pst-export.md`](unique-pst-export.md) (do not treat this page as a flag dump).  
**Track:** 0081 (Series L closer). Product RC **0.2.0-rc.1**.

---

## 0. Collection & chain-of-custody

**Prefer Microsoft Purview eDiscovery** (or equivalent enterprise export) for Microsoft 365 collections. Purview-style exports tend to preserve audit continuity, cloud-attachment semantics, and defensible collection metadata better than ad-hoc Outlook desktop dumps or legacy Exchange mailbox exports.

This product **accepts any readable Unicode PST** (including Permute-encrypted stores when the reader path supports them). Collection guidance is a **soft preference**, not a hard product gate — non-Purview sources are not blocked.

Before first scan:

| Step | Practice |
|---|---|
| Local disk | Stage sources on local disk for the matter workset (avoid network-share `matter.db` / thrashing). |
| Read-only originals | Treat source PSTs as **read-only evidence**. Never run ScanPST or repair tools on the original. |
| Matter labels | Record matter ID, custodian, collection date, and source SHA-256 **before** any basename path redaction. |
| Chain file | Keep a non-produced **Matter Archive** (see §7) with absolute paths and digests. |

Risks of manual Outlook / legacy Exchange dumps (disclose when used): incomplete cloud attachments, altered metadata, weaker audit continuity, and operator-local path leakage into report packs.

---

## 1. Honesty / ships-vs-not

| Ships | Does **not** ship |
|---|---|
| Read-only multi-input unique-PST export + report pack | Source mutation or “repair in place” |
| Keep-set policies, attach ledger, CRC/export_risk, exit codes 0/64/65/130 | New exit integers beyond the 0078 table |
| Optional QC (`--qc-level`), BYOB external reader, optional scanpst on a **copy** | COM automation against Outlook |
| Streaming Unicode production PST writer | Product secure-wipe binary |
| Basename path mode for handoff CSVs (default off / `full`) | Full de-identification of custodian names |
| MS-PST recipient tables on unique-PST (**0082**); BCC opt-in write | Distribution-list / GAL expansion |
| `retryable` boolean on summary JSON (**0082**) | Blanket retry of exit **5** without classification |

**Outlook clients (re-verified 2026-07-29):**

- **New Outlook** can **open/add** Outlook Data Files (`.pst`): Settings → Files → Outlook Data Files → Add file. Classic Outlook must also be installed; both versions must share the same bitness (32-bit or 64-bit).  
  Citations (access date **2026-07-29**):  
  - [Open and find items in an Outlook Data File (.pst)](https://support.microsoft.com/en-us/outlook/open-and-find-items-in-an-outlook-data-file-pst)  
  - [Open and close Outlook Data Files (.pst)](https://support.microsoft.com/en-us/outlook/open-and-close-outlook-data-files-pst)
- **Import-only** claims are **stale** for new Outlook email browse/open; calendar/contacts and some import paths still differ — re-check Microsoft Support before stating import limitations.
- **This product does not use COM** to drive Outlook.
- **`scanpst.exe`** is a **classic Outlook** inbox-repair tool. Use it only on a **working copy**, never on originals (see §5).

---

## 2. Golden flow

Recommended counsel-grade sequence:

1. **`inspect`** — confirm Unicode PST, rough folder/message counts.  
2. **`scan --json`** — integrity + CRC telemetry; capture preflight recommendation.  
3. **Optional deep-attach** — `--deep-attach-preflight` when attachment fidelity is matter-critical.  
4. **`unique-pst`** — keep-set resolve → streaming write → report pack.  
5. **Exit / report** — read process exit + `summary.json` (`fidelity`, `export_risk`, `exit_reason`, `phase_timings`).  
6. **QC / ScanPST-on-copy** — default `--qc-level sample`; optional `--qc-scanpst` / external reader; ScanPST only on a copy (§5).  
7. **Handoff** — unique PST volume(s) + report pack subset; apply basename only if Matter Archive mapping is retained (§7).  
8. **Disposition** — after hold release, purge workstation intermediates per firm policy (§8).

Day-1 CLI sketch:

```powershell
.\pst-dedup.exe inspect C:\evidence\custA.pst --top 20
.\pst-dedup.exe scan C:\evidence\custA.pst C:\evidence\custB.pst --json | Set-Content -Encoding utf8 scan.json
.\pst-dedup.exe unique-pst C:\evidence\custA.pst C:\evidence\custB.pst `
  --out C:\work\unique.pst `
  --report-dir C:\work\unique_report `
  --policy first_seen `
  --overwrite `
  --json
```

Optional timing harness (no client paths baked in): [`scripts/unique-pst-timing.ps1`](../scripts/unique-pst-timing.ps1).

---

## 3. Policy cookbook

| Situation | Guidance |
|---|---|
| Multi-file / multi-custodian | Prefer `--source-rank` (best-first) so keep-set winners favor the intended collection tier. |
| Recoverable Items / dumpster-like folders | `--prefer-folder-class` (and/or custom `--folder-rank`) when purging soft-deleted noise is matter policy. |
| Sender-copy completeness | `--prefer-bcc-copy` when BCC-bearing copies are preferred for **keep-set winner choice**. |
| BCC on the **deliverable** | Default **suppresses** Bcc TC rows / `PidTagDisplayBcc` (disclosure). Use `--include-bcc-recipients` only under counsel instruction when full-fidelity BCC must appear in the written PST. |
| Chronological winner | `--policy earliest_date` prefers earliest submit time (delivery fallback); missing dates rank last — disclose when used. |
| `first_seen` honesty | Default `first_seen` is **sorted input-path order**, not chronological send time. |
| Per-source isolation | `--dedupe-scope per-source` when cross-mailbox collapse is not authorized. |

Full flag table: [`unique-pst-export.md`](unique-pst-export.md).

### BCC disclosure & near-duplicates (0082)

Unique-PST consolidates custodians. Writing every BCC into a multi-mailbox deliverable can over-disclose relative to a single custodian's outward view — same class as visible-only defaults elsewhere.

| Path | Behavior |
|---|---|
| **Write (default)** | To + Cc only; no Bcc rows / no `PidTagDisplayBcc` |
| **Write (opt-in)** | `--include-bcc-recipients` writes Bcc when the source had them |
| **Identity (Tier-2.5)** | To+Cc+**Bcc** still participate in the content hash when a recipient table is present — so copies that differ only by BCC do **not** false-merge |
| **Ledger** | `export_messages.csv` → `bcc_suppressed`; summary → `bcc_suppressed_message_count` |

**Reviewer note:** two near-identical messages in the unique-PST with `bcc_suppressed=true` are **not** a dedupe failure. BCC variance was retained for identity and omitted from the deliverable by policy. Disclose the suppress count to counsel when handoff includes the report pack.

### No distribution-list expansion

Fidelity is to the **PST file**, not live Exchange group membership. If the source stored only a DL display name or EX address without expanded members, the unique-PST **replicates that** and does **not** resolve membership against a GAL. Do not claim expanded To/Cc from this product alone.

### Zero-recipient anomaly (telemetry)

| Condition | Product response |
|---|---|
| Empty recipient TC **and** `MSGFLAG_UNSENT` **not** set | Count `sent_message_with_no_recipients_count` (anomaly telemetry) |
| Empty TC **and** UNSENT set (draft) | Normal — no anomaly |
| Flags unreadable | Skip anomaly (do not invent UNSENT) |

This is **telemetry only** — it does **not** hard-fail the export and does **not** invent a new `export_risk` value (`ok` \| `re_export_recommended` \| `not_export_ready` remains frozen).

---

## 4. Integrity table (defaults)

Product defaults (CLI-configurable vs fixed product constants — do **not** assume every row is a flag):

| Signal | Default | Configurability |
|---|---|---|
| `max_skip_rate` | **0.05** | **CLI** `--max-skip-rate` |
| `max_crc_skip_rate` | **0.01** | **CLI** `--max-crc-skip-rate` |
| `max_attach_fail_rate` | **0.05** | **CLI** `--max-attach-fail-rate` |
| `block_crc_read_rate` escalate class | **≥ 0.15** | **Fixed product constant** (0077 export-risk policy; not a CLI flag) |
| Dual-rate poly heuristic | page ≥ **0.50** **AND** block ≥ **0.50** | **Fixed product constant** (0077 `CRC_SUSPECT` poly heuristic; not a CLI flag) |
| `export_risk` vocabulary | `ok` \| `re_export_recommended` \| `not_export_ready` | **One** fixed vocabulary (optional `--fail-on-export-risk` gates exit **65**) |

`export_risk` is computed post-export and **never lowers** scan preflight risk. Optional `--fail-on-export-risk` can turn advisory risk into exit **65**.

---

## 5. Remediation

1. **Prefer Purview (or enterprise) re-export** when integrity/`export_risk` says `re_export_recommended` or `not_export_ready` and the collection path is M365.  
2. **Unindexed items ≠ CRC failure** — do not equate search-index gaps with block CRC corruption.  
3. **ScanPST on a copy only** — never on originals. After repair, compare message counts with the two-command workflow:

```powershell
.\pst-dedup.exe scan C:\evidence\original.pst --json | Set-Content -Encoding utf8 before.json
# ScanPST on a COPY only (classic Outlook scanpst.exe)
.\pst-dedup.exe scan C:\work\repaired-copy.pst --json | Set-Content -Encoding utf8 after.json
# Compare total_messages + per-folder counts; disclose any drop to counsel
```

There is **no** `compare-counts` CLI — the JSON pair above is the supported count-diff procedure.

---

## 6. Exit codes

Stable unique-export / matter automation codes (0078 table; **no new integers**):

| Code | Meaning | Operator action |
|---|---|---|
| **0** | Success (complete fidelity) | Proceed to QC / handoff |
| **64** | Partial fidelity — **message-complete artifact retained** | Disclose attach/body soft-fails; **do not delete** the PST solely for 64 |
| **65** | Export risk gate (`--fail-on-export-risk`) | Review `export_risk`; prefer re-export before relying on artifact |
| **130** | Operator cancel (SIGINT convention) | Truncated volume quarantined to `.partial`; re-run when ready |
| **1** | Generic / hard fail | Artifact absent or untrustworthy — investigate logs |
| **2** | Usage / validation | Fix args / paths |
| **3** | Matter busy | Wait / other job |
| **4** | Job failed or cancelled (matter job path) | Inspect job status |
| **5** | Matter open/create/IO | **Do not blanket-retry** — may be `AuditChainBroken`, schema mismatch, wrong passphrase, or transient IO |

### `retryable` on summary JSON (0082)

Additive boolean on `summary.json` / `--json` stdout — **not** a new exit code and **not** a substitute for classifying exit **5**.

| `retryable` | Typical cases |
|---|---|
| `true` | Operator cancel (exit 130), clear transient matter/disk IO |
| `false` | Success, usage, risk gate (65), partial fidelity (64), verify/count/report hard fails, schema / passphrase / audit-chain classes |

**Rule:** never script `while ($code -eq 5) { retry }` without reading `retryable` **and** the error class. Exit 5 remains mixed permanent/transient; `retryable: false` forbids blind loops even when the shell code is 5.

### PowerShell switch example

```powershell
& .\pst-dedup.exe unique-pst @inputs --out $Out --report-dir $Report --json
$code = $LASTEXITCODE
# Prefer parsing summary.json retryable when automating.
switch ($code) {
    0   { Write-Host 'ok — complete' }
    64  { Write-Host 'partial fidelity — retain artifact; disclose soft-fails' }
    65  { Write-Host 'export_risk gate — review summary.json before handoff' }
    130 { Write-Host 'cancelled — partial quarantined; retryable' }
    1   { Write-Host 'hard fail — do not hand off' }
    2   { Write-Host 'usage error — fix args' }
    5   {
        # NEVER blanket-retry exit 5.
        # Inspect summary.retryable + error class: AuditChainBroken / SchemaVersionMismatch /
        # WrongPassphrase are permanent until fixed; only clear transient IO after diagnosis.
        Write-Host 'matter IO/open — diagnose; do not blind retry'
    }
    default { Write-Host "exit $code — see docs/unique-pst-export.md" }
}
```

**Rule:** never script `while ($code -eq 5) { retry }` without classifying the error.

---

## 7. Artifact inventory & handoff

Typical `--report-dir` contents:

| Artifact | Role |
|---|---|
| `summary.json` | `unique_export_report_v1` — fidelity, exit, `export_risk`, `phase_timings`, digests, `retryable`, `bcc_suppressed_message_count`, `sent_message_with_no_recipients_count` |
| `export_messages.csv` | Winner → volume crosswalk (mandatory when messages written); includes `bcc_suppressed` (**0082**) |
| `export_attachments.csv` | Attach failure ledger when `--attach-ledger=full` |
| `volumes.csv` | Per-volume path/bytes/hashes |
| `decisions.csv` / `keepset.json` | Keep-set provenance |
| `integrity.csv` (optional) | Integrity detail when requested |

**Sensitivity:** report-dir often embeds **absolute workstation paths**, custodian basenames, subjects, and message IDs. Treat as matter-sensitive.

### Basename mode & Matter Archive

```powershell
.\pst-dedup.exe unique-pst @inputs --out $Out --report-dir $Report `
  --ledger-path-mode basename
```

- Default is **`full`** (absolute/workstation paths in CSV `source_path` columns).  
- **`basename`** rewrites path columns in **both** `export_messages.csv` and `export_attachments.csv` for handoff copies.  
- **`source_id` remains the join key** and is never basenamed away (present on both `export_messages.csv` and `export_attachments.csv`).  
- **Basename is not full de-identification** — custodian filenames and subjects remain.

**Mandatory when using basename:** retain a non-produced **Matter Archive** mapping:

| Field | Required |
|---|---|
| `source_id` | Yes (0-based index into `summary.inputs`) |
| Absolute source path | Yes |
| Source SHA-256 (preferred) | Strongly recommended |
| Collection / custodian label | Recommended |

Losing that mapping **breaks origin proof**. Do not ship the Matter Archive to opposing counsel unless intentional.

In-process QC during the same run continues to use full paths; basename applies at CSV serialization for handoff artifacts.

**Standalone `qc-pst` after basename:** when CSV `source_path` is basenamed and no longer opens as a file, re-QC resolves the open path via `source_id` + `summary.inputs` (or your Matter Archive mapping). Do not invent `source_id` 0 when the column is missing on older packs.

---

## 8. Disposition & secure purge

What accumulates on the workstation:

- Source staging copies (if not using archive-only mounts)
- `--out` unique volumes and multi-volume siblings
- `--report-dir` packs (may include sensitive paths)
- Temp / `.partial` quarantine from cancel or hard volume fail
- Optional ScanPST working copies

**When to purge:** after legal hold release / matter close, per firm retention schedule — not automatically by this product.

| Keep | Purge candidate |
|---|---|
| Matter Archive (paths, digests, chain) | Scratch unique volumes after delivery verified |
| Final production handoff set (if retained in DMS) | Intermediate report-dir copies on laptops |
| Privilege / work-product notes | Local ScanPST repair copies |

**Secure delete:** follow **firm DLP / secure-delete policy**. This product **does not** provide a secure-wipe binary and makes **no** cryptographic-erase claim. Point operators at internal IT/security procedure for media sanitization.

---

## 9. QC (0080) short

| Control | Default / note |
|---|---|
| `--qc-level` | Default **`sample`** (also `off` / `structure` / `full`) |
| `--qc-sample-max` | Default **64** risk-weighted sample |
| `--qc-scanpst` | Optional; requires discoverable classic `scanpst.exe`; runs on a **temp copy** |
| `--qc-external-reader` | BYOB `pffinfo` / `readpst` counts only — never auto-download |
| COM | **Not used** |

QC green is necessary but not sufficient for counsel sign-off when `export_risk` is elevated or exit is 64/65.

Standalone re-QC of a basename-mode report pack uses `source_id` + `summary.inputs` (or Matter Archive) when basenamed `source_path` values no longer resolve as files.

---

## 10. Timings

- **`summary.json` → `phase_timings`** — authoritative per-phase ms (scan / resolve / materialize / write / report / verify / …).  
- **Operator harness:** [`scripts/unique-pst-timing.ps1`](../scripts/unique-pst-timing.ps1) — parameterized inputs/out/report-dir; optional `-TimingJson`; optional `-RunScanFirst`.  
- **Historical evidence (not an SLA):** redacted multi-mailbox operator run (INC0102784 class) observed ~**3728** messages written, ~**366** attach fails, ~**275 s** wall on operator hardware. Use only as scale grounding — not a warranty or benchmark commitment.

---

## 11. Links

| Doc | Role |
|---|---|
| [`unique-pst-export.md`](unique-pst-export.md) | Flag encyclopedia |
| [`operator-golden-path.md`](operator-golden-path.md) | Day-1 RC paths (Path B unique-PST) |
| [`deferred.md`](deferred.md) | Residuals (basename closed in 0081; recipient table / retryable closed in **0082**) |
| Writer / fidelity notes under `docs/` | Volume shape honesty |
| [Microsoft Support — open PST in new Outlook](https://support.microsoft.com/en-us/outlook/open-and-find-items-in-an-outlook-data-file-pst) | Access date **2026-07-29** |
| [Microsoft Support — open/close Outlook Data Files](https://support.microsoft.com/en-us/outlook/open-and-close-outlook-data-files-pst) | Access date **2026-07-29** |
| [`scripts/unique-pst-timing.ps1`](../scripts/unique-pst-timing.ps1) | Timing harness |
