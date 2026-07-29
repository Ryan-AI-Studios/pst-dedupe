# 0085 — Body-Inline Cloud Link Detect

> Structure follows `C:\dev\coordinated\conductor\templates\0000-Description\spec.md`.
> Expanded subsections under §2–§3 are normative design for implementers. DoD is §7.
>
> **Review fold-in (2026-07-29):** dual-AI review of Ready draft incorporated below.
> Disposition of each claim is in §2.10 (agree / partial / decline with reason).

- **Track ID:** 0085-BodyCloudLinks
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → Series M (unique-export fidelity residuals) after 0082–0084
- **Cross-repo contract:** n/a
- **Status:** Ready — not started (review-folded 2026-07-29)
- **Depends on:** 0073 · 0078 · 0080 · 0083 · 0084 (all **Completed** on board)
- **Spec authored:** 2026-07-29
- **Series:** M (Unique export fidelity residuals)

---

## 1. Objective

Ship **offline body-surface detection** of SharePoint/OneDrive-shaped cloud links that appear in message **HTML (and optionally plain) body** without a MAPI Attachment Table row — emit an **actionable multi-row ledger + summary counts**, update **fidelity-contract honesty** so operators no longer treat “0084 closed cloud attach” as full modern-attachment coverage, and **never** hydrate payloads, invent attachment objects from body URLs, or claim Purview collection parity.

---

## 2. Context (read before starting)

### 2.1 Why this track exists now

| Deferred / ceiling | Severity | Claim |
|---|---|---|
| **D-0084-body-cloud-links** | P2 | Body-only / inline paste SharePoint/OneDrive URLs without attach-table rows — opened by 0084 review fold-in |
| **0084 honesty ceiling** | — | `fidelity_contract_v1.cloud_modern_attachments` explicitly says body-inline not scanned |
| Mode A / attach incomplete | — | `is_attach_incomplete` cannot see body-only links; Mode A will not promote for them |
| Operator actionability | — | After 0084, attach-table cloud gets `cloud_url` hit-list; body paste links still require opening every message |

0084 closed **attachment-table** cloud/modern detect (`ATTACH_CLOUD_LINK`, Mode A incomplete, pointer preserve). Dual-AI review + Purview practice both treat **body hyperlinks** as a common (often dominant) modern-attachment shape. Closing D-0080 without a body residual would over-claim. This track is the **board-first Series M residual** after 0082–0084.

### 2.2 Industry / product anchors (researched 2026-07-29)

