# 0084 — Named Property Resolution & Cloud Attach Detect

> Structure follows `C:\dev\coordinated\conductor\templates\0000-Description\spec.md`.
> Expanded subsections under §2–§3 are normative design for implementers. DoD is §7.
>
> **Review fold-in (2026-07-29):** dual-AI review of Ready draft incorporated below.
> Disposition of each claim is in §2.10 (agree / partial / decline with reason).

- **Track ID:** 0084-NamedPropCloudAttach
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → Series M (unique-export fidelity residuals) after 0082–0083
- **Cross-repo contract:** n/a
- **Status:** Ready — not started (review-folded 2026-07-29)
- **Depends on:** 0069 · 0073 · 0074 · 0080 · 0082 · 0083 (all **Completed** on board)
- **Spec authored:** 2026-07-29
- **Series:** M (Unique export fidelity residuals)

---

## 1. Objective

Ship **reader-side MS-PST named-property resolution** sufficient to **detect attachment-table cloud/modern (OneDrive/SharePoint web-reference) attachments**, mark them as **attach-incomplete for Mode A**, emit a stable **ledger reason with actionable URL/provider metadata**, **preserve pointer metadata** on the unique-PST (no invented payload bytes), and replace the 0080 **blind-spot silence** with an honest fidelity-contract / operator story — **without** downloading cloud payloads, inventing attach bytes, claiming body-inline link coverage, or shipping a full MAPI named-prop encyclopedia.

---

## 2. Context (read before starting)

### 2.1 Why this track exists now

| Deferred / ceiling | Severity | Claim |
|---|---|---|
| **D-0080-cloud-attachments** | P2 | Cannot detect `PidNameAttachmentProviderType`; contract marks cloud as `DroppedByDesign` blind spot |
| **D-0068-04** named-prop residual | — | Full named-prop set beyond store stub; attach/recip halves closed elsewhere |
| **0083 Mode A honesty ceiling** | — | Incomplete predicate **cannot** treat undetected link-only attaches as incomplete |
| `fidelity_contract_v1` | — | `cloud_modern_attachments` + `PidNameAttachmentProviderType` + generic `named_properties` all DroppedByDesign |

0082/0083 closed recipients and Mode A promote. The next board-named Series M residual is **named props / cloud attach**. Modern attachments are standard in M365 collections; eDiscovery practice requires **flagging** link-only attachment-table items, not silently treating them as successful tiny attaches — and not shipping empty “ghost” attach rows with no recoverable pointer for supplemental collection.

### 2.2 Industry / protocol anchors (researched 2026-07-29; re-checked on review fold-in)

**MS-PST Named Property Lookup Map** ([learn.microsoft.com](https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/e17e195d-0454-4b9b-b398-c9127a26a678), access date 2026-07-29):

- One Name-to-ID-Map per PST at **`NID_NAME_TO_ID_MAP` = `0x61`** (constant already in `pst-reader` / writer stub).
- Standard PC with special properties: Entry Stream, GUID Stream, String Stream, hash table.
- Named property = **(GUID, identifier)** where identifier is a **string name** or **16-bit LID**.
- Well-known sets/names: **[MS-OXPROPS]**.

**PidNameAttachmentProviderType** ([MS-OXPROPS](https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-oxprops/559749de-8dc9-4b60-b6f0-9d4547fd5032), access date 2026-07-29) — **verified for Phase 0 lock**:

| Field | Value |
|---|---|
| Property set | **PSETID_Attachment** `{96357F7F-59E1-47D0-99A7-46515C183B54}` |
| Property name | **`AttachmentProviderType`** (string name — **not** a numeric LID) |
| Data type | **PtypString** (`0x001F`) |
| Role | Provider type for a **web reference attachment** |
| Documented values (MS) | **`OneDrivePro`**, **`OneDriveConsumer`** only — treat as **open string**, not a closed enum |

