# 0101 — Embedded Message Depth Flag — Plan

> Phased checklist mapped to `spec.md` §7. Planning-only Phase 0 is **closed**. Do not implement until the user says Implement.
>
> **Ledger (implement):** `ledgerful ledger start crates/pst-dedup-cli --category FEATURE --message "0101 unique-pst --max-embedded-depth"`
>
> **Fold-in (2026-08-27):** `opencode-review.md` + `agy-review.md` → spec §2.9 / `foldin-note.md`. Chain-of-9 CLI test removed; cancel-path threading and summary asserts added.

---

## Phase 0 — Spec expand → Ready (closed 2026-08-27)

- [x] Flag name `--max-embedded-depth`; default **3**; clap reject outside 1–8.
- [x] Same value → materialize + writer; identity hash depth stays 3.
- [x] GUI: field default 3, no slider.
- [x] Deferred §9; last-PR comments; 0103 minted for #90 SLBLOCK.
- [x] Status **Ready — not started**.
- [x] Fold-in: DoD-2 ceiling pair + writer chain-9@8; Serialize-only summary field; cancel ctx.

---

## Phase 1 — CLI + library wire → DoD-1, DoD-3 (summary field)

- [ ] Add `parse_max_embedded_depth_arg` next to other unique-pst parsers in `unique_pst_cmd.rs`: accept 1–8 inclusive; error string names the range.
- [ ] `UniquePstClapArgs`:
  ```rust
  /// Nested ATTACH_EMBEDDED_MSG extract/write depth (0094/0101).
  /// Default 3; valid 1–8. Deeper nests ledger ATTACH_DEPTH_LIMIT.
  #[arg(long = "max-embedded-depth", default_value_t = 3, value_parser = parse_max_embedded_depth_arg)]
  pub max_embedded_depth: u32,
  ```
- [ ] `UniquePstCliArgs.max_embedded_depth: u32` + `into_cli_args` copy.
- [ ] Replace `let nested_extract_depth = 3u32;` with `args.max_embedded_depth.clamp(1, 8)` (**single assignment** used for materialize **and** `WritePstOpts`).
- [ ] **Third consumer (do not add a second knob):** `NamedPropWritePlan::scan_messages_with_depth(..., write_opts_base.max_embedded_depth)` (~2418). It inherits the same field after the assignment above. Leave it that way.
- [ ] `ExportSection.max_embedded_depth: u32` — plain always-serialized field. **No** `serde(default)` (type is Serialize-only; default would be a no-op). **No** `skip_serializing_if`. Set from the effective clamped value.
- [ ] Cancel path: add `max_embedded_depth: u32` to `CancelledSummaryCtx`; pass the **effective** clamp at all three sites (~1440, ~1621, ~1699); `write_cancelled_summary_json` copies it onto `ExportSection` (do **not** hardcode 3; existing `include_bcc_recipients: false` on cancel is a separate 0082 quirk — do not copy that pattern for depth).
- [ ] Module test `ExportSection` literals in `unique_export_report.rs` (~2240 / ~2352 structs): add the field.
- [ ] Human summary: print `max_embedded_depth` near attach counts (~3333).
- [ ] GUI `unique_wizard.rs` `UniquePstCliArgs { ... max_embedded_depth: 3, ... }` (after `promote_on_attach_fail` ~420).
- [ ] Other `UniquePstCliArgs {` literals (re-verify at execute):
  - `crates/pst-dedup-cli/src/unique_pst_cmd.rs` (~4 unit-test structs)
  - `crates/pst-dedup-cli/tests/export_exit_0078.rs` (2)
  - `crates/pst-dedup-cli/tests/digest_probe_unify_0091.rs` (1)
- [ ] Do **not** change `MAX_EMBEDDED_IDENTITY_DEPTH`, `eml_pack::DEFAULT_MAX_EMBEDDED_DEPTH`, or writer `Default` (already 3).

---

## Phase 2 — Tests → DoD-2, DoD-3 asserts

New integration binary **`unique_pst_depth.rs`** (name locked in spec §8 unless execute finds a collision). Spawn `pst-dedup` **without** the `unique_pst.rs` helper that injects `--no-attachments`. Reuse the method-5 chain builder from `writer_fidelity.rs` `embedded_depth_cap_enforced`. `pst-writer` is already a dev-dependency of `pst-dedup-cli`.

- [ ] **Depth-4 pair:** write source with `max_embedded_depth: 8` and a chain of **4** method-5 embeds.
  - unique-pst default (no flag): `ATTACH_DEPTH_LIMIT`; 4th nest absent; `summary.json` `export.max_embedded_depth == 3`.
  - unique-pst `--max-embedded-depth 4`: no depth-limit for that chain; 4th nest present; `export.max_embedded_depth == 4`.
