# 0102 — Export Oracle Inputs Attest — Plan

> Phased checklist mapped to `spec.md` §7. Planning-only Phase 0 is **closed**. Do not implement until the user says Implement.
>
> **Ledger (implement):** `ledgerful ledger start crates/pst-dedup-cli --category BUGFIX --message "0102 export_oracle keep export_risk.inputs"`
>
> **Fold-in (2026-08-28):** `opencode-review.md` + `agy-review.md` → spec §2.9 / `foldin-note.md`. Parent-oracle docs + inverse mismatch test; pointer-string asserts required.

---

## Phase 0 — Spec expand → Ready (closed 2026-08-28)

- [x] Two `"inputs"` keys: root path array vs `export_risk.inputs` product object.
- [x] Locked fix: remove `"inputs"` from `SUMMARY_ALLOWLIST_KEYS`; keep root blanking.
- [x] Pointers stay; compare stays on normalized tree.
- [x] Deferred §9; last-PR comments (#93–#90; origin #89 is this track). 0103 not stolen.
- [x] Status **Ready — not started**.
- [x] Fold-in: parent-oracle docs qualification; inverse pre-0099 mismatch test; required pointer-string asserts; name-based strip comment; drop absent `Dedupe-plan.md` chase.

---

## Phase 1 — Strip + pointers → DoD-1

File: `crates/pst-dedup-cli/src/export_oracle.rs` (re-verify line numbers at execute; plan-time `main` @ `11e455f`).

- [x] Remove `"inputs"` from `SUMMARY_ALLOWLIST_KEYS` (today between `"keep_set_json"` and the volume-digest comment).
- [x] Expand the allowlist doc-comment: product fields include **`export_risk` / `export_risk.inputs`** (not volatile). Recursive strip of the name `inputs` is **forbidden** — it deleted the 0099 attest object. Strip is **name-based**; new product fields must not reuse allowlist names (`path`, `bytes`, `out`, `inputs`, hashes, timings) or the oracle must go path-aware.
- [x] Update the module doc `# Parent vs HEAD / pre-0079 packs` (`export_oracle.rs` ~14–22): 0079 measurement equalization still holds; **pre-0099** packs that omit the four attest keys **must mismatch** HEAD (intended). `PST_DEDUPE_BASELINE_BIN` must be post-0099 for a green env-gated gate.
- [x] Keep `normalize_summary_for_oracle` root blanking:

  ```rust
  if let Some(obj) = v.as_object_mut() {
      // Job-level UniqueExportSummary.inputs (source paths). Not export_risk.inputs.
      obj.insert("inputs".into(), Value::Array(vec![]));
  }
  ```

  That block already exists (~427). Add/adjust the comment only; do not move it before strip (strip no longer touches `"inputs"`, so order vs strip is no longer load-bearing for the nested object — still run it **after** strip so other allowlisted keys are gone first).
- [x] Leave `compare_integrity_counters` pointer list unchanged (four `/export_risk/inputs/…` paths plus existing keep_set / export / scan / `export_risk/level` pointers).
- [x] Leave call order in `compare_export_packs` unchanged: normalize → whole-object → `compare_integrity_counters(&sa, &sb, …)`.
- [x] Do **not** edit `unique_export_report.rs`, `unique_pst_cmd.rs`, writer, reader, GUI, or `export_exit_0078.rs`.
- [x] Do **not** add path-aware `strip_keys_recursive` unless Phase 2 proves root blanking is insufficient (it should not).

---

## Phase 2 — Tests → DoD-2

Same module `export_oracle.rs` `#[cfg(test)] mod tests`. Call `normalize_summary_for_oracle` and private `compare_integrity_counters` (already in crate). No temp PST packs.

Shared fixture shape (trim as needed):

```rust
fn attest_summary(effective: f64, discounted: bool, root_paths: &[&str]) -> Value {
    json!({
        "ok": true,
        "inputs": root_paths,
        "export_risk": {
            "level": "ok",
            "reasons": ["poly_class_crc_discounted"],
            "inputs": {
                "effective_block_crc_read_rate": effective,
                "poly_class_crc_discounted": discounted,
                "discount_attach_stream_crc": true,
                "poly_class_crc_sources": 2
            }
        },
        "export": { "messages_written_total": 1, "attachments_failed": 0 },
        "keep_set": { "stats": { "degraded_winners": 0 } },
        "scan": { "block_crc_rate": 0.0, "block_crc_read_rate": 1.0 }
    })
}
```

- [x] `normalize_preserves_export_risk_inputs_attest` — after normalize, pointers
  `/export_risk/inputs/effective_block_crc_read_rate`,
  `…/poly_class_crc_discounted`,
  `…/discount_attach_stream_crc`,
  `…/poly_class_crc_sources`
  are `Some` and equal to the pre-normalize values. Root `/inputs` is `[]`.
- [x] `attest_effective_rate_mismatch` — two trees that differ **only** in `effective_block_crc_read_rate` (`0.0` vs `0.20`). Normalize both. `compare_integrity_counters` pushes a mismatch whose string **contains** `/export_risk/inputs/effective_block_crc_read_rate`. **Required** (not optional next to whole-object `sa != sb`).
- [x] `attest_poly_discount_mismatch` — differ **only** in `poly_class_crc_discounted`. Mismatch string **contains** `/export_risk/inputs/poly_class_crc_discounted`.
- [x] `root_inputs_paths_equalize` — differ **only** in root `inputs` arrays (`["C:/a.pst"]` vs `["D:/b.pst"]`). After normalize: `sa == sb` on those trees (or at least no integrity mismatch + both `/inputs` are `[]`). `export_risk.inputs` still present.
- [x] `identical_attest_matches` — same attest object → `compare_integrity_counters` adds **no** `/export_risk/inputs/` mismatch.
- [x] `pre0099_parent_attest_mismatches` — parent-shaped `export_risk.inputs` **omits** the four 0099 keys (0077 fields may remain: `attach_fail_rate`, `block_crc_read_rate`, …); HEAD-shaped includes them. After normalize, `compare_integrity_counters` mismatches on `/export_risk/inputs/effective_block_crc_read_rate` (and typically the other three). Do **not** allowlist the object to make this pass.
- [x] Keep `allowlist_equalizes_parent_without_0079_counters` green. If adding `export_risk` to that fixture, copy the **same** attest object onto parent and HEAD.
- [x] Keep `normalize_strips_timing_and_paths` green.

No `cargo test --test unique_pst` requirement. No INC*.

---

## Phase 3 — Docs → DoD-3

- [x] `docs/unique-pst-export.md` **Oracle allowlist** paragraph (~187): after listing 0079 measurement fields, add: job-level `summary.inputs` (source paths) is blanked at **root only**; do **not** recursive-strip the name `inputs` — that object key is also `export_risk.inputs` (0099 product attest). Oracle pointers `/export_risk/inputs/effective_block_crc_read_rate` (and siblings) must compare.
- [x] **Qualify** the same paragraph’s “pre-0079 parent still compares equal” claim: that is **measurement** equalization only. A **pre-0099** parent that omits the four attest keys **must mismatch** HEAD — intended, that is the attest. Operator `PST_DEDUPE_BASELINE_BIN` must be a **post-0099** binary for a green env-gated gate (otherwise expected-red on those pointers).
- [x] Doc-comment only on `unique_pst_parent_baseline_oracle_when_env_set` (`crates/pst-dedup-cli/tests/unique_pst.rs` ~1507): baseline must be post-0099; pre-0099 parents legitimately mismatch on `export_risk.inputs`. Do **not** change `assert_equivalent()` or the skip-when-unset behavior. Do **not** add unique-pst pack tests.
- [x] `CHANGELOG.md` one-liner under Unreleased: unique-pst export oracle no longer strips `export_risk.inputs` (0099 attest).
- [x] `docs/deferred.md`: mark `D-0099-oracle-inputs-attest` **closed / 0102** (on implement complete; this planning pass only notes the owner is Ready).

---

## Phase 4 — Finalize → DoD-4

- [x] `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test -p pst-dedup-cli --lib export_oracle` (filter re-verify); workspace tests before publish.
- [x] Write `review.md` in this track dir: results, evidence, no new deferred (row closed).
- [x] Update `../conductor.md`: this track **Completed**. Light `sequencing.md` / `ROADMAP.md`.
- [x] Commit the implement ledger transaction (`BUGFIX` on `crates/pst-dedup-cli`).
- [x] Notify: 0103 remains next Series P placeholder. No BCC track. Frontend stays **0105+**.

---

## Handoff notes

- Planning-only until Implement. Product crates unchanged in this pass.
- Single-exe / no-daemon constraint unchanged (CLI library module only).
- Rollback: revert the allowlist line + tests + docs. No on-disk summary schema change.
- Do not “fix” parent-oracle mismatches against pre-0099 binaries by allowlisting `export_risk.inputs`.
- Do not chase `C:\dev\Dedupe-plan.md` (absent).
- `compare_integrity_counters` is private; tests belong in the same file’s `mod tests`.
- Hotspot `crates/pst-dedup-cli/tests/export_exit_0078.rs` is **out of scope**.