Related PSETID_Attachment props (same GUID; co-allowlist only if Phase 0 fixtures show URL/payload signal):

| Name | Type | Role |
|---|---|---|
| `AttachmentPermissionType` | PtypInteger32 | Permission metadata for web-ref attach |
| `AttachmentOriginalPermissionType` | (see MS-OXPROPS) | Original permission for web-ref |

**URL location:** protocol docs for provider type do **not** by themselves define a single public URL prop. Phase 0 must lock where the URL lives on real/synthetic fixtures among:

1. Classic tags already readable (`PidTagAttachLongFilename` / path-shaped strings / long pathname if present).
2. Additional PSETID_Attachment public-string named props observed on fixtures (allowlist after sample).
3. If only provider type is present with no URL string → ledger still fires; `cloud_url` cell empty; residual honesty in docs.

**eDiscovery / modern attach practice (2024–2026; Purview page updated 2026-06-11, article ms.date 2026-02-27):**

1. **Cloud attachments** store a **link** (and provider metadata), not necessarily the file bytes, in the PST/message.
2. Purview eDiscovery can **collect** linked SharePoint/OneDrive content when operators enable “Access links (cloud attachments) in messages” — a **network/service** capability this offline product **does not** replicate.
3. Purview’s **link extraction limits** (HTML body only; 100k body chars; 2,048-char URL; first 50 links; no quoted/forwarded portions; etc.) are evidence that **body-scan modern-attachment detection is a different subsystem** from attachment-table web-ref detection — see §2.7.5 residual.
4. Offline PST tools **must not claim** payload preservation when only a link is present.
5. **No network fetch** in this product path (offline-first invariant) — detection + declaration + pointer preserve, not hydration.
6. Mode A (0083) should prefer a peer with **physical** attach bytes when the higher-ranked peer is link-only — only possible if link-only is classified incomplete.
7. **Do not** cite vendor draft “Reconstruction-Grade” / Cloudficient vocabulary as settled industry standard in the runbook (unlike Sedona for cross-custodian in 0083). Informal review notes only if useful.

**Known property targets for this track:**

| Target | Role |
|---|---|
| `PidNameAttachmentProviderType` | Primary cloud/provider signal when present (**GUID+name** resolve) |
| Related PSETID_Attachment public-string props (URL / permission) | Secondary — allowlist only after Phase 0 sample |
| Classic tags already readable | `PidTagAttachMethod` (by-ref/OLE/web), missing `PidTagAttachDataBinary` stream, zero-size + long pathname URL heuristics as **fallback** when NPMAP missing |

Do **not** hard-code a giant prop list. Phase 0 locks the **minimum allowlist** that fires on synthetic + (optional) operator PSTs.

### 2.3 Live code snapshot (verified 2026-07-29)

| Surface | State |
|---|---|
| `NID_NAME_TO_ID_MAP = 0x61` | Constant exists; **no reader parse** of Entry/GUID/String streams |
| Writer named map | **Stub** only (`production.rs` / `lib.rs` build named heap stub) |
| Attach extract | Standard tags + stream; `stream_available`; method enum |
| Writer non-portable methods | `method != BY_VALUE && != EMBEDDED_MSG` → **`ATTACH_METHOD_UNSUPPORTED`**, **`Ok(None)` — attach row omitted** (ghost risk for web-ref) |
| `is_attach_incomplete` (0083) | `!stream_available` / fail-severity pre-bound; **explicit cloud ceiling** |
| `export_attachments.csv` header | `source_id,…,filename,size,attach_method,reason_code,severity,…,message_subject` — **no cloud URL/provider columns** |
| `fidelity_contract_v1` | `cloud_modern_attachments`, `PidNameAttachmentProviderType`, `named_properties` = DroppedByDesign |
| Mode A promote | Live when flag on; will consume incomplete extension |
| Deep preflight (0074) | Probe stream; does not resolve named props |
| Body HTML link scan | **None** |

