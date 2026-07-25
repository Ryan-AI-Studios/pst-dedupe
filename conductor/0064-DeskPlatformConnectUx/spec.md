# 0064 — Desk platform Connect + operator UX pack

- **Track ID:** 0064-DeskPlatformConnectUx  
- **Execution repo:** `C:\dev\dedupe`  
- **Governance:** this directory in `C:\dev\dedupe\conductor\`  
- **Plan-of-record:** Series J consolidation — operator usability for Series I multi-user / production-profile surfaces  
- **Status:** **Completed** — 2026-07-25; Codex luna PASS WITH DEFERRED P3; closes D-0058-01 / D-0060-02
- **Depends on:** Hard **0058** (service API), **0060** (production profiles). Soft **0059** (OIDC) for SSO entry. Soft **0062/0063** Completed preferred (freeze + red-team baseline).  
- **Closes / advances:** **D-0058-01** (Desk Connect); **D-0060-02** (produce profile dropdown + Bates start); soft-advances **D-0059-02** (SSO sign-in UX) if §3.5 lands.  
- **Priority:** **P1 consolidation** (UX that makes Series I usable without PowerShell).  
- **Evidence:** synthetic fixtures + in-process/mock HTTP only; **no client data**; no bearer tokens or passwords in git/logs.  
- **Deferred ledger:** roll in §2.5; append **D-0064-*** for residuals.

---

## 1. Objective

Make **opt-in multi-user / production-profile** features reachable from **Dedupe Desk** (native `eframe` / `egui`), so boutique firms are not CLI-only for Series I concurrent review and multi-jurisdiction produce.

| Capability | P0 |
|---|---|
| **Connect to matter-service** | Base URL + password login; bearer session; clear **Connected** mode (**D-0058-01**) |
| **Thin remote review** | List + body + at least one mutating path (codes) with **OCC** + **session actor** |
| **Mode clarity** | Explicit **Solo** vs **Connected**; fail closed on dual-open / hybrid |
| **Produce profile picker (Solo)** | Built-in + matter profiles dropdown; **job-time `bates_start` required** (**D-0060-02**) |
| **SSO entry (soft P1)** | System browser → service OIDC; Desk obtains bearer without password when platform host is up (**D-0059-02**) |

**UI stack (LOCKED):** native Windows **`dedupe-desk`** only — **not** WASM, not a separate web SPA.

**Outcome:** Operator can (1) run `service serve` on a host and review from Desk over loopback HTTP, and (2) pick a production profile + Bates start in Solo produce without CLI.

**Industry anchors (researched 2026-07):**

- **RFC 8252** (OAuth 2.0 for Native Apps): system browser + PKCE for interactive SSO; avoid embedded WebView for credentials.  
- **eDiscovery concurrent review:** batch/lock/OCC patterns already on the service (0058) — Desk must honor versions and locks, not invent a second concurrency model.  
- **Native HTTP client:** workspace `reqwest` **0.12** (`blocking` + `json` + `rustls-tls`) — same pin as `matter-ai`. CORS is irrelevant for a non-browser client.  
- **Secret hygiene (0063):** zeroize sensitive buffers; never log bearer tokens; passwords leave memory after login attempt.

---

## 2. Context (ground truth)

### 2.1 What ships today

| Layer | State (post-0063) |
|---|---|
| Product version | **0.2.0-rc.1** |
| Schema | **SCHEMA_VERSION = 39** |
| **matter-service** | axum **0.8**; bearer sessions; strict actor; OCC `expected_version`; default bind `127.0.0.1:7749` |
| Service routes (review) | `GET /healthz`; `POST /v1/login|logout`; `GET /v1/items`, `/items/{id}`, `/items/{id}/body`; `POST …/codes|notes|privilege`; locks; batches; QC samples |
| Service routes (OIDC) | `GET /v1/oidc/login` (+ `?format=json`); `GET /v1/oidc/callback` → **JSON** `LoginResponse` (token + user); redirect URI fixed to service callback |
| Service **does not** expose | `produce` / process-runner jobs / production-profile list HTTP (jobs remain host-local CLI/Desk Solo) |
| **dedupe-desk** | Local `Matter` + process-runner only; **no** `reqwest` dep; no Connect UI |
| Produce dialog | Prefix/name/QC flags; **`bates_start` hardcoded `1`** in `params::produce_params`; **no** `production_profile` field |
| Processing profiles | Workspace dropdown already (0043) — **not** the same as production profiles (0060) |
| Exclusive open | `.matter.lock` / OS exclusive — Desk Solo + service on same matter must fail closed |

### 2.2 Residual map (this track owns)

| ID | Item | 0064 action |
|---|---|---|
| **D-0058-01** | Desk Connect URL + login + session actor | **Close** (P0) |
| **D-0060-02** | Desk produce profile dropdown | **Close** (P0 Solo) |
| **D-0059-02** | Desk browser SSO UX | **Soft-close or advance** (§3.5); residual if seamless loopback deferred |
| D-0058-02 | LAN TLS | Out — docs warn only |
| D-0058-03..09 | Heartbeat, multi-matter host, field RBAC, … | Out |
| D-0061-10 | Cloud storage admin UI | Out |
| D-0063-05 | egui passphrase widgets zeroize | Out (related polish; optional if touching Connect password field) |

### 2.3 Product modes (LOCKED)

```text
┌──────────── Solo (default) ────────────┐   ┌────── Connected (opt-in) ──────┐
│ Open local matter path                 │   │ No local Matter open          │
│ process-runner jobs (ingest…produce)   │   │ HTTP → matter-service         │
│ free-string actor OK for audit         │   │ Session user_id is actor      │
│ production profile + bates_start UI    │   │ Thin review slice only (P0)   │
└────────────────────────────────────────┘   └───────────────────────────────┘
         Never hybrid: no local write-open while Connected to same matter.
