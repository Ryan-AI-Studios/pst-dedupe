# Track Completion Audit — 0064-DeskPlatformConnectUx

| Field | Value |
|---|---|
| **Track** | 0064-DeskPlatformConnectUx |
| **Branch** | `feat/0064-desk-platform-connect-ux` |
| **Auditor role** | Read-mostly completion reviewer (subagent r1) |
| **Date** | 2026-07-25 |
| **Authority** | `conductor/0064-DeskPlatformConnectUx/spec.md`, `plan.md` |
| **Method** | Full static inspection of Desk Connect/remote/produce modules, matter-service OIDC handoff, docs/deferred, §3.9 test inventory. Targeted `cargo` gates **not executed in this session** (no shell tool in reviewer context). |

## Verdict: **FAIL**

**Finding counts:** **0 P0 · 0 P1 · 2 P2 · 5 P3**

Product surface for Connect + thin remote review + Solo produce profile is largely implemented and wired. Track fails completion because two **P2** gaps violate locked mode/networking rules (DoD-3 partial, DoD-7 partial). Fix those (or justify + residual with product owner) before PASS.

---

## Scope Reviewed

| Area | Paths |
|---|---|
| Desk HTTP + session | `crates/dedupe-desk/src/remote_client.rs` |
| Connect dialog / SSO / mode guards | `crates/dedupe-desk/src/connect.rs` |
| Thin remote review + OCC | `crates/dedupe-desk/src/remote_review_ui.rs` |
| App wiring (banner, dual-open, produce, 401 stub) | `crates/dedupe-desk/src/app.rs` |
| Produce params / pre-flight | `crates/dedupe-desk/src/params.rs` |
| Module registration | `crates/dedupe-desk/src/main.rs` |
| Desk deps | `crates/dedupe-desk/Cargo.toml` (`reqwest`, `zeroize`, `url`, tokio features) |
| Service handoff | `crates/matter-service/src/routes.rs`, `oidc.rs` |
| Docs | `docs/operator-golden-path.md`, `docs/deferred.md`, `crates/dedupe-desk/README.md`, `crates/matter-service/README.md`, `conductor/How-to-use.md`, `conductor/features.md` |
| Registry | `conductor/conductor.md` (still **Ready**, not Completed) |
| Closeout artifact | **No** `review.md` yet (this file is subagent r1) |

**Not in scope of rewrite:** product code changes (audit only).

---

## Requirement and DoD Matrix

