# Fix notes — subagent r1 findings (track 0064)

**Branch:** `feat/0064-desk-platform-connect-ux`  
**Date:** 2026-07-25  
**Authority:** `review.subagent-r1.md`, spec §3.3 / §3.7.1  
**Scope:** P2 blocking fixes + easy P3 polish. No track Completed; no commit; ledger left open.

---

## P2-1 — Mid-session 401 drops to Solo

**Problem:** Review path detected 401/unauthorized text but left `connected_session` in place (comment-only branch).

**Fix:**
- `remote_client::{is_auth_failure_message, force_clear_connected_session, AUTH_FAIL_SOLO_STATUS}` — shared helpers; token zeroizes on `ConnectedSession` / `BearerToken` drop.
- `RemoteReviewState::has_auth_failure()` — checks `error`, `list_error`, and body `Err` for 401/unauthorized/session-expired wording.
- `DeskApp::force_disconnect_auth_fail()` — clears session via helper, clears remote review, closes Connect dialog, sets status *Session expired or unauthorized — returned to Solo* + error *Session expired (401). Reconnect when ready.* (no best-effort logout; bearer already dead).
- Connected Review branch calls `force_disconnect_auth_fail()` when `has_auth_failure()`.
- Codes Unauthorized message aligned to the same wording.

**Tests:** `auth_failure_message_and_session_clear`, `has_auth_failure_detects_401_surfaces`, `auth_fail_helper_clears_session`.

---

## P2-2 — Body loads single-flight (not fire-and-forget pool)

**Problem:** Each selection spawned a new blocking body thread; generation discarded stale *results* but concurrent HTTP piled up.

**Fix (preferred pattern from review):**
- One dedicated `desk-remote-body` worker per `RemoteReviewState`, fed by `body_job_tx`.
- `select_index` sends `(gen, item_id, session, result_tx)`; replaces `body_rx` (stale senders disconnect).
- Worker: `recv` → `take_latest_body_job` (drain queue) → at most one blocking `get_item_body` → send result; UI still generation-gates apply.
- `clear()` drops `body_job_tx` so the worker exits (no orphan pool).

**Tests:** `body_single_flight_drains_to_latest_gen`, `body_channel_drain_keeps_only_latest`, existing `stale_body_discarded_after_navigate`.

---

## Easy P3

| Finding | Action |
|---|---|
| No-op `stale_body_generation_discard` | Replaced with real auth-fail helper test in `remote_client` |
| `features.md` “Future: Connect” row | Updated to shipped thin remote Review; concurrent-review sketch text |
| SSO `accept()` no timeout | Nonblocking accept + 3-minute poll deadline in `sso_loopback_blocking` |
| `produce_params` silent `.max(1)` clamp | Removed; pre-flight `validate_bates_start` remains gate |
| Mock HTTP integration | **Not done** (still deferrable P3) |
| SSO Cancel unblocks listener via self-connect | **Not done** — `sso_pending_port` never set from bind; timeout covers hang; residual ok |

---

## Verification (this session)

```text
cargo fmt --all                                          OK
cargo clippy -p dedupe-desk -p matter-service --all-targets -- -D warnings   OK
cargo test -p dedupe-desk --bin dedupe-desk              149 passed
cargo test -p matter-service                             10 unit + 13 integration passed
```

---

## Residual / not in this pass

- Dialog Cancel during SSO still does not self-connect to unblock accept early (only 3 min timeout).
- No mock/in-process HTTP end-to-end for login/409.
- Track not marked Completed; ledger transaction left open for orchestrator closeout.
