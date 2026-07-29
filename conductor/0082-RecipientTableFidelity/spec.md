# 0082 — Recipient Table Fidelity

> Structure follows `C:\dev\coordinated\conductor\templates\0000-Description\spec.md`.
> Expanded subsections under §2–§3 are normative design for implementers. DoD is §7.

- **Track ID:** 0082-RecipientTableFidelity
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → Series M (unique-export fidelity residuals) after Series L close
- **Cross-repo contract:** n/a
- **Status:** Completed
- **Depends on:** 0068 · 0069 · 0071 · 0076 · 0078 · 0080 · 0081 (all **Completed** on board)
- **Spec authored:** 2026-07-29
- **Revised 2026-07-29:** dual-AI review fold-in — EX/LegacyExchangeDN identity tier; BCC-suppress ledger column; zero-recip vs draft anomaly telemetry; DL non-expansion honesty; MS-PST template **0x692** + full 14-column MUST set verified
- **Series:** M (Unique export fidelity residuals)

---

## 1. Objective

Ship **real MAPI recipient tables** end-to-end on the unique-export path: **read** structured recipients (SMTP **and** EX/LegacyExchangeDN + `PidTagRecipientType`) from source PSTs, **use** them for Tier-2.5 identity (closing display-name / X.500 variance on the messages that actually motivate this track), and **write** per-message recipient TCs into production unique-PSTs — replacing the 0080 stopgap of display strings only — with a locked BCC disclosure policy, an audit trail when BCC is suppressed on write, and a small automation roll-in (`retryable` on summary JSON).

## 2. Context (read before starting)

### 2.1 Why this track exists now

Series L (0073–0081) closed integrity, keep-set, attach ledger, CRC/`export_risk`, exit codes, perf, source-differential QC, and operator docs. The single highest-value **open fidelity residual** that three independent deferreds already point at is the recipient table:

| Deferred | Severity | Claim |
|---|---|---|
| **D-0080-recipient-table** | P2 | Written PSTs have **no** recipient TC rows; `PidTagDisplayCc` only |
| **D-0076-recipient-table** | P2 | Tier-2.5 hashes **display** strings → X.500 / name-order variance |
| **D-0068-04** residual | — | Recipient table + named-prop set; attach half closed in 0069 |
| **D-0018-03** | — | Extract path still Display* only |

`fidelity_contract_v1` today marks `recipient_table` as `DroppedByDesign` with reason pointing here. MS-PST states a Recipient Table **MUST exist for any Message object**.

### 2.2 Industry / protocol anchors (researched 2026-07-29)

**MS-PST Recipient Table** ([Recipient Table](https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/0e6d7ebd-c850-4772-ba9d-f5a642c9ff85); [Recipient Table Template](https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/bb069b2b-80ad-46d5-b86f-33487d16bf0c); access date **2026-07-29**):

- Standard **Table Context (TC)** with `NID_TYPE_RECIPIENT_TABLE` (**0x12** — already in `pst-reader` `NidType::RecipientTable`).
- Per-message table lives in the **message subnode**.
- **Store template NID:** `NID_RECIPIENT_TABLE` = **`0x692`** (verified live — **not** `0x671`, which is the attachment table template). Template MUST have **no data rows**.
- **One row per recipient**; **MUST** exist for any Message object.

**Template MUST-have columns (MS-PST § Recipient Table Template — full set, not a subset):**

| Property identifier | Type | Friendly name |
|---|---|---|
| `0x0C15` | PtypInteger32 | `PidTagRecipientType` |
| `0x0E0F` | PtypBoolean | `PidTagResponsibility` |
| `0x0FF9` | PtypBinary | `PidTagRecordKey` |
| `0x0FFE` | PtypInteger32 | `PidTagObjectType` |
| `0x0FFF` | PtypBinary | `PidTagEntryId` |
| `0x3001` | PtypString | `PidTagDisplayName` |
| `0x3002` | PtypString | `PidTagAddressType` |
| `0x3003` | PtypString | `PidTagEmailAddress` |
| `0x300B` | PtypBinary | `PidTagSearchKey` |
| `0x3900` | PtypInteger32 | `PidTagDisplayType` |
| `0x39FF` | PtypString | `PidTag7BitDisplayName` |
| `0x3A40` | PtypBoolean | `PidTagSendRichInfo` |
| `0x67F2` | PtypInteger32 | `PidTagLtpRowId` |
| `0x67F3` | PtypInteger32 | `PidTagLtpRowVer` |

