//! Unit-extreme aggregation and polarity mapping.

use matter_core::sentiment_polarity;

/// Scores for one unit (or the extreme-winning unit).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitScore {
    pub compound: f64,
    pub pos: f64,
    pub neu: f64,
    pub neg: f64,
}

/// Aggregated document-level sentiment from unit scores.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AggregatedSentiment {
    /// Primary compound = unit with max |compound|.
    pub compound: f64,
    pub compound_min: f64,
    pub compound_max: f64,
    /// pos/neu/neg from the winning (extreme) unit.
    pub pos: f64,
    pub neu: f64,
    pub neg: f64,
    pub polarity: &'static str,
}

/// Map compound to polarity using thresholds.
///
/// - `positive` if compound ≥ `pos_threshold`
/// - `negative` if compound ≤ `neg_threshold`
/// - else `neutral`
pub fn polarity_from_compound(
    compound: f64,
    pos_threshold: f64,
    neg_threshold: f64,
) -> &'static str {
    if compound >= pos_threshold {
        sentiment_polarity::POSITIVE
    } else if compound <= neg_threshold {
        sentiment_polarity::NEGATIVE
    } else {
        sentiment_polarity::NEUTRAL
    }
}

/// Aggregate unit scores with **unit-extreme** primary compound.
///
/// Empty input returns `None` (caller leaves item unscored).
pub fn aggregate_units(
    units: &[UnitScore],
    pos_threshold: f64,
    neg_threshold: f64,
) -> Option<AggregatedSentiment> {
    if units.is_empty() {
        return None;
    }
    let mut compound_min = units[0].compound;
    let mut compound_max = units[0].compound;
    let mut win_idx = 0usize;
    let mut win_abs = units[0].compound.abs();
    for (i, u) in units.iter().enumerate().skip(1) {
        if u.compound < compound_min {
            compound_min = u.compound;
        }
        if u.compound > compound_max {
            compound_max = u.compound;
        }
        let a = u.compound.abs();
        // Prefer later unit only if strictly greater |compound| (determinism).
        if a > win_abs {
            win_abs = a;
            win_idx = i;
        }
    }
    let win = units[win_idx];
    let polarity = polarity_from_compound(win.compound, pos_threshold, neg_threshold);
    Some(AggregatedSentiment {
        compound: win.compound,
        compound_min,
        compound_max,
        pos: win.pos,
        neu: win.neu,
        neg: win.neg,
        polarity,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extreme_picks_hostile_over_neutral() {
        let units = [
            UnitScore {
                compound: -0.8,
                pos: 0.0,
                neu: 0.2,
                neg: 0.8,
            },
            UnitScore {
                compound: 0.01,
                pos: 0.1,
                neu: 0.8,
                neg: 0.1,
            },
        ];
        let agg = aggregate_units(&units, 0.05, -0.05).unwrap();
        assert!((agg.compound - (-0.8)).abs() < 1e-9);
        assert_eq!(agg.polarity, "negative");
        assert!((agg.compound_min - (-0.8)).abs() < 1e-9);
        assert!((agg.compound_max - 0.01).abs() < 1e-9);
    }

    #[test]
    fn threshold_relabel_path() {
        assert_eq!(
            polarity_from_compound(0.10, 0.05, -0.05),
            sentiment_polarity::POSITIVE
        );
        assert_eq!(
            polarity_from_compound(0.10, 0.20, -0.05),
            sentiment_polarity::NEUTRAL
        );
    }
}