| DoD | Status | Evidence |
|---|---|---|
| **DoD-1 — Connect** | **Met** | Home **Connect to matter-service…** dialog: base URL (default `http://127.0.0.1:7749`), username/password; background `healthz` + `POST /v1/login`; `ConnectedSession` in process memory; persistent banner `Connected to {base} as {name} ({role})` + Disconnect in banner/nav/home (`app.rs` ~2266–2294, ~3108–3119; `connect.rs` `start_password_connect` / `connect_password_blocking`; `remote_client.rs` `healthz`/`login`/`banner_text`). |
| **DoD-2 — Remote mutate + 409** | **Met** | Connected Review: list `GET /v1/items`, body `…/body`, codes `POST …/codes` with `expected_version`, no body `actor` (`RemoteApplyCodesRequest`). 409 → retain draft codes, conflict panel, Retry with new version / opt-in Discard (`remote_review_ui.rs` conflict path + UI). `read_only` disables mutates. |
| **DoD-3 — Mode fail-closed** | **Partial** | Dual-open refused: `can_open_local_matter` / `can_connect_with_local_matter` + app create/open/Connect gates; Connected nav limited to Home+Review; produce refused when Connected. **Gap:** mid-session **401 does not drop to Solo / clear token** (empty branch in `app.rs` Review path) — violates §3.3. Disconnect otherwise returns Solo cleanly. |
| **DoD-4 — Produce UX (Solo)** | **Met** | Produce dialog: production profile dropdown (`list_production_profiles`), required Bates start ≥ 1, `produce_params(…, production_profile, bates_start)`, pre-flight resolve/validate/bates/QC before job start; Connected produce blocked. Closes **D-0060-02** in deferred. |
| **DoD-5 — Solo regression** | **Met (static)** | Connect is opt-in; local Review branch still used when not Connected; produce/local open paths gated only when Connected; no Solo path replacement with stubs. |
| **DoD-6 — SSO (soft)** | **Met (a)** | Automatic loopback handoff implemented: Desk binds `127.0.0.1:0`, browser to `/v1/oidc/login?handoff_url=…`, service issues one-time code + redirect, Desk redeems `POST /v1/oidc/exchange`. **No** production clipboard bearer paste UI. Advances **D-0059-02**; residuals D-0064-04/05/06 honest. |
| **DoD-7 — Networking** | **Partial** | UI thread never blocks on HTTP (worker threads + `request_repaint`). Body results are **generation-gated latest-wins**. **Gap:** each selection spawns a **new fire-and-forget blocking** body thread without cancelling prior HTTP; `body_rx` is replaced so prior results drop, but in-flight blocking work continues — conflicts with §3.7.1 “not a pool of fire-and-forget blocking tasks without cancel” / “at most one active body load … or cancel previous first”. Async reqwest not used (workspace pin is blocking+json+rustls). |
| **DoD-8 — Tests** | **Partial → Met on inventory, gates unrun** | §3.9 unit coverage present for mode guards, secrets Debug redaction, loopback URL, codes JSON (no actor + expected_version), 409 draft retain, body gen latest-wins helper, produce params/preflight, service loopback handoff URL. **Weak:** `remote_client::stale_body_generation_discard` is a trivial identity assert. No mock/in-process HTTP client integration for login/409 end-to-end. Clippy/tests **not re-run here**. |
| **DoD-9 — Docs** | **Met** | Operator Path C; Path A produce profile note; How-to Connect + Solo produce; Desk README modes matrix; matter-service README Desk Connect + OIDC exchange; features.md marks 0064 shipped; deferred closes D-0058-01 / D-0060-02, advances D-0059-02, appends D-0064-01..06. Minor residual: features.md still has one “Future: Connect” row (§3.3 UI map) while §6 marks shipped. |
| **DoD-10 — Recorded** | **Unmet (expected pre-closeout)** | No track `review.md` yet; `conductor.md` still **Ready**; ledger commit not verified. This subagent report is intermediate. |

### Spec §3.9 test matrix

| Case | Present? | Location / notes |
|---|---|---|
| Mode: cannot open local while Connected | Yes | `connect.rs` `mode_guards_refuse_dual_open` + app gates |
| Mode: Connect requires healthz + login | Partial | Implemented in `connect_password_blocking`; **no** automated HTTP mock |
| Login failure leaves Solo | Yes (semantic) | `login_failure_leaves_solo_semantics`; app never sets session on Err |
| Remote codes sends `expected_version` | Yes | `remote_client` + `remote_review_ui` tests |
| 409 retains draft; conflict flag | Yes | `apply_conflict_retain_draft` + UI path mirrors it |
| Stale body discarded (generation) | Yes (helper) | `body_result_is_current`; runtime poll uses same rule |
| Body actor not on mutate | Yes | `build_codes_request_json` / struct fields |
| produce_params profile + bates | Yes | `params.rs` tests |
| Produce pre-flight blocks invalid | Yes | `produce_preflight_blocks_invalid_bates_and_profile` |
| Solo produce starts when pre-flight ok | Partial | Path exists; no new end-to-end job test beyond existing patterns |
| SSO handoff rejects non-loopback | Yes | Desk `is_loopback_handoff_url` + service `assert_loopback_handoff_url` unit tests |
| Token/password not in Debug | Yes | `secrets_debug_redacted` |
| No production clipboard paste path | Yes (design/code review) | No paste token field; hover text + docs ban it |

### §3.11 fold disposition

| Fold | Disposition in tree |
|---|---|
| 1 Async/cancel networking | **Partial** — latest-wins yes; single-flight/abort **no** (P2) |
| 2 Ban clipboard paste; loopback handoff | **Met** |
| 3 Non-destructive 409 | **Met** |
| 4 Produce pre-flight (no invented LO/GS checks) | **Met** |

