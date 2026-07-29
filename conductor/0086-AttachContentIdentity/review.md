# Track 0086 — AttachContentIdentity — Review

**Status:** Completed (engineering + process)  
**Date:** 2026-07-29  
**Final cross-model gate:** Codex `gpt-5.6-luna` high → **PASS WITH DEFERRED P3** (`review.codex.final.md`)

## Summary

Shipped Tier-2.5 **`--strong-content-hash body-recip-attach`**: full-stream per-attachment SHA-256 digests into the strong identity preimage, with **Choice B** domain-separated unread sentinels (no omit, no tier downgrade). Default identity remains `off`. Closes **D-0076-attach-content**.

## Sentinel formula (frozen)

```
SHA-256( b"pst-dedup/attach-unread/v1\0" || name_lower_utf8 || b"\0" || size_le_u32 )
```

- Legitimate size-0 empty EOF → real `SHA-256("")` (`e3b0c442…`)
- Size > 0 + length mismatch → unread sentinel
- Cloud-link / open-fail / CRC / budget / cancel → unread sentinel

## Fail-closed (Codex FAIL rounds)

| Finding | Fix |
|---|---|
| BestEffort `list_attachments` Err → empty slots | `classify_attach_enum_for_identity` always Skip when attach-content needed |
| Soft-skipped corrupt attach PC rows → partial Ok | `list_attachments_strict` + scan identity path |
| `has_attachments=true` + empty list | Skip under attach-content |
| `--no-attachments` + `body-recip-attach` | Hard reject via `reject_no_attachments_with_attach_content` |

## Surfaces

- CLI live: scan, dups, keep-set, unique-eml, unique-pst
- Budgets: `--strong-hash-attach-max-attaches` / `-max-bytes` / `-per-attach-max-bytes` (defaults 50k / 1 GiB / 512 MiB)
- Soft warn (not reject) with `--identity-ignore-inline-attachments`
- Desk checkbox remains `body` only (**D-0076-gui**)

## Tests

- Hasher: Choice B multi-name, empty vs mismatch, hijack guard, NIST multi-block KAT, refinement
- CLI: parse accept; no-attachments reject; ignore-inline warning text
- Integration `attach_content_0086`: same name:size different bytes split only at attach level; cloud unread

## Gates

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --workspace` | pass |
| Internal subagent r1 → P3 fix → r2 | PASS WITH DEFERRED P3 |
| Codex r1 FAIL → fix → r2 FAIL → fix → r3 FAIL → fix → **final PASS WITH DEFERRED P3** | clean final gate |

## Deferred residuals

| ID | Item |
|---|---|
| **D-0086-embedded-email-hash** | Recursive Relativity-style hash for embedded-message attaches (P0 = raw stream) |
| **D-0086-digest-probe-unify** | Unify 0074 Full probe + identity digest into one pass |
| Process | Board Completed + FEATURE ledger commit on merge path |

## Review fold-in (spec §2.12)

Choice B locked; NIST KAT required; soft ignore-inline warn (not hard reject); no false Relativity “8 KiB” claim.

## Ledger

FEATURE `8c3a8b63-cfb6-4c22-8cfb-bbb34261d674` — committed at PR finalize.
