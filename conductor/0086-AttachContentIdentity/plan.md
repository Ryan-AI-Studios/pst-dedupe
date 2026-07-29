# 0086 — Attach-Content Strong Identity — Plan

> Structure follows `C:\dev\coordinated\conductor\templates\0000-Description\plan.md`.
> Phased checklist; each phase maps to DoD items in `spec.md` §7. Execute in `C:\dev\Dedupe`.
> Mark items `- [x]` as completed.
>
> **Review fold-in (2026-07-29):** Choice **B** locked (domain-separated name+size unread
> sentinel; no tier downgrade); NIST multi-block KAT mandatory; Relativity “8 KiB” citation
> trimmed; empty vs length-mismatch clarified; ignore-inline **warns** not rejects — see
> `spec.md` §2.12.
>
> **Ledger:** open a transaction before starting —
> `ledgerful ledger start 0086-AttachContentIdentity --category FEATURE --message "<intent>"`
> — and commit it in the final phase.

---

## Phase 0 — Precondition / design lock → DoD-9 (partial)

- [x] Confirm board: 0082–0085 **Completed**; **D-0076-attach-content open** in `docs/deferred.md`.
- [x] Re-read Relativity Deduplication considerations (AttachmentHash = “normal standard SHA256 file hash”; **no** block-size claim).
- [x] Grep live reject path: `grouping_cli::parse_identity_level`, help strings in `main.rs` + `unique_pst_cmd.rs`.
- [x] Confirm hasher attach branch; plan replace of `filter_map` omit with **real|sentinel per slot**.
- [x] Confirm `pst_reader::open_attachment_data` + sticky-open / LRU patterns.
- [x] **LOCKED incomplete policy = Choice B** (spec §2.6): domain-separated sentinel  
  `SHA-256("pst-dedup/attach-unread/v1\0" || name_lower || "\0" || size_le_u32)` (exact bytes freeze in code + unit test). **Choice A declined.**
- [x] **Lock empty vs length-mismatch:** size 0 + empty EOF → real empty digest; size > 0 + empty/short stream → unread sentinel.
- [x] **Lock budgets** for full-stream digests (not L2 1 MiB head). Dedicated `--strong-hash-attach-*` flags.
- [x] **Lock embedded-msg policy:** P0 raw stream SHA-256; open **D-0086-embedded-email-hash**.
- [x] Confirm cloud-link path → unread sentinel (0084).
- [x] Confirm **no new major deps**; sha2 KEEP dual; note past RUSTSEC-2021-0100.
- [x] `ledgerful ledger status --compact`; FEATURE ledger tx already open by orchestrator.
- [x] Re-read `spec.md` §2.5–§2.6 + §2.12 — Choice B, NIST KAT, no invent, no multi-GB Vec, default off, Mode A docs, soft ignore-inline warning.

## Phase 1 — CLI accept + hasher / sentinel / NIST → DoD-1, DoD-4, DoD-4b

- [x] Update `parse_identity_level` to accept `body-recip-attach`; help text on all surfaces.
- [x] Invert `rejects_body_recip_attach` → `accepts_body_recip_attach`.
- [x] Implement Choice B slot construction in strong-hash path (no omit; no tier downgrade).
- [x] Hasher unit tests: digest split/merge; order independence; unread-PDF ≠ unread-XLSX ≠ empty ≠ content; **no-attach hijack guard**.
- [x] Empty vs length-mismatch unit tests.
- [x] **NIST multi-block KAT** on the same `sha2` Digest path used for attach bytes (exact expected digests).
- [x] Combined-flag: warn when ignore-inline + body-recip-attach (unit or CLI capture).
- [x] `cargo test -p dedup-engine` + `cargo test -p pst-dedup-cli grouping_cli` green.

## Phase 2 — Digest I/O helper → DoD-2, DoD-4

- [x] Implement stream-hash helper (CLI-side): open attach → chunked `Read` → `Sha256` → real digest; enforce length-match when size authoritative.
- [x] Wire into `scan.rs` **only when** `identity.includes_attach_content()`.
- [x] Default path (`off`/`body`/`body-recip`): **zero** new attach stream opens for identity.
- [x] Failures → Choice B sentinel + `strong_hash_attach_unread` (not `None` omit).
- [x] Budgets + cancel; dedicated flags.
- [x] Stats: unread + digested count/bytes.
- [x] Targeted CLI tests without multi-GB fixtures.

## Phase 3 — Grouping integration fixture → DoD-3, DoD-5, DoD-6

- [x] Synthetic PST (writer-built): same meta name:size, different attach bytes → two winners at `body-recip-attach`, one group at `body-recip`.
- [x] Cloud-link attach: unread sentinel + stat; no panic.
- [x] Refinement assertion includes `BodyRecipAttach` (hasher unit + integration).
- [x] Default `off` golden / prior tests still pass (no new I/O).
- [x] unique-pst / keep-set inherit via shared scan path.

## Phase 4 — Docs + deferred → DoD-7

- [x] `docs/unique-pst-export.md`: identity table row for `body-recip-attach`; cost warning; vs Relativity AttachmentHash (honest mapping; **no false 8 KiB attribution**); Choice B; empty vs length-mismatch.
- [x] `docs/unique-pst-ediscovery-runbook.md`: when to enable; budgets; cloud unread; Mode A cannot cross attach-content splits; ignore-inline softens byte promise.
- [x] `docs/deferred.md`: **close D-0076-attach-content**; open D-0086-* residuals.
- [x] CHANGELOG `[Unreleased]`.
- [x] unique-eml inherits level via shared parser / ScanOptions defaults.

## Phase 5 — Full verification + finalize → DoD-8, DoD-9

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`
- [x] `cargo deny check` (no lock touched; KEEP)
- [x] `ledgerful verify` (fmt+clippy+test ran; full suite green via direct cargo)
- [ ] Write `review.md` — orchestrator / final phase
- [ ] Update `../conductor.md` + `ROADMAP.md`: 0086 → **Completed** — orchestrator
- [ ] Commit ledger FEATURE transaction — orchestrator (not this worker)

---

## Handoff notes

- **Head probe ≠ content identity.** Never treat 0074 L2 success as attach-byte equality.
- **Choice B only** — never downgrade incomplete items to `body-recip` binding.
- **Opt-in only.** Default must remain free of full-stream attach I/O.
- **Cloud has no offline bytes** — unread sentinel is correct.
- **Mode A promotes within groups;** attach-content can change who shares a group.
- **NIST KAT is not optional** — silent wrong digests are evidence spoliation.
- Production forbids `.unwrap()` / `.expect()`.
- Prefer streaming SHA-256 over any `read_to_end`.