### 2.4 Dependency currency (re-queried crates.io 2026-07-29)

No dependency bumps required unless High/Critical advisory (0081 security override).

| Dep | Lock | crates.io max | 0084 |
|---|---|---|---|
| clap | 4.6.4 | 4.6.4 | KEEP |
| rusqlite | 0.40.1 | 0.40.1 | KEEP |
| uuid | 1.24.0 | 1.24.0 | KEEP |
| camino | 1.2.5 | 1.2.5 | KEEP |
| thiserror | 2.0.19 | 2.0.19 | KEEP |
| serde_json | 1.0.151 | 1.0.151 | KEEP |
| eframe | 0.34.2 | 0.35.0 | DECLINE_MAJOR |
| reqwest | 0.12.28 (+0.13 residual) | 0.13.4 | DECLINE_MAJOR |
| sha2 / md-5 / rand / aes-gcm / argon2 / tantivy | post-0081 pins | as prior | KEEP |

No new crates for UUID parsing / GUID — use existing `uuid` workspace dep if needed for property-set GUIDs.

### 2.5 Locked product rules

1. **Sources read-only.** Never mutate source PSTs.
2. **Offline only.** No HTTP(S) fetch of OneDrive/SharePoint content. Detection ≠ hydration. Runbook: *we do not download the file; we detect, ledger, and preserve pointer metadata so the payload can be requested via supplemental / native collection.*
3. **Never invent attach payload bytes** for cloud links. Do not write fake by-value data streams.
4. **Never silently mark cloud as `Preserved`.** After this track: either **Detected incomplete / known_gap** with reason, or still-unknown with explicit residual — never “looks like a small successful attach.”
5. **Reader-first scope + pointer preserve (not full named-prop encyclopedia).**
   - NPMAP parse + attach-side resolution is P0.
   - **Ledger always carries** `cloud_provider` / `cloud_url` when known (actionability).
   - **Unique-PST:** detected CloudLink attaches must **not vanish** as “no attach row” when the source had an attachment-table row. Prefer a **metadata/pointer attach row**: original/web-ref method (or honest method encoding), filename, size without inventing binary, **best-effort URL/path string on classic tags** already in the writer’s attach PC when available. Full re-emit of arbitrary named props / complete NPMAP writer remains residual (**D-0084-cloud-named-prop-write**).
6. **Bounded allowlist.** Resolve only properties needed for cloud/modern **attachment-table** detection (+ tests). Full MS-OXPROPS dump is residual.
7. **Incomplete integration:** when attachment-table cloud/modern attach detected without exportable by-value/embedded payload → treat as **attach-incomplete** for Mode A (`is_attach_incomplete` true) and fail-severity ledger reason (see §2.7).
8. **Reason code (fixed public string):** `ATTACH_CLOUD_LINK` (severity fail). Histogram + CSV. Do not overload `ATTACH_METHOD_UNSUPPORTED` alone when cloud is positively classified (method may still look like by-value with empty data, or web-ref — prefer the cloud reason when CloudLink).
9. **fidelity_contract honesty:**
   - `cloud_modern_attachments`: **KnownGap** / **BestEffort** with reason *“attachment-table web-ref link detected; payload not collected offline; body-only inline links not scanned (D-0084-body-cloud-links)”* — **not** `Preserved`. Do **not** imply full modern-attach coverage.
   - `PidNameAttachmentProviderType`: **BestEffort** or **KnownGap** once readable when present; absence is not a defect.
   - `named_properties` generic: remain DroppedByDesign / BestEffort for “full set” — do not claim full map write.
