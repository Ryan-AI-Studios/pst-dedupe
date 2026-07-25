# Dedupe Desk — Feature catalog & UI map

> **Living product inventory** for operators and planners.  
> **Implementation truth** lives in crates; tracks live under `conductor/####-*/`.  
> **UI stack:** native Windows **`eframe` / `egui`** (`dedupe-desk`) — **not WASM**.  
> **Evidence:** real case data stays outside git; features are proven with synthetic fixtures + optional local smoke.

**Surfaces**

| Surface | Binary / crate | Role |
|---|---|---|
| **Dedupe Desk** | `dedupe-desk` | Primary product shell (matter-centric review workstation) |
| **CLI** | `pst-dedup` | Headless matter jobs, service host, platform admin, PST tools |
| **Matter service** | `matter-service` (via CLI) | Opt-in multi-user HTTP host (0058); platform/OIDC (0059) |
| **Legacy GUI** | `pst-dedup-gui` | Older PST scan/dedup wizard (still builds; not the main product) |

**Product modes**

| Mode | How | Auth | Default? |
|---|---|---|---|
| **Desk solo** | Local matter open | Free-string actor | **Yes** |
| **Local multi-user** | `pst-dedup service serve` + clients | Matter users + bearer | Opt-in |
| **Platform / SSO** | serve + `platform.db` + OIDC | Tenant isolation | Opt-in |
| **Cloud CAS** | Storage backend S3-compatible (0061) | IAM/env secrets | Opt-in; offline default |

---

## 1. Feature catalog

Features are grouped by workflow stage. Track IDs are the primary provenance.

### 1.1 Matter store & security foundation

| Feature | Detail | Tracks |
|---|---|---|
| **Matter directory** | SQLite metadata + CAS blobs + audit + jobs under one storage root | 0015 |
| **Content-addressable store (CAS)** | SHA-256 natives/text; streaming put; optional chunked AEAD when encrypted | 0015, 0057 |
| **Audit hash chain** | Append-only events (actor, action, params, tool version, prev hash) | 0015, 0010 |
| **Jobs + checkpoints** | Resumable long work; cancel/resume via process-runner | 0015, 0019 |
| **Optional encryption at rest** | Passphrase → KDF → KEK wraps DEK; encrypt DB+CAS+FTS; change passphrase re-wraps DEK; matter-local temps | 0057 |
| **Exclusive matter open** | OS file lock on write-open; blocks dual Desk+service writers | 0058 |
| **Schema versioning** | Migrations through current `SCHEMA_VERSION` (v38+; v39 with 0061) | 0015+ |

### 1.2 Ingest & extraction

| Feature | Detail | Tracks |
|---|---|---|
| **Purview / ZIP / folder ingest** | Safe expand, path traversal rejection, leaf checkpoints | 0016 |
| **PST as source** | Add PST path; inventory mail.pst-class items | 0018, 0020 |
| **PST extract** | Folders/messages/attachments → Normalized Items + CAS natives; mid-folder resume | 0018 |
| **Normalized Item model** | Family graph, logical_hash, BCC-aware identity fields | 0017 |
| **Office text extract** | DOCX/XLSX/PPTX → text CAS (`office_extract`) | 0033 |
| **PDF text extract** | Text CAS; empty/low-text → `pdf_needs_ocr` (`pdf_extract`) | 0034 |
| **Calendar / ICS** | Multi-event containers + PST calendar class (`ics_extract`) | 0035 |
| **OCR (opt-in)** | Local Tesseract CLI + mock; job `ocr`; Settings enable | 0036 |
| **File category taxonomy** | `taxonomy_v1` classifier job `classify` | 0037 |
| **Teams / chat adapters** | HTML+PST → items; day-bucket `conversation_id` | 0055 |
| **Transcription (opt-in)** | Local Whisper-class STT → transcript CAS + FTS | 0053 |

### 1.3 Reduce, promote, identity

