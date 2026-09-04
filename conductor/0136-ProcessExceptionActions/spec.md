# 0136 — Process exception groups (real errors only)

> Mockup password vault / “request from custodian” stays **out**.
> **D-0034-06** (password bypass) remains **never**. Honest empty if no `item_errors`.

- **Track ID:** 0136-ProcessExceptionActions
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\`
- **Status:** Ready — not started
- **Depends on:** **0126 Completed** (exception heading + groups)
- **Spec authored:** 2026-09-03 (placeholder) → **2026-09-03 Ready** (HEAD `cc88576`)
- **Series:** V

> **Closes / absorbs:** `D-0136-exception-actions`. Does **not** close D-0034-06.
> **HITL:** synthetic `item_errors` (or a failed extract on a fixture) → group + Retry resumes the failed job. Zero-error matter still shows “No item_errors recorded.” No vault copy.

---

## 1. Objective

When Process has real `item_errors`, present mockup-like **group + detail + action that exists** (resume failed job). Do not invent vault, OCR-queue, exclude, or ACME filenames.

---

## 2. Context (read before starting)

### 2.1 Live APIs (`cc88576`; **re-verify at execute**)

| Surface | Fact |
|---|---|
| `ItemError` | `item_id, source_id, job_id, stage, code, message` (+ detail/created_at). |
| Groups | `process_page` groups `list_item_errors_recent(100)` by `code` → `ProcessErrorGroup { code, count, sample_message }`. **No** `job_id` / `item_id` on the DTO today. |
| UI | Group buttons + detail (`code` / count / sample_message). Footer still `EXCEPTIONS_NOT_THIS_TRACK` (“Retry / exclude / password vault: not this track.”). |
| Resume | `process_resume_blocking` → `ProcessRunner::resume` (Busy if another job active). Allowed from Process for failed/paused jobs the runner accepts. |
| Exclude | **No** host API to exclude an `item_error` from the corpus at plan-time. **Keep honest empty.** |
| Vault | **D-0034-06 never.** Encrypted → fail closed. |
| Codes (examples) | ingest-purview: `zip_path_traversal`, `zip_corrupt`, `unsupported_7z`, `io_error`, `package_not_found`, … Use **live** codes as titles; do not invent mockup “corrupt PST / OCR queue” buckets that do not match codes. |
| Schema | **41**. No bump. |
| MS-PST | **N/A this track.** |

### 2.2 Locks

Retry = `process_resume` when `sample_job_id` is Some **and** `retry_allowed(job.state)` is true. Pure helper: **`failed` or `paused` only** (not succeeded / running / cancelled / pending). Hide otherwise. Jobs table already has Resume (**0122**) — exception Retry is the same host command, not a second queue writer. **Do not** reuse bare `spawn_resume` `let _ =` — surface Busy / InvalidJob on the Process `error` signal like ingest CTAs.

`sample_job_id`: from the **first** grouped error that has a `job_id`. `sample_item_id`: from the **first** grouped error that has an `item_id` (may be a different row). Groups still come from `list_item_errors_recent(100)` — sample may not be the newest error of that code (existing honesty line stays).

Titles: `exception_title(code)` match on live codes; default arm returns `code` as-is. No invented mockup buckets.

Chrome HITL Exceptions (0) after ingest-only is **correct**. Gap appears after extract/profile errors.

### 2.3 Tools / comments

Same as 0133. Decline Bugbot usage-limit.

---

## 3. In scope

1. Optional `sample_job_id` / `sample_item_id` on `ProcessErrorGroup` (`#[serde(default)]`) — first-with-job_id / first-with-item_id independently.
2. `exception_title` from live codes; fallback = raw `code`.
3. Detail keeps sample_message. **Retry** iff `retry_allowed(&job.state)` after looking up `page.jobs`. Open-in-review only if `sample_item_id` is Some (`review_doc_href`).
4. Replace `EXCEPTIONS_NOT_THIS_TRACK` with honest copy: no vault; exclude not available.
5. Tests: `retry_allowed` table; Retry hidden without job_id / on succeeded; Retry present with failed; empty groups stay honest.

## 4. Out of scope

- Password list / request-from-custodian / vault UI.
- Exclude (unless a real host API exists at execute — then document in `review.md`, do not fake).
- Produce pre-flight (**0137**).
- Inventing 37 quarantined ACME rows.

## 5. Preconditions

- **P1:** 0126 groups + 0116 `process_resume` in live chrome.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Retry without job_id | Hide button |
| Vault copy implies a feature | Forbidden |

## 7. Definition of Done

- [ ] **DoD-1** Zero errors: still honest empty, no fake quarantined counts.
- [ ] **DoD-2** Non-zero: groups from real codes; Retry only when `retry_allowed` (failed/paused) after `page.jobs` lookup; resume errors surface (no `let _ =`).
- [ ] **DoD-3** No vault copy that implies a feature. D-0034-06 stays never. Exclude not faked.
- [ ] **DoD-4 Recorded.**

## 8. Verification

```powershell
cargo test -p dedupe-chrome --lib
cargo test --manifest-path crates\dedupe-chrome\ui\Cargo.toml process
```

## 9. Deferred roll

| Row | Disposition |
|---|---|
| D-0136-exception-actions | **Absorb** |
| D-0034-06 | **Decline / never** |
| D-0116-workflow | Remain |
| D-0024-01 | Decline |
| Last-PR comments | **Decline** |
| Fold-in opencode-M1 / AGY-136-01 | **Fold** — `retry_allowed` + surface resume errors |
| Fold-in opencode-m1 | **Fold** — sample_job_id / sample_item_id independently |
| Fold-in opencode-m2 | **Fold** — recent-100 sample may be stale (honesty line stays) |
| Fold-in AGY-136-02 | **Fold** — `exception_title` default = raw code |
| Fold-in AGY-136-03 | **Already covered** — no vault / exclude |