10. **No new exit integers.** Cloud incomplete contributes to attach fails / exit 64 like other soft attach fails.
11. **No new `export_risk` enum values** unless existing vocabulary already covers (prefer attach ledger + partial fidelity).
12. **Synthetic fixtures in git.** Build minimal PST with NPMAP entries via `pst-writer` test helpers or handcrafted bytes; never commit client PSTs.
13. **No `unwrap`/`expect` in production** — `miette` + `Result`.
14. **Degrade gracefully:** missing/corrupt NPMAP → empty resolution map; classic tag heuristics still run; do not hard-fail open of entire PST solely for NPMAP parse failure (log/metric + continue).
15. **Mode A interaction:** document that `--promote-on-attach-fail` can now prefer a peer with physical attaches over a link-only winner when both share a keep-set group (identity levels still not attach-content).
16. **Attachment-table scope only.** Classification hooks run on **Attachment Table / attach PC** rows. Cloud URLs pasted into HTML/plain body without a MAPI attachment object are **out of detection scope** (honesty clause + residual **D-0084-body-cloud-links**). Closing D-0080-cloud-attachments means *named-prop attach-table detection exists*, not *all modern-attachment shapes are covered*.
17. **Independent OR signals (intentional).** Named-prop provider hit **and** non-portable attach method + no payload are **independent OR’d** signals (precedence in §2.7.2). Do **not** “simplify” to named-prop-only — third-party cloud add-ins may use ATTACH_BY_WEBREFERENCE without populating Microsoft’s `AttachmentProviderType` values.
18. **CSV schema append only.** New ledger columns append to the right of the fixed 0073 header; do not reorder existing columns (same discipline as 0075/0081/0082 appends). Empty cells when not cloud.

### 2.6 Deferred roll-in matrix

| ID | Disposition in 0084 | Why |
|---|---|---|
| **D-0080-cloud-attachments** | **Ship / close** (detect + declare + incomplete + ledger actionability + pointer preserve; not hydrate; **attach-table only**) | Core deliverable; body-inline residual opened |
| **D-0068-04** named-prop residual | **Partial close** | Reader resolve allowlist; full set + writer encyclopedia remains residual |
| **0083 cloud ceiling** | **Lift for detected attachment-table cases** | Extend `is_attach_incomplete` when cloud signal present without payload |
| **D-0073-eml** | **Decline** (full CSV) | Optional: same reason code if eml path shares materialize — not DoD-blocking |
| **D-0076-attach-content** | **Decline** | Identity hashing attach bytes is a different product surface; Mode A group-fracture risk |
| **D-0079-deterministic-key** | **Decline** | Product record-key change |
| Full calendar PidLid set / Location | **Decline** | Tempting but expands scope; residual calendar polish |
| Network hydration / Graph download | **Decline permanently here** | Offline invariant |
| Body-only / inline paste SharePoint URLs | **Open residual D-0084-body-cloud-links** | Different parser surface (HTML body scan); Purview-shaped; not NPMAP |
| Full allowlist named-prop re-emit on unique-PST | **Open residual D-0084-cloud-named-prop-write** | Pointer preserve via classic tags + ledger is in-scope; full NPMAP write is not |
| Vendor “Reconstruction-Grade” vocabulary in runbook | **Decline** | Not settled industry standard (draft / vendor); informal notes only |

### 2.7 Design sketch (normative)

#### 2.7.1 NPMAP reader (`pst-reader`)

```text
open PST → read node 0x61 PC → parse GUID stream + string stream + entry stream
         → NameIdMap { resolve(guid, lid|name) -> Option<u16 npid> }
         → reverse lookup: npid -> NamedPropKey (for debugging)
```

- Cache map per `PstFile` open (parse once).
- Public API shape (illustrative):

```rust
pub struct NamedPropKey { pub guid: [u8; 16], /* or uuid::Uuid */ pub kind: NamedPropId }
pub enum NamedPropId { Lid(u16), Name(String) }

impl PstFile {
    pub fn name_id_map(&mut self) -> Result<&NameIdMap, PstError>;
}
```

- **Phase 0 lock for primary signal:** resolve **GUID** `{96357F7F-59E1-47D0-99A7-46515C183B54}` + **name** `"AttachmentProviderType"` via `NamedPropId::Name` — **not** `Lid`.
- Unit tests: empty map; synthetic map with one string-named entry; corrupt entry degrades.

