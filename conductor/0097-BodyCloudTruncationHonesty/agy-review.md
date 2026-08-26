# Antigravity Adversarial Code Review — Track 0097: Body-Cloud Truncation Honesty

- **Track ID:** `0097-BodyCloudTruncationHonesty`
- **Reviewer:** Antigravity (Adversarial Code Auditor & Systems Architect)
- **Date:** 2026-08-26
- **Review Scope:** Review only (no implementation) — line-level forensic audit of `body_cloud_links.rs`, regex search subroutines, ledger row emission, and telemetry semantics.
- **Spec / Plan Reference:** [`spec.md`](file:///C:/dev/Dedupe/conductor/0097-BodyCloudTruncationHonesty/spec.md), [`plan.md`](file:///C:/dev/Dedupe/conductor/0097-BodyCloudTruncationHonesty/plan.md)

---

## 1. Executive Summary & Forensic Audit Finding

In operator exports on `INC0102784.pst` (4,055 messages), `export_body_cloud_links.csv` emitted **65 rows**:
- **3 real cloud links** (across 2 messages, `deo.sharepoint.com`).
- **62 empty-URL rows** (`cloud_url: ""`, `url_source: ""`, `reason: "BODY_CLOUD_LINK_TRUNCATED"`).

This adversarial audit proves that **60 of those 62 messages contained ZERO cloud links**. The 62 empty rows were phantom ledger entries caused by a semantic conflation between **body scan windowing (100k character budget)** and **actual cloud link truncation**.

Furthermore, this audit discovered a **critical silent-drop vulnerability**: valid SharePoint document URLs exceeding 2,048 characters are **silently dropped without setting `truncated = true`**.

---

## 2. Line-Level Discrepancies & Subroutine Vulnerabilities

### Discrepancy 0097-1: False-Positive Window Truncation Marker
- **File:** `crates/dedup-engine/src/body_cloud_links.rs` (lines 90–93, 100–103)
  ```rust
  let (text, windowed) = body_window_from_bytes(raw);
  if windowed {
      truncated = true;
  }
  ```
- **Audit Analysis:**
  - Any email with HTML body >100,000 characters (extremely common in HTML newsletters, threads with embedded base64 CSS, or long chains) sets `truncated = true` at the very beginning of the scan.
  - In `crates/pst-dedup-cli/src/unique_pst_cmd.rs` (lines 2517–2527):
    `if p.body_cloud_truncated { body_cloud_link_rows.push(BodyCloudLinkRow::truncated_marker(...)); }`
  - This emits an empty-URL row in `export_body_cloud_links.csv` with `reason: "BODY_CLOUD_LINK_TRUNCATED"`.
- **Operator Harm:** Counsel reading `export_body_cloud_links.csv` falsely concludes that 62 cloud links were detected and lost, when in reality 60 of those emails never contained a cloud link.

### Discrepancy 0097-2: Silent Discard of Oversized URLs (`MAX_URL_LEN = 2048`)
- **File:** `crates/dedup-engine/src/body_cloud_links.rs` (lines 282–285)
  ```rust
  let cand = normalize_candidate(raw, strip_trailing_punct);
  if cand.is_empty() || cand.chars().count() > MAX_URL_LEN {
      return;
  }
  ```
- **Audit Analysis:**
  - When `hits.len() < MAX_LINKS_PER_MESSAGE` (50), if a candidate URL exceeds 2,048 characters, `try_keep_candidate` simply executes `return;`.
  - **`*truncated` is NOT set to `true`**.
  - **The Defect:** If an email has 1 valid SharePoint link that is 2,200 characters long, the link is silently discarded, `hits` remains empty, and `truncated` remains `false`.
  - Conversely, only if `hits.len() >= 50` (line 273) does `try_keep_candidate` inspect candidate length before setting `*truncated = true`.

### Discrepancy 0097-3: Inefficient Full-Text Rescan in `more_document_candidates_beyond`
- **File:** `crates/dedup-engine/src/body_cloud_links.rs` (lines 228–260)
  ```rust
  fn more_document_candidates_beyond(text: &str, seen: &HashSet<String>) -> bool
  ```
- **Audit Analysis:**
  - When the 50th link is reached in `collect_from_html`, `more_document_candidates_beyond` re-runs `bare_url_re()` and `href_re()` over the **entire** text from index 0.
  - While `seen.contains(&cand)` prevents duplicate counting, re-scanning 100 KB from byte 0 imposes unnecessary quadratic regex overhead on high-link messages.

---

## 3. Concrete Solution Architecture & Policy Locks

### Policy Rule 1: Eliminate Empty-URL Phantom Rows from CSV
- `export_body_cloud_links.csv` must ONLY contain rows for **actual identified cloud URLs**.
- If a message had >100,000 body characters but 0 cloud links in the scanned window, **do NOT emit a row into `export_body_cloud_links.csv`**.

### Policy Rule 2: Honest URL Truncation Handling
1. **Oversized URL (>2,048 chars):**
   - Do not silently discard. Truncate the URL to 2,048 chars, keep it in `hits`, set `truncated: true`, and emit with `reason: "BODY_CLOUD_LINK_URL_TRUNCATED"`.
   - This allows counsel to inspect the tenant domain (`<tenant>.sharepoint.com`) and document path prefix.
2. **Link Cap (>50 links per message):**
   - Emit the first 50 links with `link_index = 0..49`.
   - If link 51+ exist, set `truncated = true` on the message and record `reason: "BODY_CLOUD_LINK_MAX_LINKS_EXCEEDED"`.

### Policy Rule 3: Unambiguous Summary Counters
In `summary.json`, provide honest separation:
- `body_cloud_links_total: 3` — total real URLs in CSV.
- `messages_with_body_cloud_links: 2` — messages with ≥1 URL in CSV.
- `body_cloud_link_truncated_messages: 0` — messages where detected URLs were capped (>50 links or >2048 chars).
- `body_scan_window_capped_messages: 62` — messages whose raw body exceeded 100,000 characters.

---

## 4. Recommended Spec & Plan Amendments

1. **Update `body_cloud_links.rs`:**
   - Modify `try_keep_candidate`: when `cand.chars().count() > MAX_URL_LEN`, store the truncated 2048-char prefix with `truncated = true` and `reason = URL_TRUNCATED` instead of silently returning.
   - Do not set `BodyCloudScan.truncated = true` solely for `body_window_str` exceeding 100k chars; track `window_capped: bool` as an independent field on `BodyCloudScan`.
2. **Update `unique_pst_cmd.rs`:**
   - Remove `BodyCloudLinkRow::truncated_marker` emission for messages with 0 hits.
   - Bind `body_cloud_link_truncated_messages` strictly to actual link caps/truncations.
   - Record `body_scan_window_capped_messages` in `summary.json`.
3. **Update Definition of Done (DoD-1 & DoD-2):**
   - Assert that on a 150 KB body with 0 cloud links, `export_body_cloud_links.csv` contains 0 rows, and `summary.json` records `body_scan_window_capped_messages: 1` and `body_cloud_link_truncated_messages: 0`.
   - Assert that a 2,500-char SharePoint link is captured with 2048-char prefix, `truncated: true`, and `reason: BODY_CLOUD_LINK_URL_TRUNCATED`.

---

## 5. Verdict & Risk Rating

- **Track Rating:** **PASS (Ready with forensic bug fixes and telemetry decoupling)**
- **Complexity / Risk:** Low (pure string scanner and reporting logic; zero on-disk PST format changes).
- **Execution Estimate:** 0.5 – 1 day.