**Microsoft Purview — Cloud attachments in eDiscovery** ([Learn](https://learn.microsoft.com/en-us/purview/edisc-cloud-attachments), article ms.date 2026-02-27, content updated **2026-06-11**, access date 2026-07-29):

Purview collection of modern attachments is **link-oriented** (service-side), not “Attachment Table named-prop only.” Documented **limits** (design inputs for this track — **not** product parity claims):

| Cap / rule | Purview |
|---|---|
| Body format | **HTML only** (plain text not supported for modern-attach extraction) |
| Body length | Content beyond **100,000** characters not considered |
| URL length | URLs longer than **2,048** characters skipped |
| Links per message | First **50** links only |
| Quoted / forwarded | Cloud links in **quoted** portions of forwards/replies **not** extracted |
| Non-clickable / malformed | Not processed |
| Encrypted messages | Not supported |
| Folder / OneNote notebook links | Not collected as file modern attachments |
| Network collection | Service can add linked file to review set — **this product does not** |

**Product position (locked):**

1. Offline PST tools **detect + ledger** body cloud-shaped URLs already present in the body text.
2. Unique-PST **already preserves body HTML/plain** when fidelity allows — the URL string usually **survives** in the deliverable (unlike the 0084 “ghost attach” case). The gap is **file payload not in the export pack**, not pointer loss inside the PST.
3. Therefore: **do not invent Attachment Table rows** from body URLs; **do not** treat body-only hits as `is_attach_incomplete` / Mode A promote drivers by default.
4. Value is **operator hit-list + contract honesty**, aligned with 0084’s “detect ≠ hydrate” story on a different surface.
5. Use Purview caps as **scan budget defaults**; document intentional differences (e.g. optional plain-text pass).

**URL shape allowlist (Phase 0 locks exact host/path patterns from fixtures + commercial M365 hosts):**

**Document-shaped only (proportionality).** Bare `*.sharepoint.com` host match is **not** enough — corporate intranet, HR wikis, and site home pages would flood the hit-list and break operator use of the CSV as a Purview re-collection list. A candidate URL must match **both** a host class **and** a **document-shaped path marker** (or a short-link host that exists only for file shares).

| Host class (commercial defaults) | Role |
|---|---|
| `*.sharepoint.com`, `*.sharepoint-df.com` | SharePoint Online / OneDrive for Business path host |
| `onedrive.live.com` | Consumer OneDrive |
| `1drv.ms` | OneDrive short share links (document-shaped by nature of the shortener) |
| `*.safelinks.protection.outlook.com` (and regional commercial variants) | SafeLinks wrapper — unwrap `url=` then re-test target against allowlist |

**SharePoint / OneDrive path action tokens** (Microsoft sharing route codes — **include Excel**):

| Token | Document class |
|---|---|
| `:w:` | Word |
| `:x:` | **Excel** (required — common in legal/finance exhibits; must not be omitted) |
| `:p:` | PowerPoint |
| `:b:` | PDF / binary-style share |
| `:u:` | Catch-all file share (webpages, Visio, zip, Publisher, mail, other) |
| `:f:` | **Folder** share — **exclude by default** (Purview does not collect folder links as modern file attachments; ledger noise) |

Also accept document-shaped path markers when action tokens are absent:

- Path ends with common Office/PDF extensions (case-insensitive): `.docx`, `.doc`, `.xlsx`, `.xls`, `.xlsm`, `.pptx`, `.ppt`, `.pdf`, `.csv` (Phase 0 may extend conservatively).
- Document library item / download-style paths locked in Phase 0 from fixtures (e.g. `/_layouts/15/Doc.aspx`, `/download.aspx` **only when** query implies a document — do not match bare site roots).

**Reject (examples):** site home `https://contoso.sharepoint.com/sites/HR`, wiki pages without document markers, generic `/sites/Foo` roots, non-cloud `https://` hosts.

**SafeLinks:** Commercial format `https://<region>.safelinks.protection.outlook.com/?url=...`. Many tenants now use **API-only Safe Links** (no body rewrite) — raw SharePoint/OneDrive hrefs already appear; both paths must hit (wrapper unwrap **and** direct document-shaped URL).

**Residual hosts (explicit, not silently “done”):** GCC High / DoD / other sovereign-cloud SharePoint and SafeLinks hostname suffixes are **not** fully published in a single stable public table for this product’s Phase 0. Open residual **`D-0085-sovereign-cloud-hosts`** — commercial allowlist ships; sovereign host variants are documented incompleteness, not claimed coverage.

Do **not** treat every `https://` or every `*.sharepoint.com` link as a modern document attach (false-positive / proportionality trap).

### 2.3 Live code snapshot (verified 2026-07-29)

| Surface | State |
|---|---|
| `CanonicalMessage.body_plain` / `body_html` | Present on materialize path (`pst_materializer`) |
| Attach cloud detect (0084) | NPMAP + attach PC; `is_cloud_link`; attach ledger `cloud_*` |
| `is_attach_incomplete` | Attach-table only; doc cites D-0084-body-cloud-links |
| `export_attachments.csv` | Attach-locus ledger; not designed for body multi-link rows |
| `export_messages.csv` | Message locus; append-only column history (0075/0081/0082) |
| Body URL scan | **None** in unique-pst path |
| `fidelity_contract_v1.cloud_modern_attachments` | BestEffort; reason names body residual open |
| Workspace `regex` | **1.x** present (matter-entity FA packs); available for linear-time patterns |
| `url` crate | Present in desk/service crates; **not** required if Phase 0 keeps hand parse of `url=` + host match |
| HTML parser crates | `html5ever` / `ammonia` in lock (desk/other); **prefer no new dep** — regex/`href` scan first |

### 2.4 Dependency currency (re-queried crates.io 2026-07-29)

No dependency bumps required unless High/Critical advisory (0081 security override). Prefer **workspace `regex` only** for scan; no HTML browser stack.

| Dep | Workspace / lock | crates.io max | 0085 |
|---|---|---|---|
| clap | 4.6.4 | 4.6.4 | KEEP |
| rusqlite | 0.40.1 | 0.40.1 | KEEP |
| uuid | 1.24.0 | 1.24.0 | KEEP |
| camino | 1.2.5 | 1.2.5 | KEEP |
| thiserror | 2.0.19 (ws 2) | 2.0.19 | KEEP |
| serde_json | 1.0.151 | 1.0.151 | KEEP |
| regex | 1.13.1 (ws 1) | (1.x line) | **USE** (existing) |
| eframe | 0.34.2 | 0.35.0 | DECLINE_MAJOR |
| reqwest | 0.12.28 (+0.13 present) | 0.13.4 | DECLINE_MAJOR |
| md-5 | 0.10 (ws) | 0.11.0 | DECLINE_MAJOR (unrelated) |
| aes-gcm | 0.10 (ws) | 0.11.0 | DECLINE_MAJOR |
| argon2 | 0.5 (ws) | 0.6-rc | DECLINE |
| sha2 / tantivy | post-0081 pins | as prior | KEEP |

**No network client** for link fetch. No new major crates without 0083-style provenance gate.

### 2.5 Locked product rules

1. **Sources read-only.** Never mutate source PSTs.
2. **Offline only.** No HTTP(S) fetch of linked files. Detection ≠ hydration. Runbook: *body already contains the URL; we extract a hit-list for native Purview/SharePoint re-collection; we do not download the file.*
3. **Never invent Attachment Table rows / attach binary** from body URLs. Body links stay body links.
4. **Never claim payload `Preserved`.** Body scan does not make cloud file content present offline.
5. **Do not fold body hits into `is_attach_incomplete` by default.** Mode A promote remains **attach-table completeness** only (0083/0084). Body-only hits must not reclassify a message as attach-incomplete solely because of HTML links (would promote peers for the wrong reason when peers share the same body, **and** would still miss the real cross-shape case below).
   - **Known gap (mandatory honesty — contract + runbook):** Mode A **will** prefer a peer with a **physical / attach-table** complete copy over a peer with **attachment-table CloudLink** incompleteness (0083/0084). Mode A **will not** prefer a peer that carries the file as a classic by-value attachment over a peer that only has the **same logical message with an HTML inline link** (body-only). Counsel must know: *physical attach beats MAPI cloud-link incomplete; physical attach does **not** beat HTML-inline-only via Mode A.* Residual product if ever desired: separate “prefer physical payload over body-only link” policy — **out of 0085**.
6. **Ledger surface (normative):**
   - New report file: **`export_body_cloud_links.csv`** (multi-row, one row per kept link hit; default **on** when unique-pst report pack is written and attach-ledger mode is not `off` — exact flag wiring in §2.7.3).
   - Append **`body_cloud_link_count`** on `export_messages.csv` (count only; join via `source_id` + `nid`).
   - Summary JSON / histogram: messages-with-hits, total-links-kept, truncated-by-cap counts.
7. **Reason / kind codes (fixed public strings):**
   - Row kind / reason: **`BODY_CLOUD_LINK`**
   - Truncation marker row (if needed): **`BODY_CLOUD_LINK_TRUNCATED`** (message-level, once per message when caps bite)
8. **Scan budgets (defaults; document; CLI overrides optional but not DoD-required):**
   - Prefer **HTML body** first (Purview-aligned).
   - Optional **plain-text** pass for bare URLs when HTML missing or after HTML (document order).
   - Max body scan window: **100_000** Unicode scalar values (or bytes if Phase 0 chooses UTF-8 byte cap — lock one and test).
   - Max URL length: **2_048**.
   - Max kept **document-shaped** cloud links per message: **50** (first-N after allowlist filter).
9. **SafeLinks:** When a matched URL is an Exchange SafeLinks wrapper carrying a nested target that itself matches the **document-shaped** allowlist, ledger the **unwrapped target** (HTML-unescaped query value). Optional secondary column for wrapper URL only if cheap — default: target only in `cloud_url`. Commercial SafeLinks hosts only in default allowlist; sovereign residual **D-0085-sovereign-cloud-hosts**.
10. **Quoted content:** Phase 0 chooses **A)** scan full body (simpler; more hits than Purview) **or B)** skip common quote markers / `blockquote` (closer to Purview). Default recommendation: **A full body** + honesty note “broader than Purview quoted-skip”; residual if false positives dominate.
11. **Exit codes:** Body-only cloud hits **do not alone** force exit **64**. Attach fails / existing partial rules unchanged. Optional future `--fail-on-body-cloud-links` is **out** unless product insists mid-track.
12. **CSV injection neutralization** on free-text URL cells (0073 rule) — prefix formula-dangerous cells; **do not** rewrite URL structure.
13. **Append-only schemas** for existing CSVs; new file gets its own fixed header constant + tests.
14. **Synthetic fixtures only** in git (HTML bodies with sharepoint/onedrive/safelinks **document-shaped** shapes, including **`:x:` Excel**).
15. **No `unwrap`/`expect` in production** — `miette` + `Result`.
16. **Degrade gracefully:** unreadable body → no hits (existing body_unavailable/incomplete flags); never hard-fail export solely for scan.
17. **Dedup within message — preserve forensic URL fidelity:**
    - **Never strip query parameters** (`?d=`, `?csf=`, `&e=`, sharing tokens, etc.). Those often encode permission grant / share context needed for native “as-sent” collection.
    - Allowed “normalize” steps only: HTML entity unescape of the href; trim surrounding whitespace; optional strip of **trailing sentence punctuation outside the URL** (e.g. trailing `).` after the URL) — must not touch the query string.
    - Dedup key = **exact post-unescape string** (full URL including query). Two links that differ only in query stay **two rows**.
    - Cap applies after dedupe of exact strings.
