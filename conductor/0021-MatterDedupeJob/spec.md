# 0021 — Tiered dedupe as matter job

- **Track ID:** 0021-MatterDedupeJob
- **Execution repo:** `C:\dev\dedupe`
- **Governance:** this directory in `C:\dev\dedupe\conductor\`
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → Series B / track 0021
- **Cross-repo contract:** n/a
- **Status:** Ready — not started

---

## 1. Objective

Integrate/extend dedup-engine as a matter job using **Message-ID + logical_hash + family-aware policies** (not raw-blob-only identity).

## 2. Context (read before starting)

- Product plan: `C:\dev\Dedupe-plan.md` (architecture, phases, security, UX).
- Guardrails: `../TRACK-GUARDRAILS.md`.
- Comparison context (optional): `C:\dev\Comparison.md`.
- Existing crates to reuse where possible: `pst-reader`, `dedup-engine`, `pst-dedup-cli`, `pst-dedup-gui`.
- **Desktop rule:** single-process / single-exe launch; no user-managed servers, Redis, Postgres, or Docker for Desk edition.
- **AI rule:** AI remains optional and off by default unless a track explicitly delivers a plugin that is still opt-in.

## 3. In scope

1. Wire `dedup-engine` tiers into matter items (Tier-1 Message-ID, Tier-2 content/logical).
2. Prefer **`logical_hash` / Message-ID** over `native_sha256` for suppress-duplicate decisions.
3. Keep `native_sha256` for custody and “same file bit-for-bit” queries.
4. Family-aware policy hooks (e.g. suppress body dups but retain unique attachments).
5. Fixture: same email as PST-extracted item vs EML item → one unique under logical policy.
6. Audit dedupe run parameters and counts.

## 4. Out of scope (do NOT do here)

- Work owned by other tracks in Series A–I (see `../sequencing.md`).
- Multi-tenant SaaS, SSO, or horizontal workers (Series I) unless this *is* a Series I track.
- Shipping always-on AI or cloud egress by default.
- Destructive writes to source PST/Purview export files.
- Unrelated dependency major upgrades.

## 5. Preconditions & dependencies

- **P1 (blocking):** Dependencies: **0018,0019**
- **P2:** `C:\dev\Dedupe-plan.md` accepted as plan-of-record.
- **P3:** Workspace builds: `cargo check --workspace` green on `main` (or document blockers).
- *Verified to date:* Tracks 001–014 history live under legacy `track###-…` folders; new work uses `####-PascalName`.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Scope creep into full Nuix parity | Stick to Series B outcome; defer via Proposed tracks |
| Silent data loss on bad inputs | Honest errors; partial results labeled; item-level skip accounting |
| Breaks single-exe UX | No external daemon; child processes only if app-owned and optional |
| Weak audit trail | Append-only audit events with tool version + params |

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Capability:** The Series B outcome for this track is implemented and exercisable on Windows without manual server setup.
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