```

| Rule | Detail |
|---|---|
| **Default remains Solo** | Connect is opt-in; offline Desk golden path unchanged |
| **Connected = remote review client** | P0 does **not** remote-run jobs/produce through service (API gap) |
| **Fail closed dual-open** | Refuse local open when Connected; if service holds lock, Solo open errors honestly |
| **Strict actor** | Connected mutates never send body `actor`; service ignores it anyway — client must not pretend |

### 2.4 Dependency pins (current workspace)

| Crate / pin | Use in 0064 |
|---|---|
| `eframe` **0.34** | Desk UI |
| `rfd` **0.17** | Existing file dialogs (Solo) |
| `reqwest` **0.12** (workspace: blocking, json, rustls-tls) | **Add** to `dedupe-desk` for Connect client |
| `zeroize` **1** (workspace) | Bearer / password buffers on drop where practical |
| `serde` / `serde_json` / `camino` | Already on Desk |
| `openidconnect` **4.0** | **Service-side only** — Desk does not re-implement OIDC discovery |
| `tower-http` cors | **Not required** for native client (browser CORS N/A) |

**Do not** add a second HTTP stack (`ureq`, raw hyper) without justification.

### 2.5 Deferred roll-in (reasonable)

| Item | Action |
|---|---|
| Full remote feature parity (FTS, redaction editor, gap, people, clusters, AI, jobs) | **Residual matrix** — document “Connected supports X; Solo for Y” |
| Remote produce / job start via HTTP | **Out** until service gains job routes (future track) |
| Persist bearer in OS keyring across restarts | Optional residual (**D-0064-***) |
| RFC 8252 Desk-owned loopback redirect (change IdP redirect allowlist) | Residual — service fixed callback is P0 security (0059/0063) |
| TLS for `--allow-lan` | Still **D-0058-02** |
| Multi-matter portfolio Connect | Out (**D-0038-05**) |

### 2.6 Product rules (LOCKED)

1. **Native egui only** — no WASM rewrite.  
2. **Never block the UI thread** on network I/O — worker / background thread + `request_repaint` (same discipline as 0072).  
3. **No secrets in logs**, status strings, or audit dumps (mask token prefixes at most).  
4. **Solo regression** — existing local review/produce paths must keep working.  
5. **Synthetic CI only** — no real IdP or client matters in git.  
6. **0063 honesty** — LAN without TLS remains residual; Connected default assumes loopback.  
7. **No destructive 409 refresh** — retain local drafts (§3.4.1).  
8. **No production clipboard bearer paste** — automatic SSO handoff only (§3.5).  
9. **Cancelable network** — body loads latest-wins / abort on navigate (§3.7.1).

---

## 3. In scope

### 3.1 Placement (LOCKED)

| Component | Location |
|---|---|
| Connect session + HTTP client | `crates/dedupe-desk` — new module(s), e.g. `connect.rs` / `remote_client.rs` |
| Connect / disconnect UI | Home or Settings + persistent banner when Connected |
| Thin remote review wiring | `review_ui` / `review_body` / coding paths branch on mode |
| Produce profile + Bates start | Produce screen / dialog — **Solo only** for P0 |
| SSO handoff | Desk ephemeral loopback listener + service post-auth handoff (§3.5); small **matter-service** exchange/redirect if needed |
| Tests | `dedupe-desk` unit + `matter-service` integration reuse / mock transport |
| Docs | `docs/operator-golden-path.md`, `conductor/How-to-use.md`, `crates/dedupe-desk/README.md`, `matter-service` README cross-link |

**Forbidden:** reimplementing review domain in the client; Desk Connected must call the **existing** service JSON API only (plus minimal SSO handoff endpoints if added).

### 3.2 Connect dialog + session (LOCKED) — closes D-0058-01

| Field / control | Rules |
|---|---|
| **Base URL** | Default `http://127.0.0.1:7749`; trim trailing `/`; scheme `http` or `https` only |
| **Username / password** | Password login via `POST /v1/login` `{name,password}` → `{token,user,expires_at}` |
| **Connect** | (1) `GET /healthz` → `{ok:true}`; (2) login; (3) store session in memory |
| **Disconnect** | `POST /v1/logout` with bearer (best-effort); clear local session state |
| **Banner** | Always visible when Connected: `Connected to {base} as {display_name} ({role})` |
| **Token storage** | Process memory only for P0; clear on disconnect/exit; **zeroize** password buffer after attempt; prefer zeroizing wrapper for token if low cost |
| **Errors** | Surface `oidc_required` (403) with “use SSO” hint; network errors fail closed (stay Solo) |

