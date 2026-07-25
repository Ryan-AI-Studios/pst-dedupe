# Fix notes — Codex review r1 (0064)

**Branch:** `feat/0064-desk-platform-connect-ux`  
**Audit:** `review.codex.md` — Verdict FAIL  
**Date:** 2026-07-25  
**Scope:** Validated P1/P2 must-fix + easy P3. No commit. Track not marked Completed.

---

## [P1] Async Connect/open race → hybrid Solo + Connected

### Cause
Local open/create remained enabled while Connect dialog/worker was pending. `apply_connect_result` and `poll_matter_op` did not re-check the opposite mode before committing state.

### Fix
| Layer | Change |
|---|---|
| `connect.rs` | `can_open_local_matter(connected, connect_pending)`; `can_apply_connect_session(matter_open)`; `can_apply_local_matter(...)`; `ConnectDialogState::is_pending()` |
| `app.rs` open/create | Guards pass `connect_dialog.is_pending()`; Create/Open/Recent UI disabled while pending |
| `apply_connect_result` | If `matter_root` is set, **refuse** session (fail closed); surface error; token drops without storage |
| `poll_matter_op` | On Created/Opened success: refuse `set_matter` when Connected or Connect pending |

### Tests
- `connect_pending_blocks_local_open`
- `apply_connect_refuses_when_matter_open`
- `apply_local_matter_refuses_when_connected`
- `connect_dialog_pending_covers_open_and_busy`

---

## [P1] Remote coding drafts not item-scoped

### Cause
Global `codes_draft`, conflict flag, and codes result channel were reused across selections. Navigate A→B could Apply A’s draft to B or apply stale success/409 for A onto B.

### Fix
| Layer | Change |
|---|---|
| `RemoteReviewState` | `codes_gen`, `codes_item_id`; draft/conflict cleared on item change via `clear_codes_for_navigation` |
| `select_index` | Clear draft/conflict + drop in-flight `codes_rx` when target item ≠ draft scope |
| `apply_codes` | Always uses **current selection** item id; bumps `codes_gen`; result carries `(gen, item_id)` |
| `poll` | `codes_result_is_current` — ignore stale gen/item; do not mutate current draft |

### Tests
- `navigate_clears_codes_draft_for_other_item`
- `select_index_clears_draft_when_item_changes`
- `stale_codes_result_for_item_a_ignored_when_on_b`
- `apply_always_uses_current_item_id`

---

## [P2] 409 server snapshot incomplete + swallows auth

### Cause
`get_item(...).ok()` discarded 401 and other refresh errors. Conflict UI had no honest thin-API summary.

### Fix
| Layer | Change |
|---|---|
| `map_conflict_with_snapshot` | Typed path: success → version/subject/status + note that codes list is **not** in thin API; `Unauthorized` → `CodesApplyErr::Unauthorized` (forces Solo via existing auth surface); other errors → `snapshot_error` on conflict panel |
| Conflict UI | Shows `server_snapshot_summary`, thin-API note, and non-silent snapshot errors |

### Tests
- `snapshot_auth_failure_is_detectable`
- `snapshot_error_not_swallowed_on_conflict`
- `server_snapshot_summary_includes_version_status`
- `classify_snapshot_refresh_error` Unauthorized vs Other

---

## Easy P3 (done)

| Item | Change |
|---|---|
| SSO cancel self-connect | Bind loopback on UI thread; store `sso_pending_port`; `close()` TCP-probes the port to unblock accept; worker reports “SSO cancelled.” |
| Trailing whitespace | Removed from `conductor/ROADMAP.md` and `docs/operator-golden-path.md` (`git diff --check`) |

### Deferred (P3 residual, still OK)
- Desk↔service in-process integration tests for login + 409 (harness cost)
- True abortable body HTTP (single-flight + gen gate remains)

---

## Verification

```
cargo fmt --all
cargo clippy -p dedupe-desk -p matter-service --all-targets -- -D warnings   # ok
cargo test -p dedupe-desk --bin dedupe-desk                                 # 160 passed
cargo test -p matter-service                                                # 10 unit + 13 integration passed
```

**Not done (per instruction):** git commit; mark track Completed; registry/`review.md` closeout.