| Feature | Detail | Tracks |
|---|---|---|
| **Matter dedupe** | MID → logical_hash; family flags; job `dedupe` | 0021 |
| **Email threading** | `thread_id` from headers + subject fallback; job `thread` | 0022 |
| **Near-duplicate** | MinHash shingles + LSH; pivot/similarity; job `neardup` | 0023 |
| **Cull / reduce** | Flag-only presets (unique/date/path/type/empty; optional local hash-list DeNIST); job `cull` | 0024 |
| **Promote to review** | Review corpus membership; `auto` = cull_included ∥ unique_only; family expand; job `promote` | 0025 |

### 1.4 Review core

| Feature | Detail | Tracks |
|---|---|---|
| **Review list + body** | Virtualized list; keyboard next/prev; CAS body (text strip of HTML) | 0026 |
| **Coding & batch** | Code catalog; batch add/remove; family opt-in; audit | 0027 |
| **Filters & saved searches** | Flat AND FilterSpec; family CTE; codes/dates; Load more | 0028 |
| **Keyword FTS** | Per-matter Tantivy; Boolean/phrase; ∩ filters; job `fts_index` | 0029 |
| **Notes & highlights** | Stand-off highlights; has_notes filters | 0030 |
| **Privilege workflow** | Claims, withhold holds, privilege log CSV, 502 protocol stub | 0031 |
| **Redaction v1** | Text regions + true redacted CAS (`[REDACTED]`); blackout paint | 0032 |
| **Entity / PII packs** | Offline regex + Luhn; masked hits; job `entity_scan` | 0046 |
| **AI suggestions (opt-in)** | Provider trait; first-pass codes; suggestions ≠ final | 0051 |
| **AI citations** | Grounded quotes; human promote to codes | 0052 |
| **Semantic search (opt-in)** | Local embeddings; separate from keyword FTS; job `semantic_index` | 0050 |
| **Multilingual / CJK FTS** | Hybrid n-gram packs; fingerprint/rebuild | 0054 |
| **Conversation review** | Day-bucket list + paged stream for chat | 0056 |

### 1.5 Analytics & investigation

| Feature | Detail | Tracks |
|---|---|---|
| **Case overview** | KPIs: counts, size, review progress, errors, categories, custodians | 0038 |
| **Matter report export** | CSV pack from overview + jobs (`matter_report_v1`) | 0039 |
| **People–comms graph** | Participants, directed edges, timeline; job `people_graph` | 0047 |
| **Concept clustering** | Offline TF-IDF k-means + c-TF-IDF labels; job `concept_cluster` | 0048 |
| **Sentiment** | Offline VADER-class; opt-in job `sentiment` | 0049 |
| **Gap analysis** | Expected custodians + opposing DAT set-diff | 0042 |

### 1.6 Production & QC

| Feature | Detail | Tracks |
|---|---|---|
| **Production export** | NATIVES + TEXT + Concordance DAT (+ CSV twin); Bates; withhold fail-closed; redacted-text only | 0040 |
| **Production QC** | Pre-produce rules; findings CSV; fingerprint gate; `require_qc_pass` | 0041 |
| **Jurisdiction profiles** | Named load-file/Bates/layout + bound QC packs; job-time Bates start | 0060 |

### 1.7 Automation

| Feature | Detail | Tracks |
|---|---|---|
| **Process runner** | Single matter worker; progress watch; cancel | 0019 |
| **Processing profiles** | Named stage presets; sequential `profile_run` | 0043 |
| **Workflow engine** | Declarative multi-node `workflow_run`; hard gates; parent_job_id | 0044 |
| **CLI automation** | Full headless matter/job/profile/workflow/produce/qc/gap/service/platform | 0045 |

### 1.8 Platform (opt-in)