**Session struct (conceptual):**

```text
ConnectedSession {
  base_url: String,
  token: Zeroizing<String> | String,  // prefer zeroize
  user_id, display_name, role,
  expires_at: Option<String>,
}
```

### 3.3 Mode state machine (LOCKED)

```text
Solo ──Connect success──► Connected
  ▲                            │
  └──────── Disconnect / auth fail / logout ──┘
```

| Transition guard | Behavior |
|---|---|
| Enter Connected while local matter open | **Refuse** until operator closes local matter (or auto-close with confirm) |
| Open local matter while Connected | **Refuse** — “Disconnect first” |
| Auth failure mid-session (401) | Drop to Solo; clear token; message |
| Service unreachable mid-mutate | Error toast; do not silently write local |

### 3.4 Thin remote review (LOCKED)

**P0 minimum viable Connected review:**

| Action | API | Notes |
|---|---|---|
| List review items | `GET /v1/items` | Thin rows + `review_version` |
| Open body | `GET /v1/items/{id}/body` | Off UI thread; **cancel/stale-drop** on navigate (§3.7) |
| Apply codes | `POST /v1/items/{id}/codes` | **Require** `expected_version`; **non-destructive 409** (§3.4.1) |
| Session actor | Header `Authorization: Bearer …` only | Never send body `actor` |
| Read-only role | Disable mutates when `role == read_only` | Mirror service 403 honesty |