#### 2.7.2 Attach cloud classification (attachment table only)

```text
fn classify_attach(...) -> AttachKind {
  ClassicByValue | EmbeddedMsg | CloudLink {
    provider: Option<String>,  // open string (OneDrivePro / OneDriveConsumer / unknown)
    url: Option<String>,       // best-effort from allowlist / classic path props
  } | Unknown
}
```

Signals (**OR**, document order of precedence) — **intentional independence** (review fold-in):

1. Named prop allowlist hit (`AttachmentProviderType` or locked co-props) **and** no usable binary payload stream → CloudLink; `provider` = string value when present.
2. Attach method in non-portable / web-reference set (already failing today as METHOD_UNSUPPORTED) refined to CloudLink when method alone + no payload is locked in Phase 0 as cloud-shaped (even if named prop absent — third-party providers).
3. **Conservative fallback (optional, Phase 0 gate):** empty/missing data binary + pathname/URL-shaped string — only if false-positive rate acceptable on fixtures; otherwise residual.

CloudLink without payload → `stream_available = false` **or** explicit `is_cloud_link` flag that `is_attach_incomplete` honors (prefer explicit flag so parents_only semantics stay clean).

#### 2.7.3 Ledger / Mode A / contract (actionable CSV)

- Reason: **`ATTACH_CLOUD_LINK`** on fail-severity rows (prefer over bare `ATTACH_METHOD_UNSUPPORTED` when CloudLink classified).
- Summary histogram includes the code.
- **`export_attachments.csv` schema append** (fixed header constant + `AttachLedgerRow` + docs):

  | New column | When filled | Notes |
  |---|---|---|
  | `cloud_provider` | CloudLink with provider string | e.g. `OneDrivePro`; empty otherwise |
  | `cloud_url` | CloudLink with known URL/path string | Operator hit-list for native Purview / SharePoint collection; empty if unknown |

- Joinability preserved: existing columns unchanged; new columns always present (may be empty).
- CSV injection neutralization applies to free-text URL cells (0073 rule).
- `is_attach_incomplete`: true if any attach is CloudLink without payload **or** existing rules.
- Contract updates per rule 9; tests flip expectations in `unique_pst_qc_0080` / fidelity_contract unit tests.
- Mode A test: incomplete cloud peer0 + complete by-value peer1 → promote when flag on.

#### 2.7.4 Pointer preserve on unique-PST (anti–ghost attach)

**Problem (review):** Today non-BY_VALUE methods return `Ok(None)` with no attach table row. Detecting cloud + ledgering alone still leaves opposing counsel looking at a message that **lost** the attachment object and may lack any URL pointer in the deliverable PST.

**In-scope remedy (not hydration, not full named-prop writer):**

1. When CloudLink is classified and the message is materialised to unique-PST:
   - Prefer writing an **attachment table row + attach PC** with honest metadata (filename, method encoding for web-ref / original method when representable, size without inventing binary stream contents).
   - When a URL/path string is known, write it on the **best classic string tag(s)** the writer already supports for attach PCs (e.g. long filename / pathname path — Phase 0 picks the tag that survives Outlook/reader inspection without inventing named-prop streams).
2. Still emit **`ATTACH_CLOUD_LINK`** fail-severity ledger + incomplete (payload not collected).
3. Do **not** write fake `PidTagAttachDataBinary` bytes.
4. If Phase 0 proves classic-tag URL placement is insufficient for counsel review, implementer documents residual **D-0084-cloud-named-prop-write** with evidence; **ledger URL remains mandatory** either way.
5. Runbook language (mandatory): *Offline-only: we do not download cloud file bytes. We detect attachment-table web-reference cloud attaches, mark them incomplete for Mode A, ledger provider/URL for native re-collection, and preserve pointer metadata on the unique-PST when available so the deliverable is not a silent empty attach.*