| Feature | Detail | Tracks |
|---|---|---|
| **Multi-user service** | Loopback HTTP; users/roles; locks; batches; OCC; sampling QC; strict actor | 0058 |
| **Multi-tenant + OIDC** | platform.db; tenants; IdP; JIT allowlists; PMK for IdP secrets; storage sandbox | 0059 |
| **Cloud blob backends** | BlobStore trait; local default; S3-compatible opt-in; cache + integrity locks | 0061 |

### 1.9 PST tools (CLI / legacy)

| Feature | Detail | Tracks |
|---|---|---|
| **Inspect / scan / dups** | Pure-Rust PST read; tiered dedup; CSV report | 001–014 lineage, CLI |
| **EML export (legacy GUI)** | Unique messages as EML | track005 |

---

## 2. UI architecture (Dedupe Desk)

### 2.1 Stack & rules

```text
┌─────────────────────────────────────────────────────────────┐
│  dedupe-desk.exe  (native eframe / egui — NOT WASM)         │
│                                                             │
│  UI thread: paint, input, start/cancel jobs, open_for_read  │
│       │                                                     │
│       ├── ProcessRunner (background matter worker)          │
│       │     ingest / extract / jobs / produce / qc / …      │
│       ├── Off-thread file dialogs (rfd)                     │
│       └── Short-lived Matter::open_for_read for lists       │
└─────────────────────────────────────────────────────────────┘
```

| Do | Don’t |
|---|---|
| Jobs via process-runner | Heavy parse/hash on UI thread |
| `open_for_read` for concurrent lists | Dual write-open same matter (lock fails closed) |
| Repaint ~100ms while job Running | Spin request_repaint every frame |

### 2.2 Global chrome (all screens)

| Element | Behavior |
|---|---|
| **Nav bar** | Home · Workspace · Reduce* · Review · Conversations · Produce · Gap · People · Clusters |
| **Matter context** | Current matter name/path when open |
| **Progress strip** | Live job progress when runner busy (Workspace and job-aware screens) |
| **Error / status** | Transient messages |
| **Settings** | OCR enable/paths, semantic enable, AI-related prefs (opt-in) |
| **About** | Version / product info |

\* **Reduce** is still a **stub** nav entry (reduce jobs live under Workspace process actions / profiles).

Screens that **require an open matter** ignore nav clicks until Home create/open succeeds.

### 2.3 Screen map

#### A. Home

**Purpose:** Matter lifecycle entry.

| UI | Features |
|---|---|
| Create matter | Name, path; **optional encrypt** + passphrase confirm (0057) |
| Open matter | Folder picker; **unlock passphrase** if encrypted |
| Recent / path display | Local paths only |
| Change passphrase | When encrypted matter open (0057) |

**Connections**

- Success → **Workspace** (matter open).  
- No matter → other nav targets blocked.

---

#### B. Workspace

**Purpose:** Ingest, extract, process pipeline, overview, automation.

| Region | Controls / features |
|---|---|
| **Header** | Matter name, path, id |
| **Progress** | Running job stage/counts; cancel |
| **Sources** | Add folder / ZIP / PST; source list & status |
| **PST inventory** | Select PST; Extract selected / Extract all (queued) |
| **Process actions** | Dedupe, thread, neardup, cull, promote, classify, office/pdf/ics extract, OCR, FTS build/rebuild, semantic build/rebuild, entity scan, people_graph, cluster, sentiment, … (as wired) |
| **Processing profile** | Dropdown built-ins + user; Apply defaults; Run profile; Save as… |
| **Workflow** | Dropdown; Run workflow; bind source/PST params |
| **Case Overview** | KPIs, rollups, errors-by-code, jobs strip; Refresh; **Export matter report…** |
| **Jobs list** | History; parent job grouping for profile/workflow children |
| **Counts / stats** | Snapshot counters |

**Connections**

- After promote → operator goes to **Review**.  
- After QC → **Produce**.  
- Teams chat content → later **Conversations**.  
- Jobs power **People** / **Clusters** data.

---

#### C. Review

**Purpose:** First-pass / QC coding on the review corpus (core counsel loop).