**Extra column (product, not in template MUST set):** `PidTagSmtpAddress` (`0x39FE`) — written/read when known; the design prefers it for identity but it is **additive** to the 14-column baseline, not a substitute for it.

**Write-side synthesis defaults for structural columns** (when source omits them — mirror attach-table EntryID/`RecordKey` patterns in `production.rs` where applicable):

| Column | Conventional default when synthesizing |
|---|---|
| `PidTagObjectType` | `MAPI_MAILUSER` = **6** |
| `PidTagResponsibility` | `true` |
| `PidTagSendRichInfo` | `false` (safe default) |
| `PidTagDisplayType` | `0` (DT_MAILUSER) unless source says otherwise |
| `PidTagRecordKey` / `PidTagEntryId` / `PidTagSearchKey` | Derive per existing writer patterns; never leave template columns absent |

**eDiscovery practice (robust default):**

1. **Processing identity** uses structured addresses when available — SMTP first, then stable EX/LegacyExchangeDN, **not** display-only for on-prem Exchange rows that lack SMTP.
2. **Production / deliverable disclosure** of BCC is a **policy** choice — default suppress in the written artifact; opt-in for full-fidelity copies under counsel instruction. When suppress is active, the **ledger must explain** visual near-duplicates (see §2.5 rule 7).
3. **New Outlook** (default client since April 2026): still **no COM**; PST remains a correct *deliverable* (0080/0081). Durable proof stays **source-differential reader QC + optional scanpst-on-copy**, not client automation.
4. **No DL expansion:** fidelity is to the **PST file**, not to live Exchange group membership. If the source stored only a DL display name / EX address without expanded members, the unique-PST **replicates that** and does **not** resolve membership (runbook honesty clause).

### 2.3 Live code snapshot (verified 2026-07-29)

| Surface | State |
|---|---|
| `pst-reader` `Message` / extract | `display_to` / `display_cc` / `display_bcc` only — **no** recipient TC walk |
| `NidType::RecipientTable` (0x12) | Enum exists; **no** reader path loads it |
| `WriteMessage` / production write | `display_to` + `display_cc` props; **no** recipient subnode TC |
| Attachment table pattern | Template NBT `0x671` + per-message subnode TC + RowIndex — **TC machinery to reuse**; recipient template is **`0x692`** |
| `dedup-engine` Tier-2.5 | `normalize_recipients` / `recipient_has_x500` on **display** strings; stats `tier2_5_splits_recipients_only`, `x500_recipient_items` |
| `fidelity_contract_v1` | `recipient_table` = `DroppedByDesign` |
| `CanonicalMessage.display_bcc` | Exists; dropped on write; counted as dropped_by_design (0080) |
| `PidTagMessageFlags` (`0x0E07`) | Confirm whether reader already surfaces flags; needed for zero-recip vs draft anomaly (§2.5 rule 8) |

### 2.4 Dependency currency (re-queried crates.io 2026-07-29 — post-0081)

0081 already applied safe PATCH/MINOR pins. **No dependency bumps are required for this track** unless a High/Critical advisory appears on the locked version (0081 security override rule).

| Dep | Lock (post-0081) | crates.io max | Decision for 0082 |
|---|---|---|---|
| clap | 4.6.4 | 4.6.4 | KEEP |
| serde_json | 1.0.151 | 1.0.151 | KEEP |
| thiserror | 2.0.19 (+1.x dual) | 2.0.19 | KEEP |
| camino | 1.2.5 | 1.2.5 | KEEP |
| uuid | 1.24.0 | 1.24.0 | KEEP |
| rusqlite | 0.40.1 | 0.40.1 | KEEP |
| sha2 | 0.11.0 (+0.10.9 dual) | 0.11.0 | KEEP / ACCEPT_DUAL |
| md-5 | 0.10.6 product | 0.11.0 | KEEP product pin |
| eframe | 0.34.2 | 0.35.0 | DECLINE_MAJOR (mid-RC) |
| reqwest | 0.12.28 (+0.13 residual) | 0.13.4 | DECLINE_MAJOR |
| aes-gcm | 0.10.3 | 0.11.0 | DECLINE_MAJOR |
| argon2 | 0.5.3 | 0.6.0-rc.8 | KEEP stable |
| rand | 0.8.7 / 0.9.5 / 0.10.2 | 0.10.2 | KEEP (RUSTSEC-2026-0097 floors met) |
| tantivy | 0.26.1 | 0.26.1 | KEEP |