**P1 nice (if cheap after P0):** notes upsert, privilege upsert, item lock/unlock — same OCC rules + §3.4.1. Otherwise residual matrix.

#### 3.4.1 OCC 409 — non-destructive conflict UX (LOCKED)

**Problem:** Blind “refresh on 409” wipes the operator’s in-progress codes / draft notes after another reviewer mutates the same item.

**Rules:**

1. On `409` / `version_conflict`, **forbid** replacing the local editor state with a silent full refresh.  
2. **Retain** all local draft form state (selected codes to add/remove, note text, privilege draft, etc.).  
3. **Fetch** the latest server item (version + current codes/notes summary) in the background.  
4. **Present** an explicit conflict panel, e.g. *“Another reviewer updated this item (version N→M). Your unsaved changes are kept below. Review server state, then re-apply or discard.”*  
5. Re-apply uses the **new** `expected_version` only after the operator confirms (or a clear “Retry with my changes” action).  
6. “Discard my changes and load server” is **opt-in** only — never the default on 409.  
7. Unit/UI-state test: after simulated 409, draft fields remain; conflict flag set.

**Explicitly residual for Connected (document in UI/help):**

- Ingest / extract / profile_run / workflow jobs  
- Produce / QC jobs  
- FTS index rebuild, semantic, people graph, clusters, AI suggest  
- Redaction editor, gap, conversations full parity  
- Local CAS path browsing  

### 3.5 SSO entry (soft P1) — advances D-0059-02

**Constraint:** OIDC is **service-mediated**. PKCE verifier and **IdP redirect URI** remain the service fixed callback (`…/v1/oidc/callback`). Desk must **not** become a second OIDC client against the IdP (would break redirect allowlist and SSRF controls from 0063). RFC 8252-style loopback applies to **native post-auth handoff** after the service issues a session — not to re-homing IdP discovery onto Desk.

#### 3.5.1 Ban clipboard bearer paste as production path (LOCKED)

| Forbidden as P0 / production DoD | Why |
|---|---|
| Operator copies multi-KB bearer into a Desk “paste token” field | Windows Clipboard History / malware scrape; error-prone; bad UX |

Manual paste may exist only as an explicit **dev residual** (`D-0064-*`) behind a non-default path — **not** the operator SSO story and **not** DoD-6 success.

#### 3.5.2 Automatic handoff (LOCKED preference)

```text
Desk                              Browser                         Service
 │ bind 127.0.0.1:ephemeral         │                               │
 │ register handoff (loopback only) │                               │
 │ open system browser ────────────►│ GET /v1/oidc/login            │
 │                                  │── IdP Auth Code + PKCE ──────►│
 │                                  │◄─ service /v1/oidc/callback ──│
 │                                  │   issue session; one-time code│
 │◄── GET /connect/callback?code=…──│ Redirect to Desk loopback     │
 │ redeem one-time code → LoginResponse                             │
 │ drop listener; store session in memory                           │
```

| Mechanism | Notes |
|---|---|
| **A. Ephemeral Desk loopback (preferred)** | Desk listens on `127.0.0.1:random` for one login attempt; service post-auth redirect only to **loopback** URLs (never LAN/public). Prefer a short-lived **one-time exchange code** in the URL — not the long-lived bearer. |
| **B. Custom URI scheme (alt)** | e.g. `dedupe-desk://connect?code=…` if loopback is blocked; same one-time code rules; Windows registration residual-ok if A lands first. |
| **C. Clipboard paste** | **Banned** for production DoD (§3.5.1). |

**Service delta (allowed, minimal):**

1. After successful OIDC, support post-auth handoff to a **Desk-registered loopback** URL (validate: host is loopback, path allowlisted, no open redirect).  
2. Optional `POST /v1/oidc/exchange` (or equivalent) redeems one-time code → `LoginResponse`.  
3. IdP `redirect_uri` **unchanged** (still service callback only).