18. **Do not change keep-set identity** to include body cloud URLs (product later if needed — related to but not D-0076-attach-content).
19. **Document-shaped allowlist only** (see §2.2). Default product does **not** need a `--strict-document-links` flag if the default already requires document markers; optional future “looser host-only” mode is **out** (would recreate the noise machine).
20. **Regex implementation:** use workspace **`regex` crate only** (finite automata; no backreferences/lookaround). Patterns are **fixed at compile/implementer time**, never derived from untrusted body text. **Do not** add `fancy-regex` for lookaround exclusions. ReDoS is **closed by construction** under these constraints + the 100k scan window — test effort goes to **allowlist accuracy**, not adversarial ReDoS property tests.
21. **Sovereign-cloud hosts:** commercial allowlist is the ship target; GCC High / DoD / other sovereign SharePoint + SafeLinks host variants remain residual **D-0085-sovereign-cloud-hosts** (document; do not claim complete).

### 2.6 Deferred roll-in matrix

| ID | Disposition in 0085 | Why |
|---|---|---|
| **D-0084-body-cloud-links** | **Ship / close** | Core deliverable |
| **D-0084-cloud-named-prop-write** | **Decline** | Writer NPMAP / named-prop re-emit; attach-table surface, not body scan |
| **D-0076-attach-content** | **Decline** | Identity hashing attach bytes; Mode A group-fracture risk; different product |
| **D-0079-deterministic-key** | **Decline** | Product record-key / byte-reproducible PST; not this surface |
| **D-0073-eml** | **Partial optional** | If unique-eml materialize already has body, **reuse scanner** and optionally write the same CSV under eml report dir when cheap — full attach-ledger CSV parity for eml remains residual; do not expand 0085 into full D-0073-eml |
| Network hydration / Graph | **Decline permanently here** | Offline invariant |
| Invent attach objects from body URLs | **Decline permanently here** | False MAPI structure |
| Mode A treat body links as attach-incomplete | **Decline** (document known gap) | Wrong promote semantics; runbook/contract must state physical-vs-inline gap |
| Purview parity / service collection | **Decline** | Different product class |
| Full HTML DOM parser dependency | **Decline default** | Regex/`href` first; only if Phase 0 proves broken |
| Bare `*.sharepoint.com` host-only matching | **Decline** | Proportionality / intranet noise |
| Strip query params on normalize | **Decline permanently** | Destroys as-sent share context |
| Sovereign-cloud host suffixes (GCC High / DoD) | **Open residual D-0085-sovereign-cloud-hosts** | Unconfirmed in public docs; legal-adjacent tenants possible |
| Folder shares (`:f:`) as BODY_CLOUD_LINK | **Decline default** | Purview-aligned non-file; noise |