Re-query crates.io at implement if >7 days after this date. Do not open a dep-only sub-scope here.

### 2.5 Locked product rules

1. **Sources remain read-only.** Never mutate source PSTs.
2. **MS-PST: every written message gets a recipient TC subnode** (may be **zero rows** if source had none / unreadable — still emit empty TC so the MUST is satisfied). Template at **`0x692`** with the **full 14-column** MUST set (§2.2); `PidTagSmtpAddress` is an optional extra column when known.
3. **Read before write before identity consumption** — order is intentional (extraction → writer → Tier-2.5). Do not hash fields the reader cannot yet emit.
4. **Identity key cascade (per recipient row, then sort+join for fingerprint):** when building Tier-2.5 recipient fingerprint, for each row pick **exactly one** key in this order:
   1. `PidTagSmtpAddress` if non-empty  
   2. `PidTagEmailAddress` if `PidTagAddressType` is SMTP (or address is clearly SMTP-shaped **and** type is missing/empty)  
   3. `PidTagEmailAddress` if `PidTagAddressType` is **EX** (or other X.500 / LegacyExchangeDN form — normalize case; keep full `/O=…` path; **do not** drop to display)  
   4. Normalized **display** fallback only when no structured address key exists  
   Document precedence in code + docs. **Rationale:** on-prem / migrated Exchange rows often have EX DN in `PidTagEmailAddress` and **no** `PidTagSmtpAddress`; the old cascade fell through to noisy display and failed the track's own problem statement.
5. **BCC disclosure (write path) — default OFF:**
   - Default: write **To + Cc** rows only; do **not** write Bcc rows or `PidTagDisplayBcc`.
   - Opt-in: `--include-bcc-recipients` writes Bcc rows + `PidTagDisplayBcc` when present.
   - Rationale: unique-PST consolidates custodians; BCC on a deliverable can over-disclose relative to a single custodian's outward view (same class as 0047 visible-only default and 0080 `dropped_by_design`).
6. **BCC in identity (hash path) — default ON when table present:** To+Cc+**Bcc** addresses participate in Tier-2.5 so copies that differ only by BCC do not false-merge. Identity is internal, not a disclosure surface.
7. **BCC-suppress audit trail (mandatory when rule 5 default applies):** because rule 6 can keep two messages unique while rule 5 strips the visual difference, `export_messages.csv` MUST include a boolean column **`bcc_suppressed`** (name fixed):
   - `true` when the source had one or more Bcc recipients (table row type Bcc **or** non-empty `display_bcc`) **and** the write path omitted them (`include_bcc_recipients == false`).
   - `false` otherwise (including when `--include-bcc-recipients` wrote them, or source had no BCC).
   - Summary JSON SHOULD carry aggregate `bcc_suppressed_message_count`.
   - Runbook documents: "two near-identical messages in the unique-PST with `bcc_suppressed=true` are **not** a dedupe failure — BCC variance was kept for identity and omitted from the deliverable by policy."
8. **Zero-recipient anomaly (draft vs sent):** empty recipient TC is structurally valid, but:
   - If row count is 0 **and** `MSGFLAG_UNSENT` (`PidTagMessageFlags` `0x0E07`, bit **0x00000008**) is **not** set → treat as anomaly **`sent_message_with_no_recipients`** (name fixed).
   - Record: summary counter + optional per-message note / fidelity observation — **do not** hard-fail the export; **do not** invent a new `export_risk` enum value (0077 vocabulary frozen: `ok` \| `re_export_recommended` \| `not_export_ready`).
   - Drafts (UNSENT set) with zero recipients are normal — no anomaly.
   - If flags are unreadable, skip anomaly (do not invent UNSENT).
