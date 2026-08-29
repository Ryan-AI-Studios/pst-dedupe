# Track Completion Audit — 0106-UniqueEmlNestedMime

## Verdict: PASS

## Scope Reviewed

- Track dir: `C:\dev\Dedupe\conductor\0106-UniqueEmlNestedMime` (`spec.md`, `plan.md`, `review.md`, `review.codex.md`, fold-in artifacts).
- Execution repo: `C:\dev\Dedupe` on branch `track/0106-unique-eml-nested-mime` @ `fde5758` vs `origin/main` (source audit of product crates + docs + deferred/registry).
- Prior Codex FAIL findings (P1 incomplete nested attach list; P2 `source_msg_nid` → `unwrap_or(0)` stream parent) re-checked as **fixed**.
- Product DoD-1..3 and §10.1 locks; **D-0067-embedded-depth** openness; DoD-4 recording note (registry may remain In Progress — not sole FAIL per gate instructions).

READ-ONLY review. No files or Git state were modified except this audit artifact.

## Requirement and DoD Matrix

| Requirement | Status | Evidence | Tests | Gap |
|---|---|---|---|---|
| DoD-1 Method-5 + DTO → reconstructed RFC 5322 (`message/rfc822`, 8bit, always `Subject:`) | **Met** | `prepare_one_attach` gates on `attach_method == ATTACH_EMBEDDED_MSG`; Nested body via `nested_to_canonical` + recursive `write_canonical_eml_to` (`eml_pack.rs` ~986–1020, ~1133–1168); `write_headers_mode` always emits `Subject:` (~718–720); nested omits parent Source/Folder X-headers (~738–742) | `embedded_nested_dto_writes_rfc822_headers`; child-file base64 test | None |
| DoD-1 Method-5 skip/DTO/depth **before** `open_attach_body` | **Met** | Depth / DTO / honesty branch returns before stream open (~986–1022); `open_attach_body` only on non–method-5 path (~1025) | `method5_without_dto_skips_no_fake_rfc822`; `parent_depth_halt_at_max_1` | None |
| DoD-1 Honesty skip → `ATTACH_EMBEDDED_UNPARSED` via dedicated variant | **Met** | `EmlWriteError::AttachSkipped`; `map_eml_attach_fail_reason` matches variant (~911) | `method5_without_dto_skips_no_fake_rfc822` | None |
| DoD-1 Parent `depth >= max` / extract limit → `ATTACH_DEPTH_LIMIT`; exactly `max` nests | **Met** | `depth >= max_depth` before reconstruct (~988–989); recursive `write_depth: depth + 1` | Unit max=1; CLI `unique_eml_depth` 4@3/4@4 and 8@7/8@8 | None |
| DoD-1 Method-1 mime rfc822 dump unchanged | **Met** | Non–method-5 path still dumps bytes as 8bit rfc822 (~1025–1044) | `method1_mime_rfc822_still_dumps` | None |
| DoD-1 Write loop calls `materialize_nested_for_winner` with same effective depth as `EmlWriteOpts` | **Met** | `unique_eml_cmd.rs` `nested_depth` once (~270–276); extract before write (~385–387); opts use same clamp | CLI depth module | None |
| DoD-1 Nested child streams use `source_msg_nid`; missing nid soft-fails children | **Met** | Missing nid clears children + `pending_child_fails` (~994–1008); `open_attach_body` rejects `parent.locus.nid == 0` (~1093–1098) **before** in-memory bypass | `nested_missing_source_msg_nid_soft_fails_children` | None (Codex P2 fixed) |
| DoD-1 Nested incomplete attach list honesty (no silent drop / no invented part) | **Met** | `attachments_incomplete` carried on `AttachBody::Nested`; emit `ATTACH_META_FAILED`, no MIME part (~1142–1157) | `nested_attachments_incomplete_emits_meta_failed_no_invented_part` | None (Codex P1 fixed) |
| DoD-1 Nested ledger events use inner subject; `parents_only` omits parts; no prod `unwrap`/`expect` | **Met** | Events built with inner parent (~999–1005, ~1156); `want_attaches` gated on `KeepAttachmentsWithParent` (~624–625); `unwrap`/`expect` confined to `#[cfg(test)]` | `nested_soft_fail_none_subject_stays_empty`; `parents_only_omits_attach_parts` | None |
| DoD-2 CLI `--max-embedded-depth` default 3; reject outside 1–8 (`pub` parser) | **Met** | `pub fn parse_max_embedded_depth_arg` (`unique_pst_cmd.rs` ~573); UniqueEml clap in `main.rs` ~479–485; `UniqueEmlCliArgs.max_embedded_depth` wired | `clap_rejects_zero_nine_and_non_integer` | None |
| DoD-2 `summary.json` always has `max_embedded_depth` = effective clamp | **Met** | `UniqueEmlSummaryOut.max_embedded_depth: u32` always serialize (~124); set from `nested_depth` (~610); no `skip_serializing_if` / no `serde(default)` on that field | CLI depth tests assert summary value | None |
| DoD-2 Required unit/CLI tests (§10.2) | **Met by source** | Nested DTO, no-DTO skip, method-1 dump, parent-depth@1, soft-fail, parents_only, incomplete meta-failed, missing-nid; `unique_eml_depth.rs` present | Named tests in `eml_pack.rs` / `unique_eml_depth.rs` | Independent cargo run not observed this pass |
| DoD-3 Docs + CHANGELOG; schema ids not bumped | **Met** | `docs/unique-eml-import.md` flag + honesty row; `docs/unique-pst-export.md` unique-eml consumes DTO; CHANGELOG Unreleased 0106; `EML_PACK_SCHEMA` / `KEEP_SET_SCHEMA` unchanged | n/a | None |
| DoD-3 / §9 **D-0067-embedded-depth** stays **open** (narrowed only) | **Met** | `docs/deferred.md:705` Owner **open**; notes unique-eml MIME shipped / residual matter children | n/a | None |
| DoD-4 Recorded (`review.md`; registry Completed; ledger) | **Partial / non-blocking per gate** | Canonical `review.md` exists with DoD matrix + gates. Registry row still **In Progress** (`conductor.md:260`). Ledger not re-queried this pass. | n/a | Registry Completed publish step remains orchestrator; **do not FAIL solely for this** |
| §10.1 Locked fix (extract→reconstruct; skip without DTO; no MAPI dump) | **Met** | Matches implementation order and skip reasons | Honesty + reconstruct tests | None |
| Out of scope not invaded (unique-pst nested rewrite, identity depth, BCC-default, frontend, D-0067 close) | **Met** | Deferred row open; no UniqueEmlClapArgs; frontend 0107+; CHANGELOG/docs say not closed | n/a | None |