- [ ] **Ceiling pair (buildable):** write source with `max_embedded_depth: 8` and a chain of **8** embeds.
  - `--max-embedded-depth 7` → `ATTACH_DEPTH_LIMIT`.
  - `--max-embedded-depth 8` → clean for that chain.
- [ ] **Do not** attempt a CLI chain-of-9. Writer halt at `depth >= max_depth` means a writer-built PST holds at most 8 nested levels; unique-pst @ 8 over it is clean.
- [ ] **Writer unit** (extends `embedded_depth_cap_enforced`): in-memory chain of **9** @ `max_embedded_depth: 8` → `embedded_depth_limit_hits > 0` and `embedded_messages_written <= 8`. This is the in-CI proof the halt fires at the product ceiling.
- [ ] Clap: `--max-embedded-depth 0` and `9` (and a non-integer) → usage error, non-zero exit.
- [ ] Library clamp: `run_unique_pst_with_options` with `UniquePstCliArgs.max_embedded_depth` **0** and **9** on a tiny synthetic PST → `export.max_embedded_depth` is **1** and **8** (clap bypassed).
- [ ] Keep `cargo test -p pst-writer embedded_depth` green; keep `embedded_msg_hash_0090` green (hash depth 3 vs writer 8).
- [ ] Human stderr line is wired in Phase 1; asserting it in the same depth-4 run is optional if stdout/stderr is already captured.

---

## Phase 3 — Docs → DoD-3

- [ ] `docs/unique-pst-export.md` flag table (near `--include-bcc-recipients`): `--max-embedded-depth` default 3, valid 1–8, `ATTACH_DEPTH_LIMIT` when exceeded; 32 MiB budget still maps to the same code. One sentence: `unique_export_report_v1` gained always-present `export.max_embedded_depth` (consumers should ignore unknown keys; schema id **not** bumped).
- [ ] Update the **Nested unique-pst export (0094)** paragraph: CLI owns the knob; hardcoded 3 is gone. Short glossary: export knob vs `MAX_EMBEDDED_IDENTITY_DEPTH` vs `DEFAULT_MAX_EMBEDDED_DEPTH`.
- [ ] `docs/unique-pst-ediscovery-runbook.md`: if `export_attachments.csv` / histogram shows `ATTACH_DEPTH_LIMIT`, re-run with `--max-embedded-depth 8`. Ceiling is 8; remaining rows stay disclosed; do not treat leftover 64 as a parser bug. Optional INC* HITL path `output/inc0102784-post-0101/` (local only).
- [ ] `docs/pst-writer-fidelity-v1.md` depth-cap row: mention unique-pst flag (writer clamp unchanged).
- [ ] `CHANGELOG.md` one-liner for the new summary key (same “no schema bump” message).
- [ ] `docs/deferred.md` on implement complete: D-0067 notes CLI shipped; D-0094-inc-resmoke from HITL.

---

## Phase 4 — Finalize → DoD-4

- [ ] `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; targeted tests; workspace tests before publish.
- [ ] `review.md`: commands, fixture assumptions (4-deep / 8-deep writer sources; no chain-of-9 CLI), HITL run/skip, any INC* residual (depth vs 32 MiB).
- [ ] Registry **Completed**; ledger commit on the implement tx.
- [ ] Do **not** mark D-0067 closed. Do **not** implement 0102/0103.

---

## Crate / file map (plan-time)

| Crate | Files |
|---|---|
| `pst-dedup-cli` | `unique_pst_cmd.rs` (clap, clamp assignment, cancel ctx, named-prop inherits), `unique_export_report.rs`, **new** `tests/unique_pst_depth.rs`, `export_exit_0078.rs`, `digest_probe_unify_0091.rs` |
| `pst-dedup-gui` | `unique_wizard.rs` (compile default only) |
| `pst-writer` | `tests/writer_fidelity.rs` — **test-only** chain-9@8 halt (no production behavior change) |
| docs | `unique-pst-export.md`, `unique-pst-ediscovery-runbook.md`, `pst-writer-fidelity-v1.md`, `CHANGELOG.md`, `deferred.md` |
| `pst-reader` | **No change** |

---

## Handoff notes

- Phase 0 locks are closed. Do not raise the default. Do not unbounded-recurse.
- Do not invent a 9-deep CLI fixture. Writer ceiling is the product ceiling.
- INC* exit 64 after this ships is still correct if remaining nests are > chosen depth or over 32 MiB.
- Owner HITL is optional; do not block CI on Desktop PSTs.
- Single-exe / no-daemon unchanged.
- Rollback: revert the clap field; writer/reader behavior is pre-existing.
