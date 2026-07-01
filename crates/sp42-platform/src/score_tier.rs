//! Shared score-tier policy for patrol severity presentation.

use serde::{Deserialize, Serialize};

pub const HIGH_SCORE_THRESHOLD: i32 = 70;
pub const MEDIUM_SCORE_THRESHOLD: i32 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScoreTier {
    Low,
    Medium,
    High,
}

#[must_use]
pub const fn score_tier(score: i32) -> ScoreTier {
    if score >= HIGH_SCORE_THRESHOLD {
        ScoreTier::High
    } else if score >= MEDIUM_SCORE_THRESHOLD {
        ScoreTier::Medium
    } else {
        ScoreTier::Low
    }
}

#[cfg(test)]
mod tests {
    use super::{ScoreTier, score_tier};

    #[test]
    fn score_tier_maps_patrol_thresholds() {
        assert_eq!(score_tier(0), ScoreTier::Low);
        assert_eq!(score_tier(29), ScoreTier::Low);
        assert_eq!(score_tier(30), ScoreTier::Medium);
        assert_eq!(score_tier(69), ScoreTier::Medium);
        assert_eq!(score_tier(70), ScoreTier::High);
    }
}
