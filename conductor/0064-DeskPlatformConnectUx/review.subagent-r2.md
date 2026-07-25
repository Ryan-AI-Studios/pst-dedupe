# Track Completion Audit — 0064-DeskPlatformConnectUx (r2)

| Field | Value |
|---|---|
| **Track** | 0064-DeskPlatformConnectUx |
| **Branch** | `feat/0064-desk-platform-connect-ux` |
| **Auditor role** | Read-mostly completion reviewer (subagent r2) |
| **Date** | 2026-07-25 |
| **Authority** | `conductor/0064-DeskPlatformConnectUx/spec.md`, `plan.md`, `review.subagent-r1.md`, `fix-notes-r1.md` |
| **Method** | Full static re-inspection of Desk Connect/remote/produce modules after r1 P2 fixes; prior P2 verification required. Cargo gates claimed green in `fix-notes-r1.md` (149 desk unit, matter-service unit+integration, clippy `-D warnings`); this r2 process had no shell — did not re-execute `cargo` (orchestrator should paste live output into final `review.md`). |

## Verdict: **PASS WITH DEFERRED P3**

**Finding counts:** **0 P0 · 0 P1 · 0 P2 · 3 P3**

Both **P2** blockers from r1 are **fixed and wired** in product code with unit coverage. Product DoDs 1–7 and 9 are Met; DoD-8 inventory Met (gates not re-run in this process); DoD-10 remains orchestrator closeout. Remaining gaps are deferrable polish only.

---

## Prior P2 verification (required)

### P2-1 — Mid-session 401 drops Solo — **FIXED**

| Check | Result | Evidence |
|---|---|---|
| Shared auth-fail detection | Yes | `remote_client::is_auth_failure_message` — matches `401` / `unauthorized` / `session expired` (case-insensitive) |
| Session clear helper | Yes | `force_clear_connected_session` takes `Option<ConnectedSession>`; token zeroizes via `BearerToken` `ZeroizeOnDrop` |
| Remote review surfaces | Yes | `RemoteReviewState::has_auth_failure()` checks `error`, `list_error`, and `body_text` `Err` |
| App force-disconnect | Yes | `DeskApp::force_disconnect_auth_fail()` — clear session, `remote_review.clear()`, close Connect dialog, status `AUTH_FAIL_SOLO_STATUS`, error *Session expired (401). Reconnect when ready.* — **no** best-effort logout (bearer already dead) |
| Wired from Review path | Yes | `app.rs` Connected Review branch after `remote_review_ui::show`: `if self.remote_review.has_auth_failure() { self.force_disconnect_auth_fail(); }` |
| Codes Unauthorized wording | Yes | `CodesApplyErr::Unauthorized` → `"Session expired or unauthorized (401)"` (detectable) |
| Client 401 mapping | Yes | `authed_get` / `apply_codes` / `map_error_response` → `RemoteError::Unauthorized`; Display includes `(401)` |
| Tests | Yes | `auth_failure_message_and_session_clear` (`remote_client`); `has_auth_failure_detects_401_surfaces`, `auth_fail_helper_clears_session` (`remote_review_ui`) |

**§3.3 mode machine:** Auth fail mid-session → Solo + clear token + message — **satisfied**. Sticky half-dead Connected mode closed.

### P2-2 — Body single-flight (not fire-and-forget pool) — **FIXED**

| Check | Result | Evidence |
|---|---|---|
| Dedicated worker | Yes | One `desk-remote-body` thread per `RemoteReviewState` via `ensure_body_worker` + `body_job_tx` |
| Job queue + drain-to-latest | Yes | `body_worker_loop` → `take_latest_body_job` drains `try_recv` before each blocking `get_item_body` |
| At most one active body HTTP | Yes | Single worker loop; next job only after current fetch completes |
| Generation latest-wins apply | Yes | `poll` discards `gen != body_gen`; `body_result_is_current` helper retained |
| `select_index` no longer spawns N threads | Yes | Sends `BodyJob` on `body_job_tx`; replaces `body_rx` only (stale senders disconnect) |
| Worker lifecycle | Yes | `clear()` resets state → drops `body_job_tx` → worker `recv` fails → exit (no orphan pool) |
| Timeouts still bound hung HTTP | Yes | `RemoteClient::new` connect 5s / request 30s |
| Tests | Yes | `body_single_flight_drains_to_latest_gen`, `body_channel_drain_keeps_only_latest`, `stale_body_discarded_after_navigate` |

