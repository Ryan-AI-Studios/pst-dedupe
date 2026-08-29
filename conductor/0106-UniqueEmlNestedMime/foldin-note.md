# Fold-in note — 0106 UniqueEmlNestedMime

**Date:** 2026-08-28  
**Sources:** `opencode-review.md` + `agy-review.md`  
**Status after fold:** **Ready — not started** (not Completed)

## Folded

| Id | Disposition |
|---|---|
| opencode-M1 | Skip/DTO/depth **before** `open_attach_body`; method-5 never opens the attach stream |
| opencode-M2 | `parse_max_embedded_depth_arg` is **`pub`** (bin `main.rs` cannot see `pub(crate)`) |
| opencode-M3 | Gate on `attach_method == ATTACH_EMBEDDED_MSG`; method-1 rfc822 dump test **mandatory** |
| opencode-m1 | Parent-depth halt; exactly `max` nests; 2-level @ max=1 unit **mandatory** |
| opencode-m2 | Dedicated skip variant carrying the 0073 code (not `Other` substring) |
| opencode-m3 | Nested events: inner subject + inner `attach_index` |
| opencode-O2 | Inner headers always emit `Subject:` |

## Declined

- `UniqueEmlClapArgs` (agy Phase 2) — UniqueEml clap stays in `main.rs`
- Counting `AttachStreamSource` stub as a required DoD test
- Closing `D-0067-embedded-depth`

Harness `*-review.md` files were **not** edited.