---

## Findings

### [P2] Mid-session 401 does not drop to Solo / clear token
Confidence: **High**  
Requirement: Spec §3.3 — *Auth failure mid-session (401) → Drop to Solo; clear token; message*; DoD-3.  
Location: `crates/dedupe-desk/src/app.rs` (Review Connected branch ~3161–3168).  
Problem: Code detects remote review errors containing `"401"` / `"unauthorized"` but the branch body is a comment only (“Leave message; operator can Disconnect. Auto-drop on next poll if desired.”). Session remains Connected with a dead bearer.  
Evidence:
```text
// Drop session to Solo on 401 surfaces from remote review.
if self.remote_review.error.as_deref()
    .is_some_and(|e| e.contains("401") || e.contains("unauthorized"))
{
    // Leave message; operator can Disconnect. Auto-drop on next poll if desired.
}
```
`RemoteError::Unauthorized` is mapped on the client (`remote_client.rs`) and surfaced as error text from codes path, but app never calls `disconnect_session()` / clears `connected_session`.  
Failure scenario: Token expires or admin invalidates session mid-review. Operator keeps Connected banner/mode; further mutates fail; dual-open guards still block local Solo open until manual Disconnect — sticky half-dead Connected mode.  
Correction: On 401/Unauthorized (list/body/codes), call the same disconnect path as user Disconnect (best-effort logout optional), clear session + remote review state, status/error message “Session expired — returned to Solo.” Unit test: simulate Unauthorized → `is_connected()==false`.  
Verification: `cargo test -p dedupe-desk --bin dedupe-desk`; manual: Connect, invalidate session server-side, mutate → Solo.  
Deferrable: **No** (locked mode machine).

---

### [P2] Body loads are fire-and-forget blocking threads without cancel / single-flight
Confidence: **High**  
Requirement: Spec §3.7.1 / fold §3.11 #1 / DoD-7 — UI never blocks (ok); body requests abortable **or** single dedicated worker + cancel; **not** a pool of fire-and-forget blocking tasks; at most one active body load (or cancel previous first).  
Location: `crates/dedupe-desk/src/remote_review_ui.rs` `select_index` (~296–328), `poll` body branch (~158–198); `remote_client.rs` uses `reqwest::blocking::Client` only.  
Problem: Each item selection increments `body_gen` and spawns a **new** named thread running blocking `get_item_body`. Prior `body_rx` is replaced (stale results discarded — good), but prior HTTP requests are **not** aborted and prior threads keep running until timeout/complete. Rapid list navigation can stack many concurrent blocking body fetches.  
Evidence: `thread::Builder::new().name("desk-remote-body")` per select; no `JoinHandle` retention, no single network worker, no reqwest async/`AbortHandle`. Comment claims “cancels/stales” but only generation discard applies.  
Failure scenario: Operator clicks quickly through a large remote list on a slow host → many parallel 30s blocking HTTP calls; memory/connection pressure; “stuck” feel if server is slow (UI still paints, but network is unbounded).  
Correction (any one of):  
1. Prefer async `reqwest` + generation-scoped `AbortHandle` / drop of future on navigate; or  
2. Single dedicated network worker thread: queue at most one body job, cancel/replace on new selection (cooperative cancel via generation + stop starting new work until prior finishes is **not** enough alone — prefer drop/abort); or  
3. At minimum: track one in-flight body; do not spawn a new body thread until previous completes **or** use a cancellable client.  
Verification: Unit already covers generation discard; add stress/logic test that concurrent select does not leave N active jobs if single-flight is implemented; manual rapid click.  
Deferrable: **No** for full DoD-7 Met (locked fold). Could residual as **D-0064-*** only with explicit product acceptance of Partial DoD-7 — not recommended without owner sign-off.

---

