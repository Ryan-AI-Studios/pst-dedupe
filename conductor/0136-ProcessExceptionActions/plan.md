# 0136 — Process exception groups — Plan

> Status: **Ready — not started**. Fold-in 2026-09-03: `opencode-review.md` + `agy-review.md`.

> **Ledger:** `ledgerful ledger start 0136-exception-actions --category FEATURE --message "Process exception Retry from real item_errors"`

---

## Phase 0 — Pin codes + resume → DoD-2

- [ ] List live `item_errors` codes in ingest-purview / extract-pst / matter-core (do not invent).
- [ ] Confirm `process_resume` Busy + InvalidJob on succeeded. Note runner also accepts cancelled/pending — **UI Retry still only failed/paused**.

## Phase 1 — DTO + UI → DoD-1 / DoD-2 / DoD-3

- [ ] `ProcessErrorGroup`: `sample_job_id` from first error with `job_id`; `sample_item_id` from first with `item_id`; WASM `#[serde(default)]`.
- [ ] `fn retry_allowed(state: &str) -> bool` true only for `"failed"` / `"paused"`. Unit-test the table (succeeded/running/cancelled/pending → false).
- [ ] Retry looks up `page.jobs` by `sample_job_id`, hides unless `retry_allowed`. Invoke `process_resume`; on Err set Process `error` (no `let _ =`).
- [ ] `exception_title(code)` match live codes; `_ => code`.
- [ ] Remove vault/exclude “not this track”; honest empty for those actions.
- [ ] Tests: empty groups; Retry hidden without job_id / succeeded; present with failed.

## Phase 2 — Finalize → DoD-4

- [ ] `review.md`; ledger commit.

## Handoff

- Do not port mockup Beta_LOI_final.zip / password-vault rows.
- Do not add a credential store.