### 2.7 Design sketch (normative)

#### 2.7.1 Pure scanner (`dedup-engine` preferred)

```text
scan_body_cloud_links(opts, html: Option<&[u8]|str>, plain: Option<&str>)
  -> BodyCloudScan {
       hits: Vec<BodyCloudLinkHit { url, source: HtmlHref | HtmlBare | PlainBare | SafeLinksUnwrap }>,
       truncated: bool,
       scanned_html: bool,
       scanned_plain: bool,
     }
```

- **Document-shaped** host+path allowlist (action tokens including **`:x:`**, extensions; reject bare site roots; exclude `:f:` by default) + length caps + first-N + exact-string dedupe (**preserve query**).
- Unit tests prioritize **allowlist accuracy** over ReDoS:
  - hit: `:w:`, **`:x:` (Excel)**, `:p:`, `:b:`, `:u:`, `.xlsx` path, `1drv.ms`, SafeLinks→SharePoint document target
  - miss: bare `https://contoso.sharepoint.com/sites/HR`, generic wiki, non-cloud https, folder `:f:` (default)
  - fidelity: query string `?d=` / `?csf=` retained after unescape; two different queries → two rows
  - caps: 50 / 100k / 2048; empty body

#### 2.7.2 Wire-up (unique-pst materialize / report)

