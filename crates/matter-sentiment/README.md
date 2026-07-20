# matter-sentiment

Offline **VADER-class sentiment / tone** for Dedupe Desk (track **0049**).

## What it does

- Opt-in job kind `sentiment` (never silent on extract).
- Method id: **`vader_lexicon_v1`** via [`vader-sentimental`](https://crates.io/crates/vader-sentimental) **0.1.3** (`default-features = false`).
- **Unit-extreme aggregation:** split into sentences (lines fallback), score each unit, primary `compound` = unit with max `|compound|`. Also stores min/max compound.
- Footer / confidentiality **disclaimer strip** before split (hygiene only — not privilege detection).
- Fingerprint: body `text_sha256` + method + **threshold snapshot**. Threshold-only change **relabels** polarity from stored compound without CAS re-read.
- **Unscored ≠ Neutral:** `sentiment_polarity IS NULL` means never scored / skipped / cleared.

## Honesty / limits

| Topic | Guidance |
|---|---|
| Heuristic | Lexicon + rules ≠ emotion ground truth or intent |
| Dilution | Whole-doc VADER washes hostile clauses toward 0; unit-extreme + strip mitigate, not eliminate |
| Sarcasm | Often mis-scored |
| Unscored | NULL polarity is **not** neutral — use `has_sentiment` / Unscored chip |
| Privilege / coding | **Never** auto-apply privilege, withhold, codes, or redaction from scores |
| Language | English lexicon P0 |
| Offline | No cloud, no transformers, no model download |

## Params (defaults)

```json
{
  "pos_threshold": 0.05,
  "neg_threshold": -0.05,
  "max_text_bytes": 200000,
  "max_units": 200,
  "reset": false,
  "batch_size": 100,
  "scope": "all"
}
```

Polarity: `positive` if compound ≥ `pos_threshold`; `negative` if ≤ `neg_threshold`; else `neutral`.

## License tree (runtime)

Pin: `vader-sentimental = "0.1.3"` with `default-features = false` (no `clap`).

Expected permissive runtime deps:

| Crate | License |
|---|---|
| vader-sentimental | MIT |
| hashbrown | MIT OR Apache-2.0 |
| lazy_static | MIT OR Apache-2.0 |
| regex (+ subcrates) | MIT OR Apache-2.0 |
| unicase | MIT OR Apache-2.0 |

Audit: `cargo tree -p matter-sentiment` and/or `cargo deny check`. Do not enable the `cli` feature of vader-sentimental (pulls `clap` only for the binary; still MIT, but unused).

## Not in scope

Transformers / ONNX / LLM tone (**0050/0051**), multilingual lexicons (**0054**), aspect-based sentiment, emotion taxonomies, heatmaps, auto-coding from polarity.