**Detection:** If `POST /v1/login` returns `oidc_required`, or platform host is configured, show **Sign in with SSO** + optional tenant slug.

**System browser:** Windows `cmd /C start …` or equivalent; never embed IdP password UI in egui WebView.

**Security:** Bearer stays process-memory after exchange; zeroize on disconnect; do not write token to disk in P0; do not log codes/tokens.

### 3.6 Produce profile + Bates start (LOCKED) — closes D-0060-02

**Applies to Solo Desk produce only** (local process-runner). Connected does not start produce jobs in P0.

| Control | Rules |
|---|---|
| **Profile dropdown** | `Matter::list_production_profiles()` — built-ins (`builtin:…`) + matter-local; show name/slug |
| **Default selection** | Default built-in / engine default when none selected (same as CLI omit) |
| **`bates_start` field** | **Required** integer ≥ 1; **no longer hardcode `1` silently** without operator visibility |
| **Params JSON** | Pass `production_profile` (id or slug) + `bates_start` + existing name/prefix/QC flags per `matter-produce` contract |
| **Honesty** | Profile never stores Bates start (0060 lock); job-time only |
| **Validation** | Disable Start until `bates_start` valid; **pre-flight** before start (§3.6.1) |

#### 3.6.1 Solo produce pre-flight (LOCKED)

**Problem:** Dismissing the dialog and failing only after a background job starts is poor UX when the selection is already invalid.

**Honesty about current product:** Built-in production profiles (0060) parameterize **DAT dialect / field map / Bates layout / QC pack binding** — they do **not** currently require LibreOffice, Ghostscript, or other external converters. Do **not** invent tool checks the engine does not define.

**Pre-flight before Start (fail closed, keep dialog open):**

1. **Profile resolves** — `get_production_profile` / resolve selected id or slug succeeds.  
2. **Body validates** — `validate_production_profile_body` (or produce resolve path) succeeds; surface engine error text.  
3. **`bates_start` ≥ 1** — already required in UI.  
4. **QC soft-gate** — existing `produce_require_qc_pass` / readiness check (already in Desk) re-run at click.  
5. **Future external deps** — if a profile later declares required local tools, call the same pre-flight surface; until then residual.

Do **not** start the job if any pre-flight step fails.

**Code touchpoints (expected):**

- `dedupe-desk/src/params.rs` — extend `produce_params(…)`  
- `dedupe-desk/src/app.rs` — produce dialog state + `start_produce`  
- Hydrate profile list on matter open / Produce screen enter  

### 3.7 HTTP client implementation notes (LOCKED)

**Problem:** Unbounded `reqwest::blocking` work on a small thread pool queues body loads when the operator clicks quickly through the review list; in-flight bodies cannot be aborted → “stuck click” UX.

#### 3.7.1 Networking model (LOCKED)

| Requirement | Rule |
|---|---|
| **UI thread** | Never block on network I/O |
| **Preferred** | **`reqwest` async** client + lightweight **tokio** runtime in Desk (expand `tokio` features beyond `sync` as needed; still workspace `reqwest` **0.12** pin with rustls) |
| **Abort / cancel** | In-flight **body** (and similar nav-scoped) requests **must** be abortable or dropped when the operator selects another item / leaves Review |
| **Latest-wins** | Only apply body results whose request generation matches the current selection; stale responses discarded |
| **No unbounded queue** | Do not enqueue one blocking body fetch per click without cancellation; at most one active body load for the selection (or cancel previous first) |
| **Blocking fallback** | If async is deferred, a **single** dedicated network worker + generation token + cooperative cancel is acceptable — **not** a pool of fire-and-forget blocking tasks without cancel |

1. Timeouts: connect + request timeouts (e.g. 5–30s) — no infinite hang.  
2. TLS: rustls via workspace features; loopback HTTP is the default operator path.  
3. **Authorization** header on every authenticated call.  
4. Map HTTP status → user-visible errors (`401`, `403`, `404`, `409`, `5xx`) — 409 UX is §3.4.1.  
5. Unit-test the client against `matter-service` in-process router where practical; test stale body drop / abort.

