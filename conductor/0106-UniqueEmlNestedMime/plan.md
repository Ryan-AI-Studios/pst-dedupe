# 0106 — Unique-EML Nested MIME — Plan

> Phased checklist mapped to `spec.md` §7. Planning-only Phase 0 is **closed**. Do not implement until the user says Implement.
>
> **Ledger (implement):** `ledgerful ledger start crates/dedup-engine --category FEATURE --message "0106 unique-eml nested MIME message/rfc822"`
>
> **Fold-in (2026-08-28):** `opencode-review.md` + `agy-review.md` → spec §2.9 / `foldin-note.md`. Lock: skip/DTO before `open_attach_body`; parser **`pub`**; gate on `attach_method`; parent-depth halt; dedicated skip variant; inner subject on events; always `Subject:`.
>
> Re-verify at execute: `prepare_one_attach` still ignores `embedded_message` and still calls `open_attach_body` at :951 before `if embedded`; unique-eml write loop still has no `materialize_nested_for_winner`; UniqueEml clap still has no depth flag (`main` @ plan-time `40d5a43`).

---

## Phase 0 — Spec expand → Ready (closed 2026-08-28)

- [x] Live unique-eml dumps method-5 attach bytes as `message/rfc822` + `unparsed` (`eml_pack.rs` `prepare_one_attach` ~953; `let _ = (opts, depth)`).
- [x] `NestedCanonicalMessage` + `materialize_nested_for_winner` exist (0094); unique-eml never calls extract.
- [x] Locked fix: extract on write loop; reconstruct RFC 5322 from DTO; skip method-5 without DTO (`ATTACH_EMBEDDED_UNPARSED`); depth halt `ATTACH_DEPTH_LIMIT`; `--max-embedded-depth` 1–8 default 3.
- [x] Deferred §9; last-PR comments (#101–#98 none). Frontend **0107+**.
- [x] Status **Ready — not started**.
- [x] RFC 2046 §5.2.1: rfc822 wrapper CTE 7bit/8bit/binary only (already 8bit).
- [x] Fold-in: M1–M3 / m1–m3 / O2 applied; `UniqueEmlClapArgs` declined.

---

## Phase 1 — eml_pack reconstruct + honesty skip → DoD-1

Files: `crates/dedup-engine/src/eml_pack.rs` (re-verify line numbers at execute; plan-time `main` @ `40d5a43`). `keepset.rs` comment on `NestedCanonicalMessage` (“unique-eml ignores this field this track”) → update to “consumed by unique-eml 0106”.

- [ ] Thread `opts.max_embedded_depth` (clamp 1–8 at write). `write_canonical_eml_to` starts at **parent depth 0** (top-level winner). Halt **before writing a method-5 child** when `parent_depth >= max` (unique-pst `write_one_attachment`: `if depth >= max_depth` with top-level `build_message_node(..., 0)`). Recursive nested write passes `depth + 1`. Exactly **`max`** nested rfc822 parts: default 3 writes nests 1–3; the 4th is `ATTACH_DEPTH_LIMIT`. Do **not** halt at “child depth == max on entry” (that would drop nest 3 / nest 1 at max=1).
- [ ] `prepare_one_attach` — **method-5 gate is `att.attach_method == Some(ATTACH_EMBEDDED_MSG)`**, not the `embedded` / `is_embedded_message` local (that helper is also true for method-1 mime rfc822 and would skip those dumps as `ATTACH_EMBEDDED_UNPARSED`).
- [ ] **Move/delete `open_attach_body` for method-5** (live `:951` runs before any branch). Order:
  1. If method-5 and (`att.embedded_extract_limit` **or** `depth >= opts.max_embedded_depth.clamp(1, 8)`) → skip via dedicated error variant carrying `"ATTACH_DEPTH_LIMIT"` (no rfc822 part, **no stream open**).
  2. Else if method-5 and `att.embedded_message` is `Some` → `PreparedPart { embedded: true, unparsed: false, body: NestedRfc822(...) }` — **do not** open/dump the method-5 attach stream.
  3. Else if method-5 → skip via dedicated variant `"ATTACH_EMBEDDED_UNPARSED"` (no stream dump, **no stream open**). **Must** change today’s dump path.
  4. Else (file parts and method-1 mime rfc822) → `open_attach_body` as today.
  The `prepare_attachments` `Err` arm (`:852–863`) maps these via `map_eml_attach_fail_reason`. Method-5 skip `Err` must be unreachable as `ATTACH_STREAM_OPEN_FAILED`.
- [ ] Method ≠ 5 and mime contains `message/rfc822` → keep Memory/Stream dump + `unparsed: true` (open stream **after** the method-5 branch).
- [ ] `write_prepared_part` for parsed nested: keep wrapper
  ```
  Content-Type: message/rfc822
  Content-Disposition: attachment; filename="…"
  Content-Transfer-Encoding: 8bit
  ```
  then blank line, then reconstructed inner RFC 5322. Never base64 the wrapper.
- [ ] Inner reconstruction (factor or convert — implementer choice, locks are behavioral):
  - Headers: Message-ID, **always `Subject:`** (empty allowed — RFC 2046 §5.2.1), From, To, Cc, Bcc-if-`display_bcc` non-empty, Date UTC `+0000`. Reuse `write_headers` Subject line or replicate it; do not omit Subject when `dto.subject` is `None`.
  - **No** `X-Pst-Dedupe-Source` / `X-Pst-Dedupe-Folder` on the inner message. Optional `X-Pst-Dedupe-Nid` from `source_msg_nid` only.
  - Body structure matches parent (plain / html / alternative / mixed with child attaches).
  - Child attaches: recurse `prepare_one_attach` / `write_prepared_part` with a **stream locus** whose `source_path` is the winner’s and `nid` is `nested.source_msg_nid` (not the top-level winner nid). Missing nid → child soft-fail.
  - Inner multipart boundary includes nested nid and/or depth so it cannot equal the outer `----=_PstDedupe_mixed_{winnerNid}_`.
- [ ] Skip transport: add a dedicated `EmlWriteError` variant (name implementer-choice, e.g. `AttachSkipped(&'static str)` / `Skipped { reason: &'static str }`) that **carries** `"ATTACH_DEPTH_LIMIT"` or `"ATTACH_EMBEDDED_UNPARSED"`. `map_eml_attach_fail_reason` matches that variant and returns the code. **Do not** encode these as `EmlWriteError::Other` substring probes (`"not found"` / `"null"` already live there). Update `Display` / `Error` / every match on `EmlWriteError` (clippy `-D warnings` catches non-exhaustive). Do not emit `ATTACH_PART_FAILED` as a CSV reason.
- [ ] Nested soft-fail events: `eml_attach_event_from_soft_fail` already uses `parent.subject` and the enumerated `attach_index`. Recursive nested prepare must pass the **inner** message as `parent` so `message_subject` is the inner subject and `attach_index` is the inner list index. `attach_nid` distinguishes collisions in the CSV.
- [ ] Roll up counters from nested writes into the parent `EmlWriteResult` (embedded count, file count, failed, events, `embedded_message_unparsed` OR of children).
- [ ] `parents_only` still skips `prepare_attachments` (no nested parts).
- [ ] Do **not** edit `pst-writer` nested write, `MAX_EMBEDDED_IDENTITY_DEPTH`, unique-pst clap, or GUI wizard.
- [ ] No `unwrap` / `expect` in production.

---

## Phase 2 — unique-eml CLI wire → DoD-1, DoD-2 (flag + summary)

- [ ] Make `parse_max_embedded_depth_arg` **`pub fn`** in `unique_pst_cmd.rs` (same 1–8 error string). unique-eml clap stays the `UniqueEml` variant in **bin** `main.rs` (there is no `UniqueEmlClapArgs` — do not mint one):
  ```rust
  /// Nested ATTACH_EMBEDDED_MSG extract/write depth (0106).
  /// Default 3; valid 1–8. Deeper nests ledger ATTACH_DEPTH_LIMIT.
  #[arg(long = "max-embedded-depth", default_value_t = 3, value_parser = pst_dedup_cli::unique_pst_cmd::parse_max_embedded_depth_arg)]
  max_embedded_depth: u32,
  ```
  **`pub(crate)` is a compile error** from `main.rs` (separate bin crate). A `lib.rs` `pub use` of a private item is E0364. Optional extra `pub use unique_pst_cmd::parse_max_embedded_depth_arg` on `lib.rs` only **after** the item is `pub`.
- [ ] `UniqueEmlCliArgs.max_embedded_depth: u32` + `main.rs` struct literal copy (~1312).
- [ ] `run_unique_eml`:
  - `let nested_depth = args.max_embedded_depth.clamp(1, 8);` **once**.
  - `EmlWriteOpts { family_policy: args.family_policy, max_embedded_depth: nested_depth }`.
  - In the winner write loop, after re-materialize + fidelity copy, **before** `write_canonical_eml`:
    ```rust
    if let Err(e) = materialize_nested_for_winner(&mut attach_src, &mut msg, nested_depth) {
        tracing::warn!("nested extract nid={:#x}: {e}", msg.locus.nid);
    }
    ```
    Import `materialize_nested_for_winner` from `pst_materializer` (unique-pst already does).
  - Do **not** call extract inside `finalize_with_materialize_opts`’s `|_| Ok(())`.
- [ ] `UniqueEmlSummaryOut.max_embedded_depth: u32` — always serialize. Set `nested_depth` on the payload (~582). **No** `serde(default)`. **No** required `EmlPackManifest` field.
- [ ] Human summary: print `max_embedded_depth: {nested_depth}` near attach counts. Re-verify the print site at execute (plan-time human block is the `=== Unique EML pack` `println!`s around `:700+`; `~711` was summary assembly, not the print).
- [ ] unique-eml has `cancel: None` on scan (no cancelled-summary ctx like unique-pst). If execute finds an early-summary path, thread effective depth there — do not hardcode 3.
- [ ] Do not add a Desk slider. unique-eml is CLI-first.

---

## Phase 3 — Tests → DoD-2

### Unit (`crates/dedup-engine/src/eml_pack.rs` `#[cfg(test)]`)

- [ ] `embedded_nested_dto_writes_rfc822_headers` (name flexible): method-5 + `NestedCanonicalMessage { subject: Some("Inner subject"), sender: Some("inner@ex.com"), body_plain: Some("inner body"), source_msg_nid: Some(0x200), .. }`. Assert rfc822 wrapper, inner Subject/From/body, 8bit not base64 on wrapper, `embedded_messages_written == 1`, `!embedded_message_unparsed`, **no** `X-Pst-Dedupe-Source:` in the rfc822 body. **Must fail on HEAD.**
- [ ] Nested child file attach `data: b"Hello"` → inner `SGVsbG8=`; wrapper still 8bit.
- [ ] Replace `embedded_message_rfc822_not_octet_stream`: method-5 + `data: Some(b"From: x\r\n\r\nbody")` + `embedded_message: None` → **no** `message/rfc822`; `attachments_failed == 1`; event reason `ATTACH_EMBEDDED_UNPARSED`. **Must fail on HEAD** (today dumps rfc822 + `unparsed`).
- [ ] Keep `embedded_soft_fail_skips_no_fake_rfc822_body` green.
- [ ] **Mandatory** method-1 rfc822 dump: `attach_method: Some(1)`, `mime: Some("message/rfc822")`, `data: Some(b"From: x\r\n\r\nbody")`, `embedded_message: None` → wrapper + those bytes, 8bit, `unparsed: true`, **zero** `ATTACH_EMBEDDED_UNPARSED` events. Live tree has no such test; without it M3 gating can regress silently.
- [ ] **Mandatory** parent-depth halt: two-level method-5 DTO (`nested.embedded_message` has another method-5 child) @ `max_embedded_depth: 1` → nest 1 inner subject present; nest 2 absent; event `ATTACH_DEPTH_LIMIT`. (Optional counting `AttachStreamSource` stub that method-5 skip never calls `open_attach` is nice-to-have, not a substitute.)

### CLI (`crates/pst-dedup-cli/tests/unique_eml_depth.rs`)

Copy the `method5_chain` / `write_source` helpers from `unique_pst_depth.rs` (or share a test helper if cheap). Spawn `pst-dedup unique-eml --out …`. Parse `{out}/summary.json` and `{out}/export_attachments.csv`.

- [ ] Depth-4 source @ default: `ATTACH_DEPTH_LIMIT`; 4th nest subject absent from the `.eml`; `max_embedded_depth == 3`.
- [ ] Same source `--max-embedded-depth 4`: no depth-limit for that chain; depth-4 subject present; `max_embedded_depth == 4`.
- [ ] Ceiling pair: 8-deep @ 7 → limit; @ 8 → clean.
- [ ] Clap: `0`, `9`, non-integer → usage error, non-zero exit.
- [ ] Existing `cargo test -p pst-dedup-cli --test unique_eml` stays green (`parents_only`, ledger header, source immutability, schema counts).
- [ ] Keep `canonical_attachment_serde_skips_embedded_message` and `parent_hash_unchanged_when_embedded_message_populated` green.

No client PST. No INC*.

---

## Phase 4 — Docs → DoD-3

- [ ] `docs/unique-eml-import.md`:
  - Flag table: `--max-embedded-depth` default 3, valid 1–8; deeper nests ledger `ATTACH_DEPTH_LIMIT`.
  - Honesty table **Embedded messages** row: when nested extract succeeds, the `message/rfc822` body is reconstructed RFC 5322 (not a MAPI dump). Extract/depth failures skip the part and ledger `ATTACH_EMBEDDED_UNPARSED` / `ATTACH_DEPTH_LIMIT`. By-value attached `.eml` still dumped as rfc822. Deep matter/Relativity child documents remain residual (`D-0067`).
- [ ] `docs/unique-pst-export.md` nested unique-pst paragraph (~495): strike “unique-eml still ignores nested DTOs”. unique-eml 0106 consumes `NestedCanonicalMessage` with the same extract helper and the same `--max-embedded-depth` semantics. Glossary row for `EmlWriteOpts::max_embedded_depth`.
- [ ] `CHANGELOG.md` Unreleased: unique-eml writes nested RFC 5322 for method-5 extracts; no longer labels raw MAPI as rfc822; `--max-embedded-depth` on unique-eml. Narrows **D-0067-embedded-depth** (does not close).
- [ ] `docs/deferred.md`: `D-0067-embedded-depth` notes unique-eml MIME **shipped / 0106**; **Do not close**; residual matter children + 32 MiB + cap 8.
- [ ] One sentence: `eml_pack_v1` / `keep_set_v1` ids **not** bumped; unique-eml `summary.json` gained always-present `max_embedded_depth`.

---

## Phase 5 — Finalize → DoD-4

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] Targeted tests (Phase 3) then `cargo test --workspace` before publish
- [ ] Write `review.md` in this track dir: results, evidence, D-0067 still open (matter children)
- [ ] Update `../conductor.md`: this track **Completed**
- [ ] Commit the ledger transaction in the execution repo
- [ ] Notify: frontend Series O if started uses **0107+**; D-0067 still not closed

---

## Handoff notes

- Planning-only until Implement. Do not edit product crates in this pass.
- Outward-facing: unique-eml method-5 MIME shape **changes** (dump → reconstruct or skip). That is the point. Operators who already imported MAPI-labeled rfc822 parts should re-export.
- Rollback: revert `eml_pack.rs` + unique-eml CLI; unique-pst nested write is untouched.
- Single-exe / no-daemon constraint unchanged.
- `conductor/` is gitignored; `git add -f` track files when the owner commits (not this planning skill unless asked).
- Do not steal 0100–0104 for frontend. Do not mint a BCC-default track. Do not mint `UniqueEmlClapArgs`.
- Re-verify `parse_max_embedded_depth_arg` is **`pub`** (not `pub(crate)`) and UniqueEml clap site at execute.