```text
materialize CanonicalMessage
  → scan body
  → attach hits to export path (not to is_attach_incomplete)
  → write export_body_cloud_links.csv rows
  → set export_messages.body_cloud_link_count
  → summary histogram
```

#### 2.7.3 Report pack

**New file** `export_body_cloud_links.csv` (name locked):

| Column | Notes |
|---|---|
| `source_id` | Join key |
| `source_path` | Path mode respects `--ledger-path-mode` |
| `folder_path` | |
| `msg_nid` | |
| `link_index` | 0-based within message after dedupe |
| `cloud_url` | Unwrapped when SafeLinks; **full query string preserved** (HTML-unescaped only) |
| `url_source` | `html_href` / `html_bare` / `plain_bare` / `safelinks` (fixed strings) |
| `truncated` | `true`/`false` on a marker row or last-hit flag — prefer dedicated marker row with reason `BODY_CLOUD_LINK_TRUNCATED` when caps fire |
| `message_subject` | Optional but useful (match attach ledger convenience) |

**`export_messages.csv` append:** `body_cloud_link_count` (u32/string count).

**Mode wiring:** When `--attach-ledger=off`, still allow body CSV via default-on for unique-pst **or** tie body CSV to report pack always-on (prefer always write body CSV when report-dir present — cheap and high value). Lock in plan Phase 0: **default write when report pack exists**.

#### 2.7.4 Contract + docs

- Update `cloud_modern_attachments` reason: attach-table **and** body-inline **document-shaped** detect offline; payload never Preserved; no invented attaches; caps documented; not Purview collection; commercial host allowlist + sovereign residual named.
- Optional new contract line `body_cloud_links` BestEffort if cleaner than overloading one property — Phase 0 chooses one approach; do not leave the residual string `D-0084-body-cloud-links` as “open” after close.
- Runbook must state:
  - operator uses CSV hit-list for native collection; preserve full URL including query for as-sent share context;
  - body URL remains in unique-PST body when body preserved;
  - Mode A **non-interaction** for body-only **and** the **known gap**: physical attach on a peer is **not** preferred over HTML-inline-only via Mode A (only attach-table incompleteness drives Mode A);
  - document-shaped filter (not every SharePoint intranet link);
  - sovereign-cloud host residual.

#### 2.7.5 Non-goals

- Download / Graph / authenticated SharePoint.
- Invent attach rows from body links.
- Mode A / `is_attach_incomplete` extension for body-only.
- Full DOM HTML5 parser unless regex path fails Phase 0.
- Purview version-at-share collection.
- D-0076 / D-0079 / full D-0073-eml / named-prop write.

### 2.8 Affected crates / docs

| Path | Change |
|---|---|
| `crates/dedup-engine` | Pure body cloud link scanner + tests |
| `crates/pst-dedup-cli` | Wire unique-pst (optional eml); CSV + messages column + summary; contract |
| `docs/unique-pst-export.md` | New CSV; column; caps; Mode A non-interaction |
| `docs/unique-pst-ediscovery-runbook.md` | Hit-list workflow; honesty; residual closes |
| `docs/pst-writer-fidelity-v1.md` | Note body links detected in report, not as attach fidelity |
| `docs/deferred.md` | Close D-0084-body-cloud-links |
| CHANGELOG `[Unreleased]` | Tier-1 |