### 3.8 Docs (LOCKED)

Update at least:

1. `docs/operator-golden-path.md` — Path C: host service + Desk Connect; Path A produce profile note.  
2. `conductor/How-to-use.md` — Connect steps; Solo produce profile.  
3. `crates/dedupe-desk/README.md` — modes table; Connected capability matrix.  
4. `crates/matter-service/README.md` — “Desk Connect (0064)” client section.  
5. `conductor/features.md` — mark planned UI shipped; refresh “last aligned”.  
6. Close/advance deferred rows in `docs/deferred.md` on completion.

### 3.9 Tests (LOCKED)

| Case | Kind |
|---|---|
| Mode: cannot open local matter while Connected | unit |
| Mode: Connect requires healthz + login | unit / integration |
| Login failure leaves Solo | unit |
| Remote codes sends `expected_version` | unit with mock or service router |
| **409 retains local draft**; conflict flag set; no silent wipe | unit |
| Stale body response discarded after navigate (generation) | unit |
| Body actor not sent on mutate | unit |
| `produce_params` includes `production_profile` + operator `bates_start` | unit |
| Produce pre-flight blocks Start on unresolved/invalid profile | unit |
| Solo produce still starts job when pre-flight ok | unit / existing patterns |
| SSO handoff rejects non-loopback post-auth URL | unit (service) |
| Token/password not present in `Debug`/status formatting | unit if custom types |
| No production path that requires clipboard bearer paste | design + code review |

### 3.10 Security checklist (inherit 0063)

| Check | Required |
|---|---|
| No bearer/password in tracing default events | Yes |
| Zeroize password after login attempt | Yes |
| Prefer zeroize bearer on disconnect/drop | Yes (P0 if low cost) |
| Base URL scheme allowlist | Yes |
| Do not disable service bind / TLS residuals in docs | Yes — loopback first |
| Connected mutates use session only | Yes |
| No production clipboard paste of bearer | Yes (§3.5.1) |
| Post-auth handoff URLs loopback-only | Yes |
| One-time exchange codes short TTL + single use | Yes (when SSO lands) |

### 3.11 Review folds accepted (LOCKED summary)

| # | Fold | Spec | Disposition |
|---|---|---|---|
| 1 | **Async / cancel networking** — no unbounded blocking body queue; abort or latest-wins on navigate | §3.7.1 | **Accepted** (prefer async reqwest; blocking only with cancel) |
| 2 | **Ban clipboard bearer paste** as production SSO; automatic loopback (or custom scheme) handoff | §3.5.1–3.5.2 | **Accepted** (service stays OIDC client; Desk is post-auth handoff) |
| 3 | **Non-destructive 409 OCC** — retain draft; conflict UI; no silent refresh wipe | §3.4.1 | **Accepted** |
| 4 | **Produce pre-flight** before Start | §3.6.1 | **Accepted** for resolve/validate/bates/QC; **rejected** inventing LibreOffice/Ghostscript checks (profiles do not require them today) |

---

## 4. Out of scope

- WASM / web Desk rewrite.  
- Full Relativity-class remote review parity.  
- Multi-matter portfolio dashboard.  
- Service-side produce/job HTTP API (new track if needed).  
- Cloud storage settings UI (**D-0061-10**).  
- LAN mTLS (**D-0058-02**).  
- SAML / SCIM / IdP RP logout polish.  
- Security red-team campaign (**0063** — already complete).  
- Desk as a second OIDC client against the IdP (IdP `redirect_uri` stays on service).  
- Invented external-tool pre-flight for produce profiles that the engine does not declare.

---

## 5. Preconditions & dependencies