### [P3] SSO loopback `accept()` has no wait timeout / Cancel does not stop listener
Confidence: **Medium**  
Requirement: Usability/resource hygiene; SSO flow “one login attempt”; disconnect/cancel should not leave zombie workers.  
Location: `crates/dedupe-desk/src/connect.rs` `sso_loopback_blocking` (~241–274); `ConnectDialogState::close` drops `rx` but does not signal listener.  
Problem: Comment says “up to 3 minutes” but `listener.accept()` has no deadline. Closing the Connect dialog during SSO drops the receiver; the worker remains blocked on `accept` until an arbitrary connection arrives or process exit.  
Failure scenario: Operator clicks SSO then Cancel → orphan thread holding an ephemeral port until something hits it.  
Correction: `set_nonblocking` + timed poll loop, or `accept` with OS-level timeout / select; on dialog close, connect to self to unblock or use a shared cancel flag.  
Verification: Manual Cancel during SSO; no orphan thread after timeout.  
Deferrable: **Yes** (P3 polish; residual D-0064-* ok).

---

### [P3] `remote_client::stale_body_generation_discard` test is a no-op
Confidence: **High**  
Requirement: §3.9 stale body case should be meaningful.  
Location: `crates/dedupe-desk/src/remote_client.rs` tests ~623–631.  
Problem: Test only asserts `2 != 3` and `3 == 3` without calling production helpers. Real policy is tested in `remote_review_ui::stale_body_discarded_after_navigate` via `body_result_is_current`.  
Failure scenario: None in product; false confidence if `remote_review_ui` test is deleted.  
Correction: Delete no-op test or call shared helper; keep one strong test.  
Verification: `cargo test -p dedupe-desk stale_body`.  
Deferrable: **Yes**.

---

### [P3] No mock/in-process HTTP integration for Connect login / remote codes 409
Confidence: **Medium**  
Requirement: §3.9 “unit with mock or service router”; plan Phase 3 “mock or in-process service router”.  
Location: Desk tests are pure unit; matter-service has handoff URL unit tests but not Desk↔service round-trip for exchange/login in Desk crate.  
Problem: Healthz+login sequence and full 409 retain path are not exercised against a router.  
Failure scenario: API DTO drift (field rename) breaks Connect silently until manual pilot.  
Correction: Optional `tower`/`axum` in-process test or `mockito`/wiremock for login + codes 409.  
Verification: New integration test green in CI.  
Deferrable: **Yes** (P3); pure unit coverage of builders/mode is present.

---

### [P3] `produce_params` still clamps `bates_start` with `.max(1)`
Confidence: **Low–Medium**  
Requirement: §3.6 — no silent hardcode of Bates start without operator visibility.  
Location: `params.rs` `produce_params` (`let start = bates_start.max(1)`).  
Problem: UI + pre-flight require ≥ 1; pure helper still coerces 0→1 if called incorrectly. Defense-in-depth vs silent product default — low risk.  
Failure scenario: Future caller bypasses pre-flight and gets Bates 1 without error.  
Correction: Return validation error or rely solely on `validate_bates_start` without clamp.  
Verification: Unit assert `produce_params` with 0 either errs or is unused.  
Deferrable: **Yes**.

---

### [P3] features.md still lists Connect as “Future” in one UI map row
Confidence: **High**  
Requirement: DoD-9 honesty; features catalog aligned.  
Location: `conductor/features.md` ~348 “Future: Connect to service | **0064**” vs ~535 “**Shipped (impl)**”.  
Problem: Conflicting rows.  
Correction: Update UI map row to Shipped / thin remote review.  
Verification: Doc skim.  
Deferrable: **Yes**.

---

## Completeness Sweep

