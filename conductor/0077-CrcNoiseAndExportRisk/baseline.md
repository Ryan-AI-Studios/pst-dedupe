# 0077 Phase 0 — Pre-edit baseline (DoD-12)

Captured on branch `feat/0077-crc-noise-export-risk` at HEAD `79b5cdf` **before** code edits.

## Dependency pins (Cargo.lock)

| Crate | Version |
|---|---|
| `tracing` | **0.1.44** |
| `tracing-subscriber` | **0.3.23** (≥0.3.20 ANSI escaping) |

No pin change required by 0077.

## Fixture scan baselines

Command: `cargo run -p pst-dedup-cli --release --quiet -- scan <fixture> --json`

### `fixtures/aspose_outlook.pst` (3,318,784 bytes)

| Field | Value |
|---|---|
| total_messages | 17 |
| unique | 17 |
| duplicates | 0 |
| recoverable_messages | 17 |
| degraded_messages | 0 |
| duration_secs | ~0.017 |

Raw JSON: `baseline-aspose.json` / summary extract `baseline-aspose-summary.json`.

### `fixtures/promotions_spam.pst` (195,472 bytes)

| Field | Value |
|---|---|
| total_messages | 0 |
| unique | 0 |
| duplicates | 0 |
| recoverable_messages | 0 |
| degraded_messages | 0 |
| duration_secs | ~0.016 |

Raw JSON: `baseline-promotions.json` / summary extract `baseline-promotions-summary.json`.

Note: this fixture currently yields zero message rows under the scan walker (structure-only / non-mail store for this path). Still recorded for DoD-12 “clean fixture corpus” presence.

## DoD-12 contract

On clean sources (no CRC hit ⇒ no taint):

- messages written / unique counts / keep-set winners / `content_hash_hex` must remain byte-identical to this baseline
- On corrupt sources only: `CRC_SUSPECT` may refine groups (split-only); never merge; accounted by `crc_suspect_messages`