#### 2.7.5 Body-only / inline cloud links (honesty + residual)

**Out of detection scope for 0084.** Many M365 users paste SharePoint/OneDrive URLs into the HTML body without creating an Attachment Table row. Microsoft Purview’s cloud-attachment collection path is largely **body-link oriented** (HTML only; documented caps). That surface is:

- **Not** NPMAP / attach-PC classification.
- **Not** closable by DoD-2 as written for attach-table fixtures.

**Mandatory honesty (docs + contract reason text):**

> This tool detects modern/cloud attachments using **structural MAPI attachment properties** (Attachment Table + allowlisted named props / method signals). Cloud URLs pasted directly into the message body (inline links) without generating MAPI attachment objects are **not** classified as `ATTACH_CLOUD_LINK` and must be handled by downstream review platforms or a future body-scan residual (**D-0084-body-cloud-links**).

Closing **D-0080-cloud-attachments** = named-prop attach-table detect exists. Opening **D-0084-body-cloud-links** keeps DoD-5 honesty: no silent claim that “cloud attach detection is complete.”

#### 2.7.6 Non-goals

- Download SharePoint/OneDrive files / Graph auth.
- Full named-prop write encyclopedia on production unique-PST.
- HTML/plain body URL extraction (residual).
- Resolving every PidLid on calendar/contact items.
- Claiming cloud payload preserved.
- Citing unsettled vendor draft standards as industry vocabulary in operator docs.

### 2.8 Affected crates / docs

| Path | Change |
|---|---|
| `crates/pst-reader` | NPMAP parse; resolve API; attach classification hooks; CloudLink metadata |
| `crates/dedup-engine` | Incomplete predicate; materialize flags; reason plumbing; optional cloud fields on canonical attach |
| `crates/pst-dedup-cli` | Ledger reason + **cloud_provider/cloud_url columns**; fidelity contract; QC tests; summary counters |
| `crates/pst-writer` | Fixture NPMAP helpers; **CloudLink pointer-row path** (metadata/no binary); production stub may stay for full named map |
| `docs/unique-pst-export.md` | ATTACH_CLOUD_LINK; CSV columns; Mode A; attach-table scope |
| `docs/unique-pst-ediscovery-runbook.md` | Offline honesty; no hydration; pointer preserve; body-link residual; counsel disclosure |
| `docs/pst-writer-fidelity-v1.md` / reader notes | Named map read; cloud pointer write |
| `docs/deferred.md` | Close D-0080-cloud-attachments; narrow D-0068-04; open D-0084-body-cloud-links + D-0084-cloud-named-prop-write |
| CHANGELOG `[Unreleased]` | Tier-1 |

### 2.9 Product decisions locked

| # | Decision | Default |
|---|---|---|
| Q1 | Parse NPMAP `0x61` | **Yes** |
| Q2 | Hydrate cloud payloads from network | **No** |
| Q3 | Cloud without payload → incomplete + `ATTACH_CLOUD_LINK` | **Yes** |
| Q4 | Full named-prop writer encyclopedia | **Out** (residual D-0084-cloud-named-prop-write) |
| Q5 | Full MS-OXPROPS coverage | **Out** — allowlist only |
| Q6 | Hard-fail PST open if NPMAP corrupt | **No** — degrade |
| Q7 | Enable D-0076-attach-content | **Out** |
| Q8 | Claim cloud payload `Preserved` | **Never** |
| Q9 | Body-only / inline paste URL detection | **Out** — residual D-0084-body-cloud-links + honesty clause |
| Q10 | Ledger `cloud_url` / `cloud_provider` columns | **Yes** — mandatory when known |
| Q11 | Pointer/metadata attach row on unique-PST for CloudLink | **Yes** — prefer non-vanishing; no invented binary |
| Q12 | Named-prop-only classification (drop method fallback) | **No** — keep independent OR signals |

### 2.10 Dual-AI review disposition (2026-07-29)