| Region | Features |
|---|---|
| **Keyword bar** | Tantivy query; compose with filters; needs FTS index |
| **Semantic bar** | Separate embedding search (opt-in index) |
| **Filter bar** | FilterSpec chips: codes, dates, privilege, OCR need, notes, categories, calendar, …; saved searches; Load more |
| **Privilege protocol strip** | Matter-level 502 / protocol stubs |
| **Item list** | Virtualized rows; keyboard next/prev; selection for batch |
| **Batch bar** | Apply/remove codes to selection/family |
| **Body viewer** | Text body; highlights; redaction paint/mode; AI citation paint |
| **Coding panel** | Codes on item; catalog |
| **AI suggestions** | Opt-in; accept/reject; not auto-final |
| **Privilege panel** | Claim/withhold/status |
| **Notes / highlights** | Work product (default not produced) |
| **Redactions panel** | Regions; regenerate redacted text |
| **Entity hits** | Masked PII/entity results |
| **Family strip** | Parent/children navigation |

**Connections**

- **Workspace** builds indexes / extracts text needed for body & FTS.  
- **Produce** consumes codes/withhold/redactions.  
- **Conversations** for chat-shaped items (same matter).  
- Jump-to-item from Gap/QC findings (partial residual polish).

---

#### D. Conversations

**Purpose:** Chat/Teams day-bucket review (0056).

| Region | Features |
|---|---|
| Conversation list | Day-bucket / conversation_id |
| Message stream | Paged load earlier/more; inline chrome |
| Handoff | Open related review item / coding when wired |

**Connections**

- Depends on **0055** ingest/adapters + extract.  
- Coding/privilege may hand off into **Review** item model.

---

#### E. Produce

**Purpose:** Counsel delivery volume + QC gate.

| Region | Features |
|---|---|
| Produce dialog | Name, Bates prefix, **bates start** (0060), fail-if-withheld, expand-family, output folder |
| **Production profile** | Built-in / matter profiles (CLI + Desk Solo produce dropdown **0064**) |
| QC readiness | Soft/hard gate from last QC run + fingerprint |
| Run produce | Job `produce` → NATIVES/TEXT/DATA load.dat |
| Findings view | Last QC findings sample (produce_qc helpers) |

**Connections**

- **Workspace** Run QC job first when `require_qc_pass`.  
- **Review** must have coded/withheld/redacted correctly.  
- **Gap** may use opposing DAT from prior productions.

---

#### F. Gap

**Purpose:** Collection completeness vs roster; opposing production set-diff.

| Region | Features |
|---|---|
| Expected custodians | Import roster CSV; refresh |
| Collection gap | Date window; run gap |
| Opposing production | Import DAT; run compare |

**Connections**

- Uses extracted item metadata from **Workspace** pipeline.  
- Opposing DAT often from external produce or prior **Produce**.

---

#### G. People

**Purpose:** Who talked to whom (0047).

| Region | Features |
|---|---|
| People / edges / timeline tables | After `people_graph` job |
| Filters / sort | As implemented in people_ui |

**Connections**

- Requires extract + participant population.  
- Investigation companion to **Review**, not a substitute for coding.

---

#### H. Clusters

**Purpose:** Theme/concept groups (0048).

| Region | Features |
|---|---|
| Cluster sets / labels | After `concept_cluster` |
| Drill toward review | Optional handoff residual |

**Connections**

- Needs text-bearing items (extract/OCR).  
- May inform **Review** filters (manual today).

---

#### I. Reduce (stub)

**Purpose:** Placeholder nav; **reduce jobs are launched from Workspace** (dedupe/cull/promote/profiles).

**Planned consolidation:** either remove stub or promote to a dedicated Reduce board (not scheduled).

---

#### J. Settings (modal / panel)

