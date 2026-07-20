//! Score one unit via vader-sentimental.

use vader_sentimental::SentimentIntensityAnalyzer;

use crate::aggregate::UnitScore;

thread_local! {
    /// Reused analyzer (construction is cheap but avoid per-unit churn).
    static ANALYZER: SentimentIntensityAnalyzer<'static> = SentimentIntensityAnalyzer::new();
}

/// Score a single text unit with the default VADER lexicon.
pub fn score_unit(text: &str) -> UnitScore {
    ANALYZER.with(|analyzer| score_unit_with(analyzer, text))
}

/// Score a single text unit with a shared analyzer instance.
pub fn score_unit_with(analyzer: &SentimentIntensityAnalyzer<'_>, text: &str) -> UnitScore {
    let s = analyzer.polarity_scores(text);
    UnitScore {
        compound: s.compound,
        pos: s.pos,
        neu: s.neu,
        neg: s.neg,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_positive_scores_positive() {
        let s = score_unit("This is wonderful amazing excellent fantastic news!!!");
        assert!(s.compound > 0.05, "compound={}", s.compound);
    }

    #[test]
    fn clear_negative_scores_negative() {
        let s = score_unit("This is terrible awful horrible disgusting hate!!!");
        assert!(s.compound < -0.05, "compound={}", s.compound);
    }
}