**§3.7.1 blocking fallback:** *“a **single** dedicated network worker + generation token + cooperative cancel”* — **implemented**. Not true HTTP abort (async/`AbortHandle` still residual), but **not** an unbounded fire-and-forget pool. In-flight fetch for a superseded selection may finish (≤ timeout) before the next; queued jobs collapse to latest. **DoD-7 Met.**

---

## Scope Reviewed

| Area | Paths |
|---|---|
| Desk HTTP + session | `crates/dedupe-desk/src/remote_client.rs` |
| Connect dialog / SSO / mode guards | `crates/dedupe-desk/src/connect.rs` |
| Thin remote review + OCC + body worker | `crates/dedupe-desk/src/remote_review_ui.rs` |
| App wiring (banner, dual-open, produce, 401) | `crates/dedupe-desk/src/app.rs` |
| Produce params / pre-flight | `crates/dedupe-desk/src/params.rs` |
| Docs / deferred / features | `docs/operator-golden-path.md`, `docs/deferred.md`, READMEs (spot), `conductor/features.md` |
| Prior audit / fixes | `review.subagent-r1.md`, `fix-notes-r1.md` |
| Closeout | **No** track `review.md` yet; registry closeout is orchestrator |

**Not in scope:** product code changes (audit only).

---

## Requirement and DoD Matrix

| DoD | Status | Evidence |
|---|---|---|
| **DoD-1 — Connect** | **Met** | Home Connect dialog; `healthz` + `POST /v1/login`; in-memory `ConnectedSession`; banner + Disconnect (`connect.rs`, `remote_client.rs`, `app.rs`). |
| **DoD-2 — Remote mutate + 409** | **Met** | List/body/codes; `expected_version`; no body `actor`; 409 retains draft + conflict UI + Retry/Discard (`remote_review_ui.rs`). |
| **DoD-3 — Mode fail-closed** | **Met** | Dual-open guards; Connected nav/produce limits; Disconnect → Solo; **401 → `force_disconnect_auth_fail`** (r1 P2 fixed). |
| **DoD-4 — Produce UX (Solo)** | **Met** | Profile dropdown; required `bates_start` ≥ 1; `produce_preflight`; Connected produce blocked. **D-0060-02** closed in deferred. |
| **DoD-5 — Solo regression** | **Met (static)** | Connect opt-in; local Review when not Connected; no Solo path replaced by stubs. |
| **DoD-6 — SSO (soft)** | **Met (a)** | Loopback handoff + exchange; nonblocking accept + **3 min deadline**; no production clipboard paste. Advances **D-0059-02**. |
| **DoD-7 — Networking** | **Met** | UI never blocks on HTTP; single-flight body worker + drain-to-latest + generation gate (r1 P2 fixed). |
| **DoD-8 — Tests** | **Met on inventory** | §3.9 cases present (mode, secrets, loopback, codes JSON, 409 draft, body drain/gen, auth-fail, produce preflight, service handoff units). **Gates:** green per `fix-notes-r1.md`; not re-executed in r2 process. |
| **DoD-9 — Docs** | **Met** | Path C Connect; produce profile notes; features.md Connect rows shipped (r1 P3 Future row fixed); deferred D-0058-01/D-0060-02 closed; D-0064-01..06 residuals honest. |
| **DoD-10 — Recorded** | **Unmet (expected)** | No track `review.md`; registry Completed + ledger commit are orchestrator closeout steps. |

### Spec §3.9 test matrix

| Case | Present? | Location / notes |
|---|---|---|
| Mode: cannot open local while Connected | Yes | `connect.rs` `mode_guards_refuse_dual_open` + app gates |
| Mode: Connect requires healthz + login | Partial | Implemented in blocking connect path; **no** automated HTTP mock (P3 residual) |
| Login failure leaves Solo | Yes | `login_failure_leaves_solo_semantics` |
| Remote codes sends `expected_version` | Yes | `remote_client` + `remote_review_ui` |
| 409 retains draft; conflict flag | Yes | `apply_conflict_retain_draft` + UI path |
| Stale body discarded (generation) | Yes | `stale_body_discarded_after_navigate` + single-flight drain tests |
| Body actor not on mutate | Yes | `build_codes_request_json` / struct fields |
| produce_params profile + bates | Yes | `params.rs` tests; **no silent `.max(1)` clamp** (r1 P3 fixed) |
| Produce pre-flight blocks invalid | Yes | `produce_preflight` / validate_bates |
| Solo produce starts when pre-flight ok | Partial | Path in `app.rs`; no new e2e job harness |
| SSO handoff rejects non-loopback | Yes | Desk + service unit tests |
| Token/password not in Debug | Yes | `secrets_debug_redacted` |
| No production clipboard paste path | Yes | Code review + hover/docs |
| **401 → Solo / clear session** | Yes | Helpers + `has_auth_failure` tests; app wiring static |
| **Body single-flight drain** | Yes | `body_single_flight_drains_to_latest_gen`, `body_channel_drain_keeps_only_latest` |

