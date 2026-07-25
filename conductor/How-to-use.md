# How to start and use Dedupe Desk

Operator instruction set for the **Windows workstation** product.  
For a full feature inventory and screen map, see [`features.md`](features.md).  
For track status and post-Series-I guidance, see [`ROADMAP.md`](ROADMAP.md).

**UI:** native Windows app (`eframe` / `egui`) — **not** a browser or WASM app.  
**Default mode:** single-user, offline, one matter folder on local disk.

---

## 1. What you need

| Requirement | Notes |
|---|---|
| **Windows** | Primary supported OS |
| **Rust** (if building from source) | 1.80+ recommended; [rustup](https://rustup.rs/) |
| **Disk space** | Matter stores natives/text/indexes under the matter path — size grows with the case |
| **Evidence location** | Keep real client PSTs/exports **outside** the git repo (e.g. Desktop or encrypted volume) |
| **Optional** | Tesseract (OCR), local Whisper CLI (STT), AI API key — all **off by default** |

**Never** put production case data into `fixtures/` or commit it.

---

## 2. Build and start the app

### 2.0 Rapid UI / UX iteration (developers)

Desk is native **egui** — there is no browser-style hot reload. For layout and
interaction polish, prefer **debug** rebuilds of Desk only (not full workspace
release). See **[`ui-iteration.md`](ui-iteration.md)** for `cargo-watch` recipes
and what to avoid during pure UI work.

```powershell
cd C:\dev\Dedupe
cargo run -p dedupe-desk
# or: cargo watch -w crates/dedupe-desk -x "run -p dedupe-desk"
```

### 2.1 Build (from repo root)

```powershell
cd C:\dev\Dedupe

# Primary GUI — Dedupe Desk
cargo build --release -p dedupe-desk

# Headless CLI (automation / service host)
cargo build --release -p pst-dedup-cli
```

| Binary | Path |
|---|---|
| **Desk** | `target\release\dedupe-desk.exe` |
| **CLI** | `target\release\pst-dedup.exe` |

### 2.2 Launch Desk

```powershell
.\target\release\dedupe-desk.exe
```

Or during development:

```powershell
cargo run -p dedupe-desk --release
```

You should see **Home**: create or open a matter.

### 2.3 Quick CLI check

```powershell
.\target\release\pst-dedup.exe --help
```

### 2.4 Keep-set export (`pst-dedup keep-set`)

Series K path for multi-PST unique export planning (feeds EML pack **0067**, PST write **0068+**, report **0071**).

```powershell
.\target\release\pst-dedup.exe keep-set a.pst b.pst `
  --policy first_seen `
  --decision-csv C:\out\decisions.csv `
  --keep-set-json C:\out\keepset.json `
  --json
```

| Flag / concept | Notes |
|---|---|
| `--policy first_seen\|keep_largest\|prefer_path` | Default `first_seen`. Applied **after** fidelity preference (clean beats degraded). |
| `--prefer-path-contains` | Repeatable; used with `prefer_path` (e.g. `Primary`). |
| `--family-policy` | `keep_attachments_with_parent` (default) or `parents_only`. |
| `--materialize` | Full extract winners; hard fail **promotes** next peer (no ghost-drop). |
| `--decision-csv` | One row per recoverable message — emitted **only after** group resolve. |
| `--keep-set-json` | Schema `keep_set_v1`: winners + stats (no bodies). |
| Integrity flags | Same as `scan` (`--mode`, thresholds, `--allow-failed-files`, `--integrity-csv`). |

**Determinism:** absolute input paths are sorted before scan; residual ties use `(path_key, nid)`.  
**EDRM MIH:** optional MD5 of normalized Message-ID on decision/keep rows — **interop only**, not a suppress tier.  
**Source PSTs are always read-only.**

### 2.5 Unique EML pack (`pst-dedup unique-eml`)

Series K **fast path** after keep-set: write one `.eml` per exportable unique winner for
manual import into Outlook / Thunderbird (interim “clean PST” without our writer).

```powershell
.\target\release\pst-dedup.exe unique-eml a.pst b.pst `
  --out C:\out\unique_eml_pack `
  --policy first_seen `
  --decision-csv C:\out\decisions.csv `
  --keep-set-json C:\out\keepset.json `
  --json
```

| Flag / concept | Notes |
|---|---|
| `--out` | **Required** pack root. Created if missing; **refuse non-empty** unless `--overwrite`. |
| Keep-set flags | Same as `keep-set` (`--policy`, `--family-policy`, integrity thresholds, …). |
| `--files-per-volume` | Default **10000** — never dump unbounded EML into one NTFS folder. |
| `--volume-prefix` | Default `VOL` → `VOL001`, `VOL002`, … |
| `--manifest-json` | Default `{out}/manifest.json` (`eml_pack_v1`). |
| **Date** | Always **UTC +0000** (not host local timezone). |
| **Embedded msgs** | MIME `message/rfc822` (not silent octet-stream). |
| **Success rule** | `eml_written == unique` (post-promotion winners only). |

**Import notes:** review `manifest.json` + decision CSV; import each `VOL###` folder into
Outlook or Thunderbird; optionally move into a new PST. Not bit-identical re-serialize —
MIME is reconstructed from MAPI. Full guide: [`docs/unique-eml-import.md`](../docs/unique-eml-import.md).

---

## 3. Core concepts (2 minutes)

| Term | Meaning |
|---|---|
| **Matter** | One case folder: SQLite DB + CAS blobs + indexes + exports |
| **Source** | Something you ingest (folder, ZIP/Purview package, PST path) |
| **Extract** | Pull messages/files from PST (and later Office/PDF/…) into items + CAS |
| **Reduce** | Dedupe / thread / near-dup / cull (flags only — no silent deletes of source) |
| **Promote** | Put items into the **review corpus** (`in_review`) |
| **Review** | Code, privilege, redact, notes on the review set |
| **Produce** | Export natives + text + Concordance DAT for delivery |
| **Job** | Background work via process-runner (progress bar in Desk) |

**Navigation (top of Desk):** Home · Workspace · Reduce* · Review · Conversations · Produce · Gap · People · Clusters  

\* **Reduce** is a stub label today — run reduce jobs from **Workspace**.

Most screens need an **open matter** (create/open on Home first).

---

## 4. First-time golden path (recommended)

This is the standard offline counsel workflow.

### Step 1 — Create a matter (Home)

1. Open **Dedupe Desk**.  
2. Enter a **name** and choose a **folder path** for the matter (empty directory preferred).  
3. Optional: enable **encryption** and set a passphrase (remember it — there is no cloud recovery).  
4. Create the matter. Desk opens **Workspace**.

**Tip:** Use a path on a local disk (or your encrypted volume). Avoid network shares for the matter DB.

### Step 2 — Add evidence (Workspace)

1. **Add folder…** — Purview export tree or loose files.  
2. **Add ZIP…** — package ZIP (safe expand).  
3. **Add PST…** — single PST file path.  

Wait for the ingest job to finish (progress panel). You can resume after cancel/crash on checkpointed ingest.

### Step 3 — Extract mail (Workspace)

1. In the PST inventory, select a PST (or use **Extract all**).  
2. Click **Extract selected** / **Extract all**.  
3. Wait until extract jobs complete. Items and attachment natives land in the matter CAS.

### Step 4 — Reduce and promote (Workspace)

**Option A — profile (easiest)**

1. **Processing profile** dropdown → e.g. `standard`.  
2. **Apply defaults** (optional).  
3. **Run profile** — runs stages in order (dedupe → … → promote, depending on profile).

**Option B — step by step**

Typical order:

1. Dedupe  
2. Thread / near-dup (optional)  
3. Cull (optional presets)  
4. **Promote** to review corpus  

### Step 5 — Search index (Workspace)

1. **Build / Update search index** (`fts_index`) so Review keyword search works.  
2. Optional: **Build semantic index** if you enabled semantic search in Settings.

### Step 6 — Review (Review)

1. Open **Review** in the nav.  
2. Use **filters** and/or **keyword** box (and optional **Semantic** bar).  
3. Select an item → read body →:  
   - apply **codes**  
   - set **privilege / withhold** when needed  
   - **notes / highlights** (work product; not produced by default)  
   - **redactions** when required (regenerate redacted text)  
4. Use keyboard next/prev to move through the list; batch code when appropriate.

### Step 7 — QC (Workspace / Produce)

1. Run **QC** (pre-production checks) from Workspace process actions when available, or rely on Produce readiness.  
2. Fix errors (withheld-in-set, missing redacted text, broken family warnings, etc.).  
3. Re-run QC if your produce policy requires a fresh pass (`require_qc_pass`).

### Step 8 — Produce (Produce)

1. Open **Produce**.  
2. Set production **name**, **Bates prefix**, and **starting Bates number** (required for multi-volume safety — do not reuse `1` on volume 2 if volume 1 already used that range).  
3. Confirm withhold policy (skip withheld by default).  
4. Choose output folder or accept default under the matter `exports\productions\…`.  
5. Run produce.  
6. Deliver the folder: `NATIVES\`, `TEXT\`, `DATA\load.dat` (and CSV twin when enabled).

### Step 9 — Optional reporting

- **Workspace → Overview → Export matter report…** for a CSV metrics pack (no bodies/subjects in the pack design).

---

## 5. Screen-by-screen cheat sheet

### Home

| Do this | When |
|---|---|
| Create matter | New case |
| Open matter | Continue work |
| Enter passphrase | Opening an encrypted matter |
| Change passphrase | Credential rotation on encrypted matter |

### Workspace

| Do this | When |
|---|---|
| Add folder / ZIP / PST | New evidence |
| Extract | After PST/package available |
| Run profile / workflow | Automate multi-step processing |
| Build FTS / semantic index | Before Review search |
| Run OCR | After PDFs mark “needs OCR”; enable OCR in Settings first |
| Overview / report | Case status for stakeholders |
| Jobs list | See running/finished work; parent rows for profiles/workflows |

### Review

| Do this | When |
|---|---|
| Keyword / filters / saved searches | Find docs |
| Code / batch code | First-pass review |
| Privilege / withhold | Privilege calls |
| Redact | Before produce when needed |
| Notes | Work product (usually stays in-app) |

### Conversations

| Do this | When |
|---|---|
| Browse day-bucket chat | Teams/chat exports ingested (0055/0056) |

### Produce

| Do this | When |
|---|---|
| QC + produce volume | Deliver to opposing counsel / another platform |

### Gap

| Do this | When |
|---|---|
| Import expected custodians | Collection completeness |
| Import opposing DAT | Compare to their production |

### People / Clusters

| Do this | When |
|---|---|
| After `people_graph` / `concept_cluster` jobs | Investigation views (not a substitute for coding) |

### Settings

| Do this | When |
|---|---|
| Enable OCR + Tesseract path | Image/scan PDFs |
| Enable semantic search | Optional embedding search |
| AI provider (if used) | Opt-in; keys via env/keyring — suggestions are **not** final codes |

---

## 6. Common follow-on tasks

### 6.1 More PSTs later

Workspace → Add PST → Extract → re-run dedupe/cull/promote as needed → Update FTS → continue Review.

### 6.2 Office / PDF text

Workspace jobs: **Extract Office text**, **Extract PDF text**.  
PDFs with little text may show **Needs OCR** in Review until OCR runs.

### 6.3 Privilege log

Use privilege panel in Review; export privilege log CSV when your protocol requires it (separate from produce volume).

### 6.4 Encrypted matters

- Create with encryption enabled and a strong passphrase.  
- Every open needs the passphrase.  
- Case content temps stay under the matter when encryption is on.  
- **Change passphrase** re-wraps keys without re-encrypting all blobs from scratch.

---

## 7. Multi-user and service mode (opt-in)

Use only when multiple reviewers need the **same** matter concurrently.

1. **Do not** open the matter in solo Desk for write while the service is hosting it (exclusive lock).  
2. Bootstrap admin / users (CLI):

```powershell
.\target\release\pst-dedup.exe service bootstrap-admin --matter "C:\Matters\Case1" --name admin
# add reviewers as documented in matter-service README
.\target\release\pst-dedup.exe service serve --matter "C:\Matters\Case1"
```

3. Default bind is **loopback** (`127.0.0.1`). LAN bind is explicit and more sensitive.  
4. **Desk Connect (0064):** on another machine or the same host with no local matter open:
   - Home → **Connect to matter-service…** → URL + password (or **Sign in with SSO**).
   - Banner shows Connected identity; **Review (remote)** for list/body/codes with OCC.
   - **Disconnect** before opening a Solo local matter. Produce/jobs stay Solo/host.
5. HTTP API remains available for scripts; clipboard bearer paste is **not** the operator SSO path.

Encrypted matter: unlock once on the **host** process; clients do not need the passphrase if the service holds the session.

### Solo produce profile (0064)

On Produce → **Start produce**, pick a **production profile** (or Default engine) and set **Bates start** ≥ 1. Pre-flight fails closed on unresolved profile / invalid Bates / QC gate.

---

## 8. Headless / agent workflow (CLI)

Same engine as Desk — useful for overnight jobs.

```powershell
$m = "C:\Matters\cli-case"
.\target\release\pst-dedup.exe matter create --path $m --name "cli-case" --json

# Example jobs (kinds vary; see --help and job list)
.\target\release\pst-dedup.exe job run --path $m --kind classify --json
.\target\release\pst-dedup.exe profile run --path $m --profile standard --json
.\target\release\pst-dedup.exe workflow run --path $m --workflow builtin:reduce_only_chain --json
```

Then open the same path in Desk for human review.

PST-only tools (no matter):

```powershell
.\target\release\pst-dedup.exe inspect C:\Evidence\mail.pst --top 20
.\target\release\pst-dedup.exe scan C:\Evidence\mail.pst --json
.\target\release\pst-dedup.exe scan C:\Evidence\a.pst C:\Evidence\b.pst --mode best-effort --csv out\report.csv --json
```

### 8.1 Multi-PST scan integrity (track 0065)

| Topic | Behavior |
|---|---|
| **Modes** | `--mode best-effort` (default triage) · `--mode strict` (fail closed on any skip/degraded) |
| **Reason codes** | Stable strings in JSON/CSV: `OPEN_FAILED`, `CRC_MISMATCH`, `BODY_TRUNCATED`, `BODY_UNAVAILABLE`, `ATTACH_META_FAILED`, `ORPHANED_NODE`, … |
| **Degraded keep (best-effort)** | Attach meta fail, partial/corrupt body, body unavailable, orphan path — kept with `degraded_reasons` (never silent) |
| **Strict** | Those events become skips; any skip/partial/failed → non-zero exit |
| **Integrity ledger** | Streaming `*.integrity.csv` (sidecar of `--csv` or `--integrity-csv`); O(1) memory; full exclusion list |
| **Preflight** | `ok` / `re_export_recommended` / `not_export_ready` from skip/CRC/failed-file rates — **guidance only, not repair** |
| **Non-zero exit** | Integrity/strict/failed-files still **fully write** CSV + integrity CSV + JSON **before** exit; non-zero means “not export-clean by default”, not “outputs missing” |
| **Source PSTs** | Read-only; **no** in-place repair (Class C forever out). External repair (scanpst) only on a **copy** |
| **Orphan vs root** | Filter on `is_orphaned` — empty `folder_path` alone is not orphan |
| **4KB body preview** | Intentional Tier-2 cap is **not** `BODY_TRUNCATED` |

Flags: `--allow-failed-files`, `--max-skip-rate` (0.05), `--max-crc-skip-rate` (0.01), `--max-failed-file-rate` (0.0), `--skip-limit` (JSON sample cap).

---

## 9. What not to do

| Don’t | Why |
|---|---|
| Put client PSTs inside the git repo | Evidence policy; leak risk |
| Open the same matter in two write processes | Corruption / lock failure |
| Produce without checking withhold/redactions | Privilege/PII leakage |
| Reuse Bates start `1` on a second volume of the same prefix | Collisions; rejection risk |
| Enable OCR/AI/cloud without understanding opt-in | Privacy / cost / egress |
| Expect Desk to be a website | Native desktop only |

---

## 10. Troubleshooting

| Symptom | What to try |
|---|---|
| Nav does nothing (Workspace/Review) | Create or open a matter on **Home** first |
| Keyword search empty | Run **Build / Update search index** on Workspace |
| Body empty / Needs OCR | Run PDF extract and/or OCR; check Settings OCR path |
| Job stuck / cancelled | Check progress panel; resume if supported; fix item errors in overview |
| “Matter already open” | Close other Desk/service holding the lock |
| Produce blocked | Run QC; fix error findings; refresh fingerprint |
| Encrypted open fails | Wrong passphrase; matter path must match encrypted store |
| UI slow while job runs | Expected under load; wait for progress; avoid starting overlapping jobs |

Logs: CLI uses stderr (`-v` / `-vv`). Prefer `$env:RUST_LOG = 'error'` for quiet automation.

---

## 11. Where things land on disk

Under your matter root (illustrative):

```text
matter/
  matter.db              # metadata
  blobs/                 # CAS natives & text
  index/                 # FTS (Tantivy)
  exports/
    productions/…        # produce volumes
    reports/…            # matter report packs
    qc/…                 # QC outputs (when used)
  workspace/             # temps / worker scratch
```

Only give opposing counsel the **production export** folder you intentionally produced — not the whole matter.

---

## 12. Related reading

| Doc | Use when |
|---|---|
| [`features.md`](features.md) | What every feature is and which screen owns it |
| [`ROADMAP.md`](ROADMAP.md) | Track status + after-0061 plan |
| `crates/dedupe-desk/README.md` | Deeper UI/process details |
| `crates/matter-produce/README.md` | Produce/DAT contracts |
| `crates/matter-service/README.md` | Multi-user service |
| Root `README.md` | Build matrix + CLI examples |

---

*This guide describes the default offline Desk path and common opt-ins. Optional platform SSO and cloud CAS are advanced; see crate READMEs and Series I track specs before enabling them.*
