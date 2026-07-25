# 0064 — Desk platform Connect + operator UX — Plan

> **Ledger:** `ledgerful ledger start 0064-deskplatformconnectux --category GUI --message "Desk Connect + produce profile UX"`

**Status:** **Completed** — 2026-07-25; Codex luna PASS WITH DEFERRED P3 (D-0064-07/08).

## Locks (from spec)

1. **Native `dedupe-desk` only** — no WASM  
2. **Explicit Solo vs Connected** — never hybrid dual-open  
3. **Thin remote review P0** — list/body/codes + OCC; full parity residual  
4. **409 non-destructive** — retain draft; conflict UI; no silent wipe (§3.4.1)  
5. **Produce profile + `bates_start` = Solo only** + pre-flight resolve/validate/QC (§3.6.1)  
6. **OIDC is service-mediated** — Desk does not re-discover IdP; system browser  
7. **SSO: ban clipboard bearer paste** for production; loopback one-time handoff (§3.5)  
8. **Networking: async preferred + abort/latest-wins** — no unbounded blocking body queue (§3.7.1)  
9. **reqwest 0.12 workspace pin**; UI thread never blocks on HTTP  
10. **Close D-0058-01 / D-0060-02**; soft-advance D-0059-02  

## Phase 0 — Preconditions → DoD grounding

- [ ] Confirm `matter-service` routes: `/healthz`, `/v1/login`, items, codes, OIDC  
- [ ] Confirm Desk produce hardcodes `bates_start: 1` and omits `production_profile`  
- [ ] Confirm `Matter::list_production_profiles` + `validate_production_profile_body`  
- [ ] Read deferred: D-0058-01, D-0059-02, D-0060-02  
- [ ] Read review folds §3.11  
- [ ] `ledgerful ledger start 0064-deskplatformconnectux --category GUI --message "…"`  
- [ ] Optional: `ledgerful scan --impact` before large edits  

## Phase 1 — HTTP client + session model → DoD-1, DoD-3, DoD-7

- [ ] Add `reqwest` (workspace) to `dedupe-desk`; expand `tokio` features if using async  
- [ ] Implement `RemoteClient` / session types (base URL normalize, healthz, login, logout, authorized JSON)  
- [ ] **Abortable / generation-scoped** body fetch (cancel on navigate; latest-wins)  
- [ ] Timeouts + status mapping (401/403/409)  
- [ ] Zeroize password after login; avoid logging bearer  
- [ ] Unit tests: URL normalize, error mapping, stale body drop, no actor on mutate builders  

## Phase 2 — Connect UX + mode machine → DoD-1, DoD-3, DoD-5

- [ ] Connect dialog (URL, user, password; Connect / Cancel)  
- [ ] Disconnect control  
- [ ] Persistent Connected banner  
- [ ] Refuse local matter open while Connected; refuse Connect while matter open (or confirm-close)  
- [ ] Solo regression: local open path untouched when never Connected  

## Phase 3 — Thin remote review + OCC UX → DoD-2, DoD-7

- [ ] Branch Review list loader: Connected → `GET /v1/items`  
- [ ] Body loader with cancel/latest-wins + repaint  
- [ ] Apply codes → `POST …/codes` with `expected_version`  
- [ ] **409 path:** retain draft; fetch server snapshot; conflict panel; re-apply / discard opt-in  
- [ ] Disable mutates for `read_only`  
- [ ] Tests: mock or in-process service router; 409 draft retained  

## Phase 4 — Produce profile + Bates start + pre-flight (Solo) → DoD-4

- [ ] State: `selected_production_profile_id`, `produce_bates_start`  
- [ ] Hydrate profiles from open matter  
- [ ] Produce UI: dropdown + Bates start field (required ≥ 1)  
- [ ] Extend `produce_params` with `production_profile` + `bates_start`  
- [ ] **Pre-flight:** resolve profile, validate body, bates, QC readiness — fail closed before job start  
- [ ] Unit tests for params JSON + pre-flight block  
- [ ] Confirm Connected mode does not claim remote produce  
- [ ] Do **not** invent LibreOffice/Ghostscript checks  

## Phase 5 — SSO automatic handoff → DoD-6

- [ ] Detect `oidc_required` / platform host  
- [ ] “Sign in with SSO” → ephemeral Desk loopback + system browser to `/v1/oidc/login`  
- [ ] Minimal service post-auth handoff (loopback-only URL + one-time exchange code)  
- [ ] Redeem code → session; **no** clipboard bearer paste for production path  
- [ ] If deferred: record **D-0064-*** / reaffirm D-0059-02 (paste still not production DoD)  

## Phase 6 — Docs + deferred → DoD-9

- [ ] `docs/operator-golden-path.md` — Path C host+Connect; produce profile note  
- [ ] `conductor/How-to-use.md`, `features.md`  
- [ ] `crates/dedupe-desk/README.md`, `matter-service/README.md`  
- [ ] `docs/deferred.md` — close D-0058-01, D-0060-02; advance D-0059-02  

## Phase 7 — Verify + finalize → DoD-8, DoD-10

- [ ] `cargo test -p dedupe-desk`  
- [ ] `cargo test -p matter-service` (if handoff endpoints added)  
- [ ] `cargo clippy -p dedupe-desk -p matter-service --all-targets -- -D warnings`  
- [ ] Full gate before commit: fmt + clippy workspace + test workspace  
- [ ] Write `review.md` (evidence, residuals, capability matrix honesty, fold disposition)  
- [ ] Update `conductor/conductor.md`, `ROADMAP.md`, `sequencing.md` → **Completed**  
- [ ] `ledgerful ledger commit <tx-id> --summary "…" --reason "…"`  

## Suggested implementation order (minimal blast radius)

1. Produce profile + `bates_start` + pre-flight (pure Solo) — early win for D-0060-02  
2. Remote client (async/cancel) + Connect dialog + mode guards  
3. Remote list/body/codes + **409 conflict UX**  
4. SSO loopback handoff  
5. Docs + review  

## Handoff notes

- **Irreversible:** none (UX track); disconnect clears session.  
- **Do not** open local Matter while Connected.  
- **Do not** add service produce routes in this track unless explicitly expanded.  
- **Do not** implement Desk→IdP OIDC discovery (SSRF + redirect policy live on service).  
- **Do not** ship clipboard bearer paste as the operator SSO path.  
- **Do not** silent-refresh wipe drafts on 409.  
- Rollback: feature is opt-in Connect; Solo path must remain default.