### 2.9 Product decisions locked

| # | Decision | Default |
|---|---|---|
| Q1 | Scan HTML body for cloud-shaped URLs | **Yes** |
| Q2 | Hydrate linked files | **No** |
| Q3 | Invent attach table rows from body URLs | **No** |
| Q4 | Body hit ⇒ `is_attach_incomplete` / Mode A | **No** |
| Q5 | Multi-row body link CSV | **Yes** (`export_body_cloud_links.csv`) |
| Q6 | `export_messages.body_cloud_link_count` | **Yes** |
| Q7 | Exit 64 solely for body-only hits | **No** |
| Q8 | SafeLinks unwrap when nested target is cloud-shaped | **Yes** |
| Q9 | New HTML parser crate | **No** (regex first) |
| Q10 | Purview parity claim | **No** — caps as design inputs only |
| Q11 | Close D-0084-body-cloud-links | **Yes** on DoD complete |
| Q12 | Full D-0073-eml attach ledger | **Out** (optional scanner reuse only) |
| Q13 | Bare SharePoint host = hit | **No** — document-shaped markers required |
| Q14 | Include `:x:` (Excel) action token | **Yes** (mandatory) |
| Q15 | Strip query params on normalize | **No** — never |
| Q16 | Body-only ⇒ Mode A incomplete | **No** — document known gap |
| Q17 | Sovereign-cloud hosts in default allowlist | **Out** — residual D-0085-sovereign-cloud-hosts |
| Q18 | ReDoS hostile property tests | **Out** if stock `regex` only — allowlist tests instead |

### 2.10 Dual-AI review disposition (2026-07-29)

| # | Claim | Disposition | Spec landing |
|---|---|---|---|
| A1-1 | Allowlist omitted Excel `:x:` | **Agree** | §2.2 token table; Q14; Phase 0 + unit tests |
| A1-2 | ReDoS not live under stock `regex` + fixed patterns + 100k window | **Agree** | Rule 20; §6 risk downgraded; redirect tests to allowlist accuracy |
| A1-3 | Sovereign-cloud SafeLinks/SharePoint hosts unconfirmed residual | **Agree** | D-0085-sovereign-cloud-hosts; Q17; rule 21 |
| A2-1 | Bare `*.sharepoint.com` floods intranet noise (proportionality) | **Agree** | Document-shaped allowlist; Q13; decline host-only mode |
| A2-2 | Must not strip query params (as-sent / share context) | **Agree** | Rule 17 rewrite; DoD-3; unit test |
| A2-3 | Mode A will not promote physical-attach peer over HTML-inline-only | **Agree (document)** | Rule 5 known gap; contract + runbook; DoD-5/6 |

---

## 3. In scope

1. **Document-shaped allowlisted body URL scan** (HTML primary; plain optional) with Purview-inspired caps; action tokens including **`:x:`**.
2. **SafeLinks unwrap** when nested target is document-shaped (commercial hosts).
3. **`export_body_cloud_links.csv`** + summary histogram + **`body_cloud_link_count`** on messages CSV; **full query preserved** in `cloud_url`.
4. **fidelity_contract** honesty update (body residual closed; Mode A known gap stated; still never Preserved payload).
5. **Docs** + deferred close for D-0084-body-cloud-links; open D-0085-sovereign-cloud-hosts.
6. **Tests:** allowlist accuracy matrix (hit/miss including Excel + intranet miss); query preserve; fixture unique-pst integration; header locks; workspace gate.
7. **Optional cheap:** unique-eml reuses scanner if body already on path (not full D-0073-eml).

## 4. Out of scope (do NOT do here)

- Network download / Graph / SharePoint auth.
- Inventing MAPI attachments from body URLs.
- Extending Mode A / `is_attach_incomplete` for body-only links (document known gap only).
- Bare host-only SharePoint matching / optional “loose” noise mode.
- Stripping query parameters for dedupe.
- Sovereign-cloud host enumeration as completeness claim (**D-0085-sovereign-cloud-hosts** residual).
- **D-0084-cloud-named-prop-write**, **D-0076-attach-content**, **D-0079-deterministic-key**.
- Full unique-eml attach ledger CSV (**D-0073-eml**).
- Full HTML5 DOM dependency by default; `fancy-regex` lookaround path.
- Claiming Purview modern-attachment collection parity.
- New exit codes; eframe major; identity key changes.