| # | Claim | Disposition | Spec landing |
|---|---|---|---|
| A1-1 | Ghost attach: detect-only without URL pointer is spoliation-adjacent | **Agree (scoped)** | §2.5 r2–r5, §2.7.4; DoD-4b; residual named-prop write |
| A1-2 | CSV needs URL/provider for operator hit-list | **Agree** | §2.7.3; DoD-4; schema append |
| A1-3 | Inline body links not covered; honesty required | **Agree** | §2.5 r16, §2.7.5; DoD-5; residual D-0084-body-cloud-links |
| A2-1 | Body-only modern attach is common; Purview body-scan shape; don’t over-claim D-0080 close | **Agree** | Same as A1-3; DoD-2 language narrowed |
| A2-2 | GUID+name resolve; open string provider; OR signals intentional | **Agree** | §2.2 table; §2.7.1–2; Q12; plan Phase 0 |
| A2-3 | Don’t elevate Cloudficient draft as Sedona-class vocabulary | **Agree** | §2.2 item 7; deferred matrix decline |

---

## 3. In scope

1. **NPMAP reader** for `NID_NAME_TO_ID_MAP` with resolve API + tests (GUID+LID and GUID+name).
2. **Allowlisted** named-prop resolution on attach PCs for cloud/provider detection (`AttachmentProviderType` minimum).
3. **Classification** + incomplete integration for Mode A (attachment-table CloudLink).
4. **Ledger** reason `ATTACH_CLOUD_LINK` + histogram + **`cloud_provider` / `cloud_url` columns**.
5. **Pointer preserve** on unique-PST for CloudLink (metadata/pointer row; no invented binary).
6. **fidelity_contract** honesty update (no silent preserved; attach-table scope; body residual named).
7. **Docs** + deferred close for D-0080-cloud-attachments; open body-link + named-prop-write residuals.
8. **Tests:** NPMAP unit; synthetic cloud attach fixture; Mode A promote prefers physical peer; ledger columns; contract tests; workspace gate.
9. **Graceful degrade** without NPMAP.
10. **Honesty clause** in runbook + contract for body-only inline links.

## 4. Out of scope (do NOT do here)

- Network download / Graph / authenticated SharePoint access.
- **HTML/plain body URL extraction** (D-0084-body-cloud-links).
- Full unique-eml ledger CSV (**D-0073-eml**).
- Deterministic store key (**D-0079-deterministic-key**).
- **D-0076-attach-content** body-recip-attach identity.
- Full writer named-prop encyclopedia / all PidLids (calendar Location, etc.) — residual **D-0084-cloud-named-prop-write** for allowlisted re-emit if classic tags prove insufficient.
- Outlook COM; new exit codes; eframe major.
- Changing keep-set grouping identity to include cloud URL hashes (product later if needed).
- Adopting unsettled vendor draft eDiscovery vocabulary as product runbook standards.

## 5. Preconditions & dependencies

- **P1:** 0080 contract + QC harness present.
- **P2:** 0083 `is_attach_incomplete` + Mode A path present.
- **P3:** 0073 attach ledger reason taxonomy extensible; CSV header append pattern known (0075/0081/0082).
- **P4:** Writer or test helper can synthesize NPMAP + attach PC for fixtures (or byte-level fixture).
- **P5:** Writer path for non-BY_VALUE methods currently omits rows — implementer must branch CloudLink pointer path without inventing binary.
- *Verified to date (2026-07-29):* `NID_NAME_TO_ID_MAP` constant only; no parse; cloud contract DroppedByDesign; crates.io pins KEEP; PSETID_Attachment GUID + AttachmentProviderType name verified on Learn; Purview cloud-attach article documents body-link collection limits (updated 2026-06-11).

## 6. Risks