9. **Display props stay:** continue writing `PidTagDisplayTo` / `PidTagDisplayCc` for clients that only read PC props; table is the structured source of truth.
10. **No DL / group expansion.** Replicate source table rows only. Document in runbook + unique-pst-export.
11. **No new exit integers** (0078 table frozen). Fidelity/partial still 64; risk block 65; cancel 130.
12. **Unique-export semantics otherwise freeze** (keep-set, CRC accept, attach ledger locus, export_risk vocabulary).
13. **No `unwrap`/`expect` in production paths** — `miette` + `Result`.
14. **Synthetic fixtures only in git**; real multi-mailbox smoke operator-local.
15. **fidelity_contract_v1 must stay honest:** after ship, `recipient_table` is **not** `DroppedByDesign`. Unknown contract names still fail closed.
16. **No full named-property map** (D-0080-cloud-attachments / named props remain residual).
17. **No Outlook COM** (D-0080-com-declined stands).
18. **Optional features default inert** — `--include-bcc-recipients` default false.

### 2.6 Deferred roll-in matrix

| ID | Disposition in 0082 | Why |
|---|---|---|
| **D-0080-recipient-table** | **Ship / close** | Core write deliverable |
| **D-0076-recipient-table** | **Ship / close** | Core identity deliverable |
| **D-0068-04** recipient half | **Ship / close** | Same TC work; named-prop half stays residual |
| **D-0018-03** | **Ship / close** (reader half) | Extract/readers surface structured recipients |
| **D-0080-bcc-policy** | **Decide + ship** | Locked in §2.5 rules 5–6; document in contract + runbook |
| **D-0078-retryable** | **Ship / close** | Small automation field; no new exit code; fits JSON honesty |
| **D-0073-promote** (P1) | **Decline — own track** | Mid-write Mode A promote is a different risk surface (keep-set mutation during materialize); do not piggyback |
| **D-0073-eml** | **Decline** | unique-eml ledger parity is a separate export path; schedule later if needed |
| **D-0076-attach-content** | **Decline** | Needs attach-content wire-up over 0074 probe; not recipient-shaped |
| **D-0080-cloud-attachments** | **Decline** | Requires named-prop resolution (own track) |
| **D-0079-deterministic-key** | **Decline** | Product decision; changes every PST record key / hash chain |
| **D-0079-stream-prepare** / `--jobs` | **Decline** | Perf Phase C; not fidelity |
| **D-0062-codesign** | **Decline** | Release ops |
| GUI checklist residuals (D-0073-gui, D-0078-gui, …) | **Decline** | CLI-first; Desk uses defaults |

### 2.7 Design sketch (normative)

#### 2.7.1 Reader (`pst-reader`)

```text
Message PC ──► existing display_* props (+ PidTagMessageFlags when present)
     │
     └── subnode NID_TYPE_RECIPIENT_TABLE (0x12)
              └── TC rows → Vec<Recipient>
```

```rust
// Shape (names illustrative — match crate style)
pub struct Recipient {
    pub recipient_type: RecipientType, // To | Cc | Bcc | Other(u32)
    pub display_name: Option<String>,
    pub address_type: Option<String>,  // SMTP, EX, …
    pub email_address: Option<String>,
    pub smtp_address: Option<String>,  // PidTagSmtpAddress when present (extra col)
    // Optional binary props if cheap to carry for writer round-trip:
    // entry_id, record_key, search_key — else re-synthesize on write
}

impl Recipient {
    /// Identity key for Tier-2.5 (§2.5 rule 4) — SMTP → EX DN → display.
    pub fn identity_key(&self) -> Option<String> { /* … */ }
}
```

- Missing/unreadable table → `recipients: Vec::new()` + existing display_* still populated (no hard fail on open).
- Do not invent recipients from display strings on the **reader** path (display → synthetic rows would invent structure). Writer may keep display props independently.
- Surface enough of `PidTagMessageFlags` (or a `is_unsent` / flags bits helper) for rule 8 anomaly detection.

#### 2.7.2 Writer (`pst-writer`)

- Emit **recipient table template** at **`NID_RECIPIENT_TABLE = 0x692`**, zero rows, **all 14 MUST columns** (§2.2). Reuse TC / RowIndex helpers from attach template (`0x671`) machinery — different NID and column set.
- Per message: subnode TC, one row per **included** recipient; empty TC always present.
- Populate every template column (synthesize structural fields per §2.2 table when source lacks them).
- **Extra:** write `PidTagSmtpAddress` (`0x39FE`) when known (does not remove any MUST column).
- Gate Bcc rows on `include_bcc_recipients` (from CLI → `WriteMessage` / production options).
- Empty table still written (zero rows).