- **P0 (blocking):** `matter-service` review API stable on `main` (0058).  
- **P0 (blocking):** `list_production_profiles` / produce profile resolve in matter-core + matter-produce (0060).  
- **Soft:** 0059 platform mode for SSO button.  
- **Soft:** 0062/0063 Completed (RC freeze + security baseline).  
- *Verified to date (research 2026-07-25):* service routes listed in §2.1; Desk produce hardcodes `bates_start: 1`; Desk has no HTTP client; OIDC callback returns JSON `LoginResponse`.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Half-connected hybrid (local + remote writes) | Explicit modes; refuse dual-open; exclusive lock honesty |
| Scope creep → full remote Desk | Thin slice + residual matrix in UI/docs |
| Blocking egui / stuck body queue | Async + abort or single-flight latest-wins (§3.7.1) |
| Clipboard bearer scrape | Ban paste for production; loopback one-time code (§3.5) |
| Token leakage via logs/UI | Mask; no Debug dumps; zeroize |
| Produce profile only Solo confuses operators | Banner + docs: “Produce requires Solo / local host jobs” |
| 409 OCC destroys local draft | Non-destructive conflict UX (§3.4.1) |
| Invalid produce start after dialog dismiss | Pre-flight resolve/validate/QC (§3.6.1) |

---

## 7. Definition of Done

Complete only when **all** hold:

- [ ] **DoD-1 — Connect:** Operator can Connect to a local matter-service with URL + password login from Desk; banner shows Connected identity.  
- [ ] **DoD-2 — Remote mutate:** At least one coding (or notes) mutate succeeds with session actor + `expected_version`; **409 retains draft** + conflict UI (§3.4.1).  
- [ ] **DoD-3 — Mode fail-closed:** Solo and Connected cannot dual-open; disconnect returns to Solo cleanly.  
- [ ] **DoD-4 — Produce UX (Solo):** Production profile dropdown + visible required `bates_start` + **pre-flight** before Start (**D-0060-02** closed).  
- [ ] **DoD-5 — Solo regression:** Local open → review → produce path still works without Connect.  
- [ ] **DoD-6 — SSO (soft):** Either (a) automatic loopback/custom-scheme handoff lands a session **without** clipboard bearer paste when platform host is configured, or (b) explicit residual **D-0064-*** / reaffirm **D-0059-02** with rationale (paste still not production DoD).  
- [ ] **DoD-7 — Networking:** Body loads cancel or latest-wins on navigate; no unbounded blocking queue (§3.7.1).  
- [ ] **DoD-8 — Tests:** §3.9 cases pass; clippy `-D warnings` on touched crates.  
- [ ] **DoD-9 — Docs:** Operator golden path + How-to + Desk/service READMEs updated; deferred ledger closed/advanced.  
- [ ] **DoD-10 — Recorded:** `review.md`; registry **Completed**; ledger committed (category `GUI` or `FEATURE`).

---

## 8. Verification commands (reference)

```powershell
cargo fmt --all --check
cargo clippy -p dedupe-desk -p matter-service --all-targets -- -D warnings
cargo test -p dedupe-desk
cargo test -p matter-service
# Full gate before commit:
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

---

## 9. Capability matrix (ship honesty)

| Feature | Solo Desk | Connected Desk (0064 P0) | CLI / service host |
|---|---|---|---|
| Open local matter / jobs | Yes | No (refuse) | Host owns matter |
| Review list / body | Local | HTTP | API |
| Codes / notes / privilege | Local | Codes P0; notes/priv residual/P1 | API |
| Locks / batches / QC samples | Local limited | Residual / P1 | API |
| Produce + production profile | **Yes (0064)** | No (use host CLI or Solo later) | CLI produce |
| Password login | n/a | Yes | `POST /v1/login` |
| OIDC SSO | n/a | Soft §3.5 | Service-mediated |

---

## 10. Handoff

Unblocks operator multi-user Desk path for pilots. Residual remote parity and remote produce stay future tracks. Series J closes when this track Completes (0062/0063 already done).