## 5. Preconditions & dependencies

- **P1:** 0084 Completed (attach-table cloud detect + contract language naming this residual).
- **P2:** Materialize path exposes `body_html` / `body_plain` on `CanonicalMessage`.
- **P3:** Report pack + CSV append patterns (0073/0075/0081/0082/0084) available.
- **P4:** Workspace `regex` usable without new majors.
- *Verified to date (2026-07-29):* no body URL scan; D-0084-body-cloud-links open P2; Purview caps live on Learn; crates.io pins KEEP for feature deps.

## 6. Risks

| Risk | Mitigation |
|---|---|
| False positives (intranet SharePoint pages) | **Document-shaped** markers required; reject bare site roots; allowlist unit tests |
| False negatives (Excel `:x:` omitted, unknown hosts) | **Mandatory `:x:`** in token set; residual hosts documented; extensible constant |
| Query strip destroys as-sent context | Rule 17 forbids query strip; test preserves `?d=` / `?csf=` |
| SafeLinks / tracking wrappers hide target | Unwrap `url=` when present; test commercial format |
| Cap too low / too high | Defaults match Purview numbers; counters for truncated |
| Performance on huge HTML | 100k scan window; stock FA `regex`; no full DOM |
| Mode A confusion / physical peer not selected over inline | Explicit non-interaction + **known-gap** prose in contract/runbook + tests |
| Over-claim “modern attach complete” | Contract + runbook; sovereign residual; no invent attaches |
| CSV schema drift | Header constants + unit locks |
| ReDoS | **Downgraded / closed by construction** with stock `regex` + fixed patterns + 100k window (rule 20). No `fancy-regex`. Spend tests on allowlist accuracy, not ReDoS fuzz. |

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Scanner:** Pure function scans HTML (and optional plain) for **document-shaped** allowlisted URLs; unit tests cover hit/miss (including **`:x:` Excel**, intranet miss, `:f:` exclude), cap, exact dedupe **with query preserved**, SafeLinks unwrap, empty body.
- [ ] **DoD-2 — Wire unique-pst:** Materialized messages with body hits produce ledger rows without inventing attaches and without setting `is_attach_incomplete` solely for body hits.
- [ ] **DoD-3 — CSV pack:** `export_body_cloud_links.csv` written with locked header; `cloud_url` actionable **with full query string**; injection neutralized without structural rewrite; `export_messages.csv` has `body_cloud_link_count`.
- [ ] **DoD-4 — Caps honesty:** Default budgets (100k window / 2048 URL / 50 links) enforced; truncation visible (marker row and/or summary counters).
- [ ] **DoD-5 — Contract:** `cloud_modern_attachments` (and/or dedicated `body_cloud_links` entry) no longer claims body residual open; still **not** Preserved for payload; attach-table path remains distinct; **Mode A body-only known gap** stated; sovereign residual named if not covered.
- [ ] **DoD-6 — Mode A non-regression + gap honesty:** Existing Mode A × attach CloudLink tests green; new test proves body-only link does **not** force attach-incomplete; runbook states physical-vs-inline gap.
- [ ] **DoD-7 — No hydration / no invent attach:** No network fetch; no Attachment Table synthesis from body URLs.
- [ ] **DoD-8 — Docs + deferred:** unique-pst-export + eDiscovery runbook + fidelity note; **D-0084-body-cloud-links closed**; **D-0085-sovereign-cloud-hosts opened**; CHANGELOG `[Unreleased]`.
- [ ] **DoD-9 — Deps:** No unapproved majors; workspace `regex` only (no `fancy-regex`).
- [ ] **DoD-10 — Tests gate:** fmt / clippy `-D warnings` / workspace test / deny green.
- [ ] **DoD-11 — Recorded:** `review.md` (fold-in table, allowlist tokens incl. `:x:`, query-preserve, ReDoS-by-construction note, sovereign residual) + board Completed + ledger `FEATURE`.

## 8. Verification commands (reference)

```powershell
cargo test -p dedup-engine -- body_cloud
cargo test -p dedup-engine -- cloud_link
cargo test -p pst-dedup-cli -- body_cloud
cargo test -p pst-dedup-cli -- fidelity
cargo test -p pst-dedup-cli -- promote
cargo test -p pst-dedup-cli -- incomplete

cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
ledgerful verify
```