### §3.11 fold disposition

| Fold | Disposition in tree |
|---|---|
| 1 Async/cancel networking | **Met** (blocking fallback: single worker + gen + drain) |
| 2 Ban clipboard paste; loopback handoff | **Met** |
| 3 Non-destructive 409 | **Met** |
| 4 Produce pre-flight | **Met** |

---

## Findings

### [P3] No mock/in-process HTTP integration for Connect login / remote codes 409
Confidence: **Medium**  
Requirement: §3.9 “unit with mock or service router”; plan Phase 3 mock/router.  
Location: Desk tests pure unit; matter-service has handoff URL units, not Desk↔router round-trip.  
Problem: Healthz+login sequence and full 409 retain path not exercised against a live router.  
Failure scenario: API DTO drift breaks Connect until manual pilot.  
Correction: Optional wiremock/`axum` in-process test for login + codes 409.  
Verification: New integration test green in CI.  
Deferrable: **Yes** — pure unit coverage of builders/mode/auth-fail/drain is present.

### [P3] SSO Cancel does not self-connect to unblock listener early
Confidence: **Medium**  
Requirement: Usability/resource hygiene on Cancel during SSO.  
Location: `connect.rs` `sso_loopback_blocking`; `ConnectDialogState::close` drops `rx`.  
Problem: r1 fixed **3-minute nonblocking accept deadline** (no infinite hang). Dialog Cancel still leaves worker polling until timeout or a connection arrives; no self-connect unblock / shared cancel flag.  
Failure scenario: Operator Cancel → worker holds ephemeral port ≤ 3 min.  
Correction: On dialog close, connect to bound port or atomic cancel.  
Verification: Manual Cancel during SSO; worker exits promptly.  
Deferrable: **Yes** (timeout bounds worst case; residual polish).

### [P3] Body HTTP is cooperative-cancel only (no true abort of in-flight request)
Confidence: **Low–Medium**  
Requirement: §3.7.1 preferred async + abort; blocking fallback allows single-flight.  
Location: `remote_review_ui::body_worker_loop` still uses `reqwest::blocking` for the active job.  
Problem: Superseded selection’s in-flight body may run to completion (≤ 30s) before the latest job starts. **Not** unbounded concurrency.  
Failure scenario: Slow host + rapid nav → up to one stale transfer delay before latest body.  
Correction: Async reqwest + generation-scoped abort (future polish residual).  
Verification: Optional residual **D-0064-*** if product wants true abort.  
Deferrable: **Yes** — DoD-7 Met under accepted blocking fallback; not a reopen of r1 P2.

---

## r1 findings disposition

| r1 ID | Severity | r2 disposition |
|---|---|---|
| Mid-session 401 no Solo | P2 | **Fixed** — `force_disconnect_auth_fail` + tests |
| Body fire-and-forget pool | P2 | **Fixed** — dedicated worker + drain-to-latest + tests |
| SSO accept no timeout | P3 | **Fixed** — nonblocking + 180s deadline |
| No-op stale_body generation test | P3 | **Fixed** — replaced with auth-fail helper test |
| Mock HTTP integration | P3 | **Open** (this report) |
| `produce_params` `.max(1)` clamp | P3 | **Fixed** — clamp removed; pre-flight gates |
| features.md “Future: Connect” | P3 | **Fixed** — shipped row |
| SSO Cancel self-unblock | P3 | **Open** (this report; timeout mitigates) |

---

## Completeness Sweep

| Check | Result |
|---|---|
| Stubs / TODO for Connect core | None for password Connect, remote list/body/codes, produce profile |
| Fake success | Connect Err stays Solo; healthz requires `ok:true` |
| No-op 401 path | **Gone** — real disconnect |
| Unbounded body thread pool | **Gone** — single worker |
| Clipboard bearer paste production path | Absent |
| SSO | Loopback + service exchange + timeout |
| Dual-open hybrid | Guards present and wired |
| Remote notes/privilege/locks/jobs/produce | Intentionally residual (D-0064-01/02) |
| Secrets | Bearer/password zeroize + redacted Debug; no token in banner |
| Logging tokens | No bearer/password tracing in remote_client/connect |
| Handoff security | Loopback-only; one-time exchange (service-side) |
| Reqwest pin | Workspace 0.12 blocking+json+rustls; timeouts set |