#### 2.7.3 Identity (`dedup-engine`)

- When `msg.recipients` non-empty: build Tier-2.5 fingerprint from each row's **`identity_key()`** (§2.5 rule 4) over **To+Cc+Bcc**, sorted + normalized (case-fold SMTP; preserve EX path structure; extend `normalize_recipients` or add `normalize_recipient_identity_keys`).
- When empty: **keep today's display-string path** (no silent behavior change for old fixtures without tables).
- Stats: keep `x500_recipient_items`; prefer counting table-sourced EX keys as X.500 (not only display `/O=` heuristics). Optional: `recipient_table_items`, `recipient_table_smtp_items`, `recipient_table_ex_items`.
- **DoD-5 synthetic cases must include an EX-only pair** (no `PidTagSmtpAddress`) that merges under EX keys but would split under display noise — not only SMTP fixtures.

#### 2.7.4 CLI / contract / QC / ledger

- Flag: `--include-bcc-recipients` (default false) on `unique-pst` (and any shared args struct used by GUI).
- **`export_messages.csv`:** add **`bcc_suppressed`** boolean column (§2.5 rule 7); stable trailing or documented position; tests for true/false.
- Summary: `bcc_suppressed_message_count`; `sent_message_with_no_recipients_count` (rule 8).
- `fidelity_contract_v1`: `recipient_table` → `Preserved` (empty source → empty table is still structured fidelity). BCC rows remain `DroppedByDesign` unless flag; document static contract + flag interaction.
- Optional contract observation / reason note for DL non-expansion is **docs-only** (not a new DroppedByDesign that implies we deleted expanded members we never had).
- Source-differential QC (0080): sample at least one message with multi-type recipients; assert table row counts/types vs source when source had a table (compare **written** set: default To+Cc only).
- **D-0078-retryable:** add `retryable: bool` to the unique-export **summary JSON** (and any typed summary struct). Classification:
  - `true` only for clearly transient IO / cancel-retry classes already identified in 0078/0081 runbook (e.g. disk full mid-write after quarantine — **not** `AuditChainBroken`, schema, passphrase, export_risk block).
  - `false` for permanent config/integrity/fidelity failures.
  - **No new exit codes.** Runbook may reference the field; 0081 "no blanket retry exit 5" stays.

### 2.8 Affected crates / docs

| Path | Change |
|---|---|
| `crates/pst-reader` | Recipient TC parse; surface on message extract |
| `crates/pst-writer` | Template + per-message recipient TC write |
| `crates/dedup-engine` | Propagate recipients through materialize / CanonicalMessage / Tier-2.5 |
| `crates/pst-dedup-cli` | Flag; fidelity contract; summary `retryable`; QC sample |
| `crates/pst-dedup-gui` | Pass-through default (flag inert unless already wired via UniquePstCliArgs) — no required new UI |
| `docs/pst-writer-fidelity-v1.md` | Recipient table row in fidelity matrix |
| `docs/unique-pst-export.md` | Flag + identity cascade (SMTP→EX→display); BCC suppress column; DL non-expansion |
| `docs/unique-pst-ediscovery-runbook.md` | BCC disclosure + `bcc_suppressed` reviewer note + retryable + DL honesty + zero-recip anomaly |
| `docs/deferred.md` | Close/roll matrix §2.6 |
| `fidelity_contract` / tests | Contract status flip + fixtures |
| `export_messages.csv` schema | `bcc_suppressed` column |

### 2.9 Product decisions locked (do not re-litigate at implement)

| # | Decision | Default |
|---|---|---|
| Q1 | Ship full recipient TC (not display-only forever) | **Yes** |
| Q2 | BCC in **written** PST | **Opt-in** (`--include-bcc-recipients`) |
| Q3 | BCC in **identity hash** when table present | **Yes** (To+Cc+Bcc) |
| Q4 | Invent recipients from Display* when table missing | **No** (display path remains fallback for hash only) |
| Q5 | Empty recipient TC on every message | **Yes** (MS-PST MUST) |
| Q6 | `retryable` JSON field | **Ship** (bool; no new exits) |
| Q7 | Mode A promote / eml ledger / named props / deterministic key | **Out of scope** |
| Q8 | Identity cascade includes **EX/LegacyExchangeDN** before display | **Yes** (§2.5 rule 4) |
| Q9 | `bcc_suppressed` ledger column when BCC omitted on write | **Yes** (§2.5 rule 7) |
| Q10 | Zero-recip non-draft anomaly telemetry (no new export_risk value) | **Yes** (§2.5 rule 8) |
| Q11 | Expand Distribution Lists | **No** — file fidelity only |
| Q12 | Template NID / columns | **`0x692` + full 14 MUST columns** (verified 2026-07-29) |