| Risk | Mitigation |
|---|---|
| Wrong GUID/name → false negative | Phase 0 re-verify MS-OXPROPS + fixture; residual false-neg documented |
| False positive cloud on classic empty attach | Conservative signals; tests; prefer named prop over URL heuristic |
| NPMAP parse bugs break all opens | Isolate parse; degrade on error; never unwrap |
| Scope creep to full named props | Allowlist DoD; calendar PidLid residual; named-prop-write residual |
| Mode A group split still if identity differs | Document; no attach-content enable |
| Operators expect download | Runbook offline honesty |
| Ghost attach if ledger-only | DoD-4b pointer preserve; test non-vanishing row when source had attach |
| Over-claim “cloud detection complete” | Attach-table DoD language; body residual; contract reason text |
| URL only in named props, classic tags empty | Ledger URL mandatory; residual D-0084-cloud-named-prop-write |
| Third-party cloud without AttachmentProviderType | Independent OR signals (rule 17) |

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — NPMAP:** Reader loads `0x61` Name-to-ID-Map; resolves allowlisted (GUID, name\|LID) → NPID including **PSETID_Attachment + `"AttachmentProviderType"`** via name branch; unit tests for hit/miss/degrade.
- [ ] **DoD-2 — Detect (attachment-table):** Synthetic (or fixture) **attachment-table** cloud/modern web-reference attach is classified as CloudLink when provider named prop (or locked allowlist / method signal) present without exportable payload. DoD language does **not** claim body-only coverage.
- [ ] **DoD-3 — Incomplete:** Such attaches make `is_attach_incomplete` true; Mode A with complete physical peer promotes when flag on (test).
- [ ] **DoD-4 — Ledger actionability:** `ATTACH_CLOUD_LINK` on fail rows + histogram; CSV joinable; **`cloud_provider` and `cloud_url` columns** appended and populated when known (empty otherwise); injection neutralization applies.
- [ ] **DoD-4b — Pointer preserve:** CloudLink materialisation does **not** silently omit the attachment object when the source had an attachment-table row: metadata/pointer attach row written without inventing binary payload; best-effort classic URL/path string when known. Residual D-0084-cloud-named-prop-write documented if Phase 0 proves classic tags insufficient.
- [ ] **DoD-5 — Contract honesty:** `cloud_modern_attachments` / provider prop entries no longer imply silent blind spot as “cannot detect attachment-table cloud”; status/reason updated honestly (**not** Preserved for payload); **explicit attach-table scope + body-inline residual** in contract reason and runbook.
- [ ] **DoD-6 — No hydration:** No network client added for attach download; offline invariant holds.
- [ ] **DoD-7 — Degrade:** Corrupt/missing NPMAP does not hard-fail entire PST open; documented behavior.
- [ ] **DoD-8 — Docs + deferred:** unique-pst-export + eDiscovery runbook (offline; no hydration; pointer preserve; body residual; Mode A benefit) + fidelity doc; **D-0080-cloud-attachments closed** (attach-table detect); **D-0068-04 narrowed**; **D-0084-body-cloud-links** and **D-0084-cloud-named-prop-write** opened as needed; CHANGELOG `[Unreleased]`.
- [ ] **DoD-9 — Deps:** No unapproved majors (default none).
- [ ] **DoD-10 — Tests gate:** fmt / clippy `-D warnings` / workspace test / deny green.
- [ ] **DoD-11 — Recorded:** `review.md` (include review fold-in disposition + intentional OR signals) + board Completed + ledger `FEATURE`.

## 8. Verification commands (reference)

```powershell
cargo test -p pst-reader -- named
cargo test -p pst-reader -- npmap
cargo test -p dedup-engine -- incomplete
cargo test -p dedup-engine -- cloud
cargo test -p pst-dedup-cli -- cloud
cargo test -p pst-dedup-cli -- fidelity
cargo test -p pst-dedup-cli -- promote
cargo test -p pst-dedup-cli -- attach_ledger
# assert export_attachments.csv header includes cloud_provider,cloud_url

cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
ledgerful verify
```
