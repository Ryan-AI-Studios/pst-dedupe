# 0098 — Template NID / Folder Contents Collision — Review

**Status:** implementation complete (2026-08-26). Ledger tx `fcdc105c-6185-4ea2-8af0-6968e7ec5930` pending git commit.

## Outcome

unique-pst verify counted **folder contents TCs**, not NBT `NormalMessage` nodes. Folder NID
`0x602` (nidIndex `0x30`) uses contents/hierarchy/assoc NIDs `0x60E`/`0x60D`/`0x60F` — the same
fixed MS-PST template objects. NBT last-wins left the empty templates; 50 Purges messages stayed
parented to `0x602` but disappeared from `folders()`.

**Fix:** `Layout::alloc_nid` skips nidIndex `0x30` / `0x33` / `0x34`. Duplicate NBT NIDs fail closed
at insert and at `write_nbt`.

## Evidence

| Check | Result |
|---|---|
| `cargo test -p pst-writer --lib nid_alloc_tests` | pass |
| `preserve_paths_many_folders_does_not_clobber_contents_template_nid` | pass |
| `cargo test -p pst-writer` | 36 lib + 1 + 52 + 18 + 35 = pass |
| `cargo clippy -p pst-writer -p pst-reader --all-targets -- -D warnings` | pass |
| `cargo fmt --all --check` | pass |

INC* re-smoke is operator-local (not CI). CRC `not_export_ready` unchanged.

## Residuals

- CRC / AMap integrity (pre-existing).
- `D-0093-attachment-tc-page`, recipient TC Strategy A.
- Frontend Series O, if started, uses **0105+**.
