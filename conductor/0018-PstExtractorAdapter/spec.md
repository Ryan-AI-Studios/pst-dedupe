# 0018 — PST extractor adapter (wrap pst-reader)

- **Track ID:** 0018-PstExtractorAdapter
- **Execution repo:** `C:\dev\dedupe`
- **Governance:** this directory in `C:\dev\dedupe\conductor\`
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → Series A / track 0018
- **Cross-repo contract:** n/a
- **Status:** Ready — not started

---

## 1. Objective

Map pst-reader folders/messages/attachments into Normalized Items for Purview PSTs, running extraction on a **blocking/CPU pool** (not the UI/async executor).

## 2. Context (read before starting)

- Product plan: `C:\dev\Dedupe-plan.md` (architecture, phases, security, UX).
- Guardrails: `../TRACK-GUARDRAILS.md`.
- Comparison context (optional): `C:\dev\Comparison.md`.
- Existing crates to reuse where possible: `pst-reader`, `dedup-engine`, `pst-dedup-cli`, `pst-dedup-gui`.
- **Desktop rule:** single-process / single-exe launch; no user-managed servers, Redis, Postgres, or Docker for Desk edition.
- **AI rule:** AI remains optional and off by default unless a track explicitly delivers a plugin that is still opt-in.

## 3. In scope

1. Adapter from `pst-reader` → Normalized Items + attachment families.
2. Populate `native_sha256` / `logical_hash` / `message_id` fields.
3. Run PST parse/hash work via **`spawn_blocking` / `rayon`** only (see plan §4.6).
4. Support **cursor/checkpoint** handoff to job runner (folder or every N messages).
5. Item-level errors without aborting entire PST when possible.
6. Fixture tests on sample Unicode PST.

## 4. Out of scope (do NOT do here)

- Work owned by other tracks in Series A–I (see `../sequencing.md`).
- Multi-tenant SaaS, SSO, or horizontal workers (Series I) unless this *is* a Series I track.
- Shipping always-on AI or cloud egress by default.
- Destructive writes to source PST/Purview export files.
- Unrelated dependency major upgrades.

## 5. Preconditions & dependencies

- **P1 (blocking):** Dependencies: **0016,0017**
- **P2:** `C:\dev\Dedupe-plan.md` accepted as plan-of-record.
- **P3:** Workspace builds: `cargo check --workspace` green on `main` (or document blockers).
- *Verified to date:* Tracks 001–014 history live under legacy `track###-…` folders; new work uses `####-PascalName`.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Scope creep into full Nuix parity | Stick to Series A outcome; defer via Proposed tracks |
| Silent data loss on bad inputs | Honest errors; partial results labeled; item-level skip accounting |
| Breaks single-exe UX | No external daemon; child processes only if app-owned and optional |
| Weak audit trail | Append-only audit events with tool version + params |

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Capability:** The Series A outcome for this track is implemented and exercisable on Windows without manual server setup.
- [ ] **DoD-2 — Tests:** Automated tests and/or documented fixture smoke prove the happy path + at least one failure path (corrupt/missing input or cancel).
- [ ] **DoD-3 — Workspace gate:** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and relevant `cargo test` targets pass (or failures are documented with justification).
- [ ] **DoD-4 — Audit / defensibility:** Any matter mutation writes audit events (or track explicitly documents why not applicable).
- [ ] **DoD-5 — Docs:** README or ARCHITECTURE note updated if user-facing surface/crates changed.
- [ ] **DoD-6 — Recorded:** `review.md` written; `../conductor.md` status → **Completed**; ledger transaction committed (category appropriate).

## 8. Verification commands (reference)

``powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
# Add track-specific tests, e.g.:
# cargo test -p matter-core
# cargo run -p pst-dedup-cli --release -- <cmd>
``