## Findings

None.

### Prior Codex findings — verification

#### [P1] Nested incomplete attachment lists → ATTACH_META_FAILED (no invented MIME part) — **FIXED**

Confidence: High

Location: `crates/dedup-engine/src/eml_pack.rs` (~857–858, ~1016, ~1142–1157, test ~1966–2010)

Evidence: `attachments_incomplete` is carried on `AttachBody::Nested`. On write, counters/`attachment_events` gain an inner-subject `ATTACH_META_FAILED` row with filename `(nested attach list incomplete)`. Test asserts reconstructed inner body present, **no** invented MIME token for the incomplete list, `attachments_failed == 1`, and the event reason/subject.

#### [P2] Missing `source_msg_nid` soft-fails children; `open_attach_body` rejects `nid == 0` — **FIXED**

Confidence: High

Location: `crates/dedup-engine/src/eml_pack.rs` (~992–1008, ~1049–1061, ~1093–1098, test ~2014–2078)

Evidence: When `source_msg_nid` is missing, children are soft-failed into `pending_child_fails` and cleared before nested write (including in-memory payloads). `open_attach_body` returns error if `parent.locus.nid == 0` **before** the in-memory data short-circuit. Test asserts no `SGVsbG8=` / no `child.txt`, `attachments_failed == 1`, reason `ATTACH_STREAM_OPEN_FAILED`, inner subject, and no `X-Pst-Dedupe-Nid: 0x0`. Residual `unwrap_or(0)` in `nested_to_canonical` is only a non-stream locus sentinel for optional X-Nid omission — not a usable stream parent.

## Completeness Sweep

Searched `eml_pack` nested path for TODO/FIXME/`todo!`/`unimplemented!`, leftover `let _ = (opts, depth)`, and method-5 MAPI dump. None found in production path.

- No new production `unwrap`/`expect` in the nested MIME path (test-only expects remain under `#[cfg(test)]`).
- Schema ids not bumped; D-0067 not closed; no invented rfc822 body on honesty/depth skips.
- Keepset comment updated: Nested DTO “Consumed by unique-eml nested MIME reconstruct (0106).”

## Wiring and Regression Review

End-to-end path:

`unique-eml clap (--max-embedded-depth)` → `UniqueEmlCliArgs` → `run_unique_eml` (`nested_depth` clamp) → promote finalize (no extract) → winner rematerialize → `materialize_nested_for_winner` → `write_canonical_eml` → method-5 prepare (depth/DTO/skip before stream) → nested RFC 5322 → attach ledger/summary (`max_embedded_depth`, histogram from events).

Regression locks observed in source:

- Method-1 `message/rfc822` dump still 8bit + `unparsed`.
- `parents_only` still skips attach preparation.
- Boundaries include nid + depth (`make_boundary(..., depth)`).
- Inner events preserve empty nested subject (no winner fallback) per unit test.
- Soft-fail / no-DTO paths emit no `message/rfc822` part.

## Verification Evidence

**Observed now (source audit):**

- Prior Codex P1/P2 fix sites and locked tests present and consistent with §10.1 / DoD-1.
- Docs, CHANGELOG, and `docs/deferred.md` D-0067 row match DoD-3 (narrowed, **open**).
- `review.md` present; conductor registry still **In Progress** for 0106.

**Reported by orchestrator / canonical `review.md` (not re-executed here):**

- `cargo fmt --all --check` pass
- clippy `dedup-engine` + `pst-dedup-cli` `-D warnings` pass
- `cargo test -p dedup-engine --lib eml_pack` (34)
- `cargo test -p pst-dedup-cli --test unique_eml` (13)
- `cargo test -p pst-dedup-cli --test unique_eml_depth` (4)
- workspace test pass (pre-commit)

**Not independently verifiable this fallback pass:**

- Live `cargo test` / clippy / `ledgerful verify` were not executed in this reviewer session (no shell gate run observed).
- Exact tip commit contents vs `fde5758` not re-diffed via `git show` in-session; conclusions rest on the current worktree sources matching the claimed fixes.

**Recommended before publish (orchestrator):** re-run §8 verification commands once, then flip registry to Completed if publish gate requires it.

## Deferred Candidates

- **D-0067-embedded-depth** — correctly remains **open** (matter/Relativity child-document extract; 32 MiB; hard cap 8). Not proposed for closure.
- No new P3 deferrals from this track.

## Completion Decision

Engineering DoD-1..3 and §10.1 product locks are met. Codex P1 and P2 are fixed with dedicated regression tests. **D-0067** stays open. Canonical `review.md` exists; registry **In Progress** alone is not a FAIL under the FINAL GATE instructions.

**Verdict: PASS**