| Check | Result |
|---|---|
| Stubs / TODO for Connect core | **None found** for password Connect, remote list/body/codes, produce profile |
| Fake success | Connect Err stays Solo; healthz requires `ok:true`; codes require real HTTP |
| No-op 401 path | **Yes — P2** (comment-only) |
| Clipboard bearer paste production path | **Absent** (banned; residual D-0064-05) |
| SSO | Real loopback + service exchange (not residual-only) |
| Dual-open hybrid | Guards present and wired |
| Remote notes/privilege/locks/jobs/produce | Intentionally residual (matrix + D-0064-01/02) |
| Secrets | `BearerToken` / `SecretString` zeroize + redacted Debug; password cleared after attempt; no token in banner |
| Logging tokens | No tracing of bearer/password in remote_client/connect |
| Handoff security | Service re-validates loopback + path `/connect/callback`; one-time remove-on-exchange; 120s TTL |
| Reqwest pin | Workspace `0.12` blocking+json+rustls on dedupe-desk |

---

## Wiring and Regression Review

```text
Connect dialog
  → normalize_base_url → healthz → login | SSO loopback → exchange
  → ConnectedSession (memory) → banner + Review (remote)
  → remote_review_ui: list / body(gen) / codes(expected_version)
  → 409: retain draft + conflict UI → Retry/Discard
  → Disconnect: best-effort logout thread + clear session

Solo (default)
  → local matter open (refused if Connected)
  → full Review / jobs
  → Produce: profile dropdown + bates_start + pre-flight → process-runner produce
```

| Risk | Assessment |
|---|---|
| Hybrid dual-open | Mitigated (guards + Connected create/open disabled) |
| 409 draft wipe | Mitigated |
| UI-thread HTTP | Mitigated (background threads) |
| Unbounded body work | **Residual risk — P2** |
| Sticky Connected after 401 | **Residual risk — P2** |
| Solo regression | Unlikely broken by Connect-only branches |

---

## Verification Evidence

**This session:** static code/doc audit only (no `cargo` execution available to the reviewer process).

**Recommended before closeout:**

```powershell
cargo test -p dedupe-desk --bin dedupe-desk
cargo test -p matter-service
cargo clippy -p dedupe-desk -p matter-service --all-targets -- -D warnings
cargo fmt --all --check
# Full gate before commit:
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Orchestrator should paste actual pass/fail output into final `review.md`.

---

## Deferred Candidates

| ID | Item | Notes |
|---|---|---|
| D-0064-01 | Full Connected parity | Already recorded — honest |
| D-0064-02 | Remote produce HTTP | Already recorded |
| D-0064-03 | Keyring bearer persist | Already recorded |
| D-0064-04 | Custom URI scheme | Already recorded |
| D-0064-05 | Dev clipboard paste | Banned for production — already recorded |
| D-0064-06 | Multi-instance handoff codes | Already recorded |
| *(new if P2#2 residualed)* | Body single-flight / abort | Only with owner acceptance of Partial DoD-7 |
| *(P3)* | SSO accept timeout / cancel | Polish |
| *(P3)* | Mock HTTP Connect/409 tests | Polish |
| Closed | D-0058-01, D-0060-02 | Correctly marked **closed** in `docs/deferred.md` |
| Advanced | D-0059-02 | Loopback SSO landed; residual polish noted |

---

## Completion Decision

| Gate | Result |
|---|---|
| Any P0–P2? | **Yes — 2× P2** → **FAIL** |
| Core product DoDs 1–2, 4–6 | Met |
| DoD-3, DoD-7 | Partial (P2s) |
| DoD-8 | Inventory Met; gates unrun here |
| DoD-9 | Met (minor P3 docs) |
| DoD-10 | Unmet until orchestrator `review.md` + registry + ledger |

### Required for PASS

1. **Fix P2-401:** On Unauthorized/401 from remote review, clear session → Solo + operator message (test).  
2. **Fix P2-body:** Single-flight or abortable body loads per §3.7.1 (not unbounded fire-and-forget blocking threads).  
3. Re-run targeted tests + clippy on `dedupe-desk` / `matter-service`.  
4. Then DoD-10 closeout: `review.md`, `conductor.md` Completed, deferred already largely updated, ledger commit.

### Optional P3 (may ship deferred)

- SSO accept timeout / cancel unblocks listener  
- Delete/strengthen no-op generation test  
- Mock HTTP integration  
- features.md Future row cleanup  
- `produce_params` clamp honesty  

---

*End of subagent r1 completion audit.*