| Feature | Notes |
|---|---|
| Enable local OCR + tool paths | Off by default |
| Semantic search enable | Dual-write matter flag |
| AI provider prefs | Off by default; keys via keyring/env |
| Connect to service (thin remote Review) | **0064** shipped |

---

## 3. How screens connect (operator mental model)

```text
                         ┌──────────┐
                         │   Home   │  create / open / unlock
                         └────┬─────┘
                              │ matter open
                              ▼
┌──────────────────────────────────────────────────────────────┐
│                        WORKSPACE                              │
│  ingest → extract → (office/pdf/ocr) → classify               │
│  dedupe / thread / neardup / cull → promote                   │
│  fts / semantic / entity / people / cluster / sentiment       │
│  profiles & workflows · overview · report export · QC job     │
└───┬──────────────┬──────────────┬──────────────┬─────────────┘
    │              │              │              │
    ▼              ▼              ▼              ▼
┌────────┐   ┌────────────┐  ┌────────┐   ┌──────────┐
│ Review │   │Conversations│  │ People │   │ Clusters │
│ code   │   │ chat stream │  │ graph  │   │ themes   │
│ priv.  │   └────────────┘  └────────┘   └──────────┘
│ redact │
└───┬────┘
    │ ready set
    ▼
┌────────┐     findings      ┌─────────┐
│Produce │◄──────────────────│   Gap   │  roster / opposing DAT
│ + QC   │                   └─────────┘
└────────┘
```

---

## 4. End-to-end workflows

### 4.1 Core counsel path (offline Desk — P0 golden path)

```text
1. Home → Create matter (optionally encrypt)
2. Workspace → Add ZIP/folder/PST
3. Workspace → Extract (selected or all)
4. Workspace → Run profile "standard" OR stepwise:
     dedupe → (thread/neardup optional) → cull → promote
5. Workspace → Build search index (fts_index)
6. Review → filter/keyword → code / privilege / redact / notes
7. Workspace → Run QC (or Produce soft-gate)
8. Produce → set Bates start + prefix → produce volume
9. Optional: Overview → Export matter report
```

**Success:** `exports/productions/...` with DAT + natives + text; withhold/redaction safe.

---

### 4.2 Purview package day

```text
Home open → Workspace Add folder/ZIP (Purview export)
  → ingest checkpoints resume on failure
  → Extract PST leaves found in package
  → continue 4.1 from dedupe/promote
```

---

### 4.3 OCR / image-heavy PDF path

```text
Workspace → pdf_extract → items marked pdf_needs_ocr
Settings → Enable local OCR + Tesseract path
Workspace → Run OCR
Workspace → Rebuild FTS
Review → Needs OCR filter clears as text appears
```

---

### 4.4 Privilege & redaction → produce

```text
Review → mark privilege / withhold
Review → redaction mode → regions → regenerate redacted text
Workspace → QC (withheld-in-set, redacted_text_missing, …)
Produce → fail_if_withheld optional; redacted TEXT only in volume
Privilege log CSV (0031) separate export when needed
```

---

### 4.5 Automation-heavy (profiles / workflows / CLI)

**Desk**

```text
Workspace → select profile → Run profile
  OR select workflow (e.g. ingest_then_standard, qc_then_produce) → Run
```

**CLI (agents / CI)**

```text
pst-dedup matter create --path … --name …
pst-dedup job run --path … --kind extract_pst --json
pst-dedup profile run --path … --profile standard --json
pst-dedup workflow run --path … --workflow builtin:qc_then_produce --json
pst-dedup produce run --path … --profile us_concordance_native_text_v1 --bates-start 1
```

Same matter DB; Desk can open afterward for human review.

---

### 4.6 Multi-user concurrent review (opt-in)

```text
Host:  pst-dedup service bootstrap-admin / user add
Host:  pst-dedup service serve --matter …   # exclusive lock; loopback
Client A/B: API or Desk Connect (0064)
  login → checkout batch / lock item → code with OCC version
  logout → locks released
```

**Desk solo must not write-open** the same matter while service hosts it.