---

## 3. In scope

1. **Reader:** load per-message recipient TC → structured `Recipient` list (incl. EX/SMTP fields); message flags enough for UNSENT; unit tests with synthetic PST (writer-roundtrip and/or hand-built fixture).
2. **Writer:** template at **`0x692`** with **all 14 MUST columns** + optional `PidTagSmtpAddress`; per-message TC rows; empty-table guarantee; BCC gated by flag; structural column synthesis.
3. **Pipeline:** materialize / keep-set / unique-pst carry recipients from reader → writer.
4. **Identity:** Tier-2.5 cascade SMTP → EX DN → display (§2.5 rule 4); tests for **EX-only** and SMTP cases; table-less path unchanged.
5. **CLI:** `--include-bcc-recipients`; help text; default inert.
6. **Ledger:** `bcc_suppressed` on `export_messages.csv` + summary count; zero-recip non-draft counter.
7. **fidelity_contract_v1** honesty for `recipient_table` (+ BCC interaction docs).
8. **QC:** extend 0080 path to compare recipient structure on at least one multi-recipient sample (fixture).
9. **`retryable: bool`** on unique-export summary JSON + unit tests for true/false classification boundaries.
10. **Docs:** fidelity matrix, unique-pst-export, eDiscovery runbook (BCC suppress reviewer note, DL non-expansion, anomaly), deferred.md closes.
11. **Verification:** fmt, clippy `-D warnings`, workspace tests, deny green; `review.md` + board update.

## 4. Out of scope (do NOT do here)