---

## Wiring and Regression Review

```text
Connect dialog
  → normalize_base_url → healthz → login | SSO loopback (≤3m) → exchange
  → ConnectedSession (memory) → banner + Review (remote)
  → remote_review_ui: list / body(single-flight worker+gen) / codes(expected_version)
  → 409: retain draft + conflict UI → Retry/Discard
  → 401 surfaces → force_disconnect_auth_fail → Solo + clear token
  → Disconnect: best-effort logout thread + clear session

Solo (default)
  → local matter open (refused if Connected)
  → full Review / jobs
  → Produce: profile dropdown + bates_start + pre-flight → process-runner produce
```

| Risk | Assessment |
|---|---|
| Hybrid dual-open | Mitigated |
| 409 draft wipe | Mitigated |
| UI-thread HTTP | Mitigated |
| Unbounded body work | **Mitigated (r2)** |
| Sticky Connected after 401 | **Mitigated (r2)** |
| Solo regression | Unlikely broken |

---

## Verification Evidence

**This session (r2):** static code/doc re-audit of fixed paths; P2 wiring confirmed by direct inspection of:

- `C:\dev\Dedupe\crates\dedupe-desk\src\remote_client.rs` (`is_auth_failure_message`, `force_clear_connected_session`, 401 mapping, tests)
- `C:\dev\Dedupe\crates\dedupe-desk\src\remote_review_ui.rs` (`has_auth_failure`, `ensure_body_worker`, `body_worker_loop`, `take_latest_body_job`, tests)
- `C:\dev\Dedupe\crates\dedupe-desk\src\app.rs` (`force_disconnect_auth_fail`, Review 401 branch)
- `C:\dev\Dedupe\crates\dedupe-desk\src\connect.rs` (SSO nonblocking deadline)
- `C:\dev\Dedupe\crates\dedupe-desk\src\params.rs` (no Bates clamp)
- `C:\dev\Dedupe\conductor\features.md`, `docs/deferred.md` (spot)

**From `fix-notes-r1.md` (implementer session):**

```text
cargo fmt --all                                          OK
cargo clippy -p dedupe-desk -p matter-service --all-targets -- -D warnings   OK
cargo test -p dedupe-desk --bin dedupe-desk              149 passed
cargo test -p matter-service                             10 unit + 13 integration passed
```

**Recommended for orchestrator closeout (re-run live):**

```powershell
cargo test -p dedupe-desk --bin dedupe-desk
cargo test -p matter-service
cargo clippy -p dedupe-desk -p matter-service --all-targets -- -D warnings
cargo fmt --all --check
# Full gate before commit:
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

---

## Deferred Candidates

| ID | Item | Notes |
|---|---|---|
| D-0064-01 | Full Connected parity | Already recorded |
| D-0064-02 | Remote produce HTTP | Already recorded |
| D-0064-03 | Keyring bearer persist | Already recorded |
| D-0064-04 | Custom URI scheme | Already recorded |
| D-0064-05 | Dev clipboard paste | Banned for production |
| D-0064-06 | Multi-instance handoff codes | Already recorded |
| *(optional P3)* | Mock HTTP Connect/409 tests | This report |
| *(optional P3)* | SSO Cancel self-unblock | Timeout already bounds hang |
| *(optional P3)* | Async body abort | Beyond blocking fallback DoD-7 |
| Closed | D-0058-01, D-0060-02 | Correctly **closed** |
| Advanced | D-0059-02 | Loopback SSO landed |

---

## Completion Decision

| Gate | Result |
|---|---|
| Any P0–P2? | **No** → not FAIL |
| Core product DoDs 1–7 | **Met** |
| DoD-8 | Inventory Met; cargo green per fix-notes (re-run at closeout) |
| DoD-9 | **Met** |
| DoD-10 | Unmet until orchestrator `review.md` + registry + ledger |
| Open P3 only | **Yes (3)** → **PASS WITH DEFERRED P3** |

### Required for track Completed (orchestrator)

1. Re-run targeted tests + clippy; paste into final `review.md`.  
2. DoD-10: `review.md`, `conductor.md` **Completed**, ledger commit (`GUI` / `FEATURE`).  
3. Optionally append residual rows for mock HTTP / true body abort / SSO Cancel if desired — not blocking.

### Not required to ship

- Mock HTTP integration  
- SSO Cancel self-connect  
- Async abort for body (cooperative single-flight already satisfies §3.7.1 fallback)

---

*End of subagent r2 completion audit.*