---

### 4.7 Platform SSO (opt-in)

```text
pst-dedup platform tenant create / idp set / matter register
pst-dedup service serve --platform platform.db --matter …
Browser/client: OIDC login (PKCE) → JIT only if domain/group allowlist
Tenant A cannot open Tenant B matters
```

---

### 4.8 Investigation side paths

| Goal | Path |
|---|---|
| Who communicated? | Workspace people_graph → **People** |
| Themes? | concept_cluster → **Clusters** → manual Review filters |
| Missing custodians? | **Gap** roster + collection gap |
| Vs opposing prod? | **Gap** import DAT → set-diff |
| Chat day? | Teams extract → **Conversations** |
| PII heat? | entity_scan → Review entity hits |
| Sentiment? | sentiment job → filters/fields as exposed |

---

### 4.9 Encrypted matter lifecycle

```text
Home create --encrypt → passphrase
Open → unlock modal
Work as usual (temps under matter when encrypted)
Change passphrase → re-wrap DEK only
Close/Drop → seal encrypted container
```

---

## 5. CLI surface map (non-UI)

| Area | Commands (illustrative) |
|---|---|
| PST tools | `scan`, `inspect`, `dups` |
| Matter | `matter create/open/…` |
| Jobs | `job run/resume/cancel/status/list` |
| Profiles / workflows | `profile …`, `workflow …` |
| Produce / QC / gap | `produce`, `qc`, `gap`, `production-profile` |
| Service | `service serve/bootstrap-admin/user` |
| Platform | `platform tenant/idp/matter` |

Use CLI for automation; Desk for human review. Both share `matter-core` + process-runner handlers.

---

## 6. Planned UI (Series J)

| Planned | Track | Status / adds to map |
|---|---|---|
| **Connect to service** | 0064 | **Shipped (impl)** — Home Connect dialog, Connected banner, thin remote Review |
| **SSO sign-in** | 0064 / D-0059-02 | **Shipped (soft)** — loopback handoff + exchange; clipboard paste banned |
| **Produce profile dropdown** | 0064 / D-0060-02 | **Shipped** — Solo produce dialog + Bates start + pre-flight |
| **RC golden path doc** | 0062 | **Completed** |
| **Reduce screen** | residual | May absorb Workspace reduce actions |

---

## 7. Feature ↔ primary UI entry (quick index)

| Feature area | Primary UI | Also |
|---|---|---|
| Create/open/encrypt | Home | CLI matter |
| Ingest/extract/process | Workspace | CLI job/profile/workflow |
| Overview/report | Workspace | CLI report |
| Code/privilege/redact | Review | Service API |
| Chat | Conversations | — |
| Produce/QC | Produce + Workspace QC | CLI produce/qc |
| Gap | Gap | CLI gap |
| People/clusters | People / Clusters | jobs from Workspace |
| FTS/semantic | Review bars + Workspace index jobs | — |
| Multi-user | Service (CLI) + Desk Connect | Thin remote review (codes); jobs Solo |
| SSO/platform | CLI platform + service | Desk SSO loopback handoff |
| Cloud CAS | Config/CLI (0061); no full Desk panel required | — |

---

## 8. Related docs

| Doc | Role |
|---|---|
| [`How-to-use.md`](How-to-use.md) | Operator start + golden path |
| [`ROADMAP.md`](ROADMAP.md) | Track status + post-0061 addendum |
| [`sequencing.md`](sequencing.md) | Execution order |
| [`../docs/deferred.md`](../docs/deferred.md) | Residuals |
| `crates/dedupe-desk/README.md` | UI thread rules + control details |
| `C:\dev\Dedupe-plan.md` | Plan-of-record product architecture |

---

*Last aligned with Desk `Screen` enum, workspace/review modules, Series I (0057–0061), Series J **0062–0063 Completed** / **0064 implemented** (Connect + produce profile UX; registry Completed by orchestrator after review).*