- Mode A promote-on-attach-fail (**D-0073-promote** → future track).
- unique-eml attach ledger CSV parity (**D-0073-eml**).
- `--strong-content-hash body-recip-attach` (**D-0076-attach-content**).
- Named-property resolution / cloud attachments (**D-0080-cloud-attachments**).
- Full named-prop map beyond recipient template columns + `PidTagSmtpAddress`.
- **Distribution list expansion** / live GAL lookup (honesty only — §2.5 rule 10).
- Elevating zero-recip anomaly to a new `export_risk` tier or hard export failure.
- Deterministic store record key (**D-0079-deterministic-key**).
- Stream-prepare / `--jobs` / multi-GB operator soak (**D-0079-***).
- Outlook COM, codesign, eframe 0.35 major, reqwest 0.13 product major.
- New exit integers; changing CRC thresholds; keep-set policy ladder redesign.
- Desk-first UI for BCC / recipient drill-down (CLI flag + defaults only).
- Matter-schema / extract-pst NormalizedItem recipient relational model (can consume reader later; not this track's DoD).

## 5. Preconditions & dependencies

- **P1 (blocking):** Series L closed — 0073–0081 **Completed** (board).
- **P2 (blocking):** Attachment table write path (0069) available as the TC pattern to mirror.
- **P3 (blocking):** 0080 fidelity contract + QC harness present so contract flip and sample assertions land in the right place.
- **P4 (non-blocking):** 0081 runbook exists for `retryable` + BCC narrative updates.
- *Verified to date (2026-07-29):*
  - `NidType::RecipientTable = 0x12` present; no load path.
  - Writer writes `display_to`/`display_cc`; no recipient TC.
  - MS-PST template **`0x692`** + 14-column MUST set confirmed on Microsoft Learn.
  - crates.io pins match §2.4; no security override forced.
  - Accidental mangled-path dirs under repo root **absent**.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Wrong template NID or column widths → Outlook/scanpst reject | **Locked `0x692` + full 14 columns** from MS-PST; mirror attach-template tests; empty + multi-row fixtures; optional scanpst-on-copy operator smoke |
| Identity cascade skips EX → still false-merges on-prem | Rule 4 middle tier; DoD-5 **requires EX-only fixture**, not SMTP-only |
| BCC hash-on / write-off looks like "dedupe failure" to reviewers | **`bcc_suppressed` CSV + summary count + runbook language** (rule 7) |
| Empty TC masks parser failure on sent mail | Rule 8 anomaly counter; do not elevate `export_risk` vocabulary |
| Identity hash change reshuffles keep-set winners on real cases | Only when structured table present; display fallback preserves old behavior for table-less messages; document in CHANGELOG / unique-pst-export |
| BCC opt-in forgotten → over-disclosure | Default **off**; runbook + help text; contract marks BCC dropped unless flag |
| Reader invents structure from Display* | Forbidden (Q4); tests assert no synthetic rows from display-only messages |
| Expectation of DL expansion | Explicit non-goal; runbook honesty clause |
| `retryable: true` on permanent failures → automation loops | Closed vocabulary; tests for AuditChainBroken / export_risk / fidelity → `false`; runbook keeps "no blanket retry exit 5" |
| Track balloons into named props / Mode A / GAL | §4 fence |
| Dual display vs table mismatch in source | Prefer table for identity; keep display props as written labels; do not "fix" display from table unless already done elsewhere |

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Reader:** Source messages with a recipient TC expose structured recipients (type + address + address_type fields); missing table → empty vec, not hard error; tests cover both; flags available for UNSENT check (or documented skip if absent on fixture).
- [ ] **DoD-2 — Writer:** Template at **`0x692`** with **all 14 MUST columns** (zero rows); every written message has a recipient TC subnode (including zero-row); multi-recipient fixture round-trips To/Cc types and address keys; optional `PidTagSmtpAddress` when known.
- [ ] **DoD-3 — BCC write policy:** Default omits Bcc rows and `PidTagDisplayBcc`; `--include-bcc-recipients` includes them; tests for both.
- [ ] **DoD-4 — Pipeline:** unique-pst materialize path carries reader recipients into writer (not display-only dead-end).
- [ ] **DoD-5 — Identity:** Tier-2.5 uses §2.5 rule 4 cascade; **at least one synthetic EX-only (no SMTP) case** improves merge vs pure display noise; SMTP case still covered; table-less path unchanged.
- [ ] **DoD-6 — Contract:** `fidelity_contract_v1` no longer lists `recipient_table` as `DroppedByDesign`; BCC interaction documented; unknown props still fail closed.
- [ ] **DoD-7 — QC:** At least one automated assertion compares source vs output recipient structure on a multi-recipient fixture (respecting BCC write filter).
- [ ] **DoD-8 — retryable:** Summary JSON includes `retryable: bool`; unit tests lock true/false boundaries; no new exit integers.
- [ ] **DoD-9 — BCC suppress ledger:** `export_messages.csv` has `bcc_suppressed`; summary has `bcc_suppressed_message_count`; tests for true when source BCC omitted on write.
- [ ] **DoD-10 — Zero-recip anomaly:** non-draft empty table increments `sent_message_with_no_recipients_count` (or equivalent); draft empty does not; no new `export_risk` value.
- [ ] **DoD-11 — Docs:** fidelity matrix, unique-pst-export, eDiscovery runbook updated (BCC suppress reviewer note, DL non-expansion, identity cascade, anomaly); deferred rows closed per §2.6.
- [ ] **DoD-12 — Deps:** No unapproved majors; if any bump, recorded with reason (default: no bumps).
- [ ] **DoD-13 — Tests gate:** `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace`; `cargo deny check` green.
- [ ] **DoD-14 — Recorded:** `review.md` with evidence + deferred disposition + MS-PST citation dates; `../conductor.md` status **Completed**; ledger transaction committed (`FEATURE` or split `FEATURE`/`DOCS`).

## 8. Verification commands (reference)

```powershell
# Targeted during work
cargo test -p pst-reader -- recipient
cargo test -p pst-writer -- recipient
cargo test -p dedup-engine -- recipient
cargo test -p pst-dedup-cli -- recipient
cargo test -p pst-dedup-cli -- fidelity
cargo test -p pst-dedup-cli -- retryable

# Full gate before complete
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
ledgerful verify

# Optional operator (local PSTs only — never commit)
# unique-pst with --include-bcc-recipients off/on; scanpst -no repair on a *copy*
```
