//! A UTC instant, in epoch milliseconds.
//!
//! The intake, lifecycle and gate contracts all talk about time, and ADR-0026
//! §4 is explicit that they resolve it against an injected clock rather than
//! reading a wall clock. This newtype exists so that discipline is visible in the
//! signature: a bare `i64` in a field called `created_at` could be seconds, and
//! nothing would say so.
//!
//! It is `#[serde(transparent)]` over `i64`, so it adds no wire format and does
//! not change any existing payload. It is deliberately *not* retrofitted onto
//! fields that already exist elsewhere in the workspace (`EditEvent.timestamp_ms`
//! and friends stay plain `i64`) — ADR-0030's principle applies here too: do
//! not rewrite shipped behaviour's representation to serve a new type.
//!
//! Nothing in `sp42-types` calls [`Clock`](crate::traits::Clock). The platform
//! evaluator takes the resolved `now_ms` as a parameter instead, so the core
//! stays clock-free (CONSTITUTION §1.4) and `sp42-platform` keeps no runtime.

use std::ops::{Add, Sub};

use serde::{Deserialize, Serialize};

/// Milliseconds in one day, for the calendar helpers below.
const MS_PER_DAY: i64 = 86_400_000;

/// A UTC instant in epoch milliseconds.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Timestamp(i64);

impl Timestamp {
    /// The Unix epoch, 1970-01-01T00:00:00Z.
    pub const UNIX_EPOCH: Self = Self(0);

    /// Wrap epoch milliseconds.
    #[must_use]
    pub const fn from_epoch_ms(ms: i64) -> Self {
        Self(ms)
    }

    /// The epoch-millisecond value.
    #[must_use]
    pub const fn epoch_ms(self) -> i64 {
        self.0
    }

    /// `now` plus `days`, saturating rather than wrapping.
    ///
    /// Pure, so a shell can pass `clock.now_ms()` in and the core stays
    /// clock-free.
    #[must_use]
    pub fn plus_days(self, days: i64) -> Self {
        Self(self.0.saturating_add(days.saturating_mul(MS_PER_DAY)))
    }

    /// Whole days elapsed between `earlier` and `self`; negative when `earlier`
    /// is in the future.
    #[must_use]
    pub fn days_since(self, earlier: Self) -> i64 {
        self.0.saturating_sub(earlier.0) / MS_PER_DAY
    }

    /// Whether `self` is strictly before `other`.
    #[must_use]
    pub fn is_before(self, other: Self) -> bool {
        self.0 < other.0
    }

    /// Whether `self` is strictly after `other`.
    #[must_use]
    pub fn is_after(self, other: Self) -> bool {
        self.0 > other.0
    }

    /// Civil `YYYY-MM-DD` (UTC) for this instant.
    ///
    /// Pure, via Howard Hinnant's days-from-civil algorithm. Deliberately a
    /// helper on the type rather than something each consumer reimplements.
    #[must_use]
    pub fn civil_date(self) -> String {
        let days = self.0.div_euclid(MS_PER_DAY);
        let (year, month, day) = civil_from_days(days);
        format!("{year:04}-{month:02}-{day:02}")
    }
}

/// Days since the Unix epoch to a civil `(year, month, day)`.
///
/// All three are `i64` rather than a narrower type so the conversion needs no
/// casts — a truncating cast here would be a silent wrong date.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    // Shift the epoch to 0000-03-01 so leap days land at the end of the cycle.
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

impl From<i64> for Timestamp {
    fn from(ms: i64) -> Self {
        Self(ms)
    }
}

impl From<Timestamp> for i64 {
    fn from(value: Timestamp) -> Self {
        value.0
    }
}

impl Add<i64> for Timestamp {
    type Output = Self;

    /// Saturating, so arithmetic on a far-future instant cannot wrap.
    fn add(self, ms: i64) -> Self {
        Self(self.0.saturating_add(ms))
    }
}

impl Sub<i64> for Timestamp {
    type Output = Self;

    fn sub(self, ms: i64) -> Self {
        Self(self.0.saturating_sub(ms))
    }
}

impl Sub<Timestamp> for Timestamp {
    type Output = i64;

    /// Signed millisecond difference; negative when `other` is later.
    fn sub(self, other: Self) -> i64 {
        self.0.saturating_sub(other.0)
    }
}

impl std::fmt::Display for Timestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.civil_date(), self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::Timestamp;

    #[test]
    fn round_trips_through_epoch_ms() {
        let stamp = Timestamp::from_epoch_ms(1_700_000_000_000);
        assert_eq!(stamp.epoch_ms(), 1_700_000_000_000);
        assert_eq!(i64::from(stamp), 1_700_000_000_000);
    }

    #[test]
    fn serializes_transparently_as_a_bare_integer() {
        // `transparent` is the point: no new wire format, so nothing that
        // already stores a millisecond timestamp changes shape.
        let json = serde_json::to_string(&Timestamp::from_epoch_ms(42)).expect("serializes");
        assert_eq!(json, "42");
        let back: Timestamp = serde_json::from_str("42").expect("deserializes");
        assert_eq!(back, Timestamp::from_epoch_ms(42));
    }

    #[test]
    fn ordering_and_comparison_are_total() {
        let early = Timestamp::from_epoch_ms(10);
        let late = Timestamp::from_epoch_ms(20);
        assert!(early < late);
        assert!(early.is_before(late));
        assert!(late.is_after(early));
        assert_eq!(late - early, 10);
        assert_eq!(early - late, -10);
    }

    #[test]
    fn arithmetic_saturates_instead_of_wrapping() {
        // A far-future instant plus an offset must not wrap to the past; a
        // wrapped timestamp would silently invert every age comparison.
        let far = Timestamp::from_epoch_ms(i64::MAX - 1);
        assert_eq!(far + 10_000, Timestamp::from_epoch_ms(i64::MAX));
        let ancient = Timestamp::from_epoch_ms(i64::MIN + 1);
        assert_eq!(ancient - 10_000, Timestamp::from_epoch_ms(i64::MIN));
    }

    #[test]
    fn plus_days_is_exact_for_whole_day_multiples() {
        let start = Timestamp::from_epoch_ms(0);
        assert_eq!(start.plus_days(20).epoch_ms(), 20 * 86_400_000);
        assert_eq!(start.plus_days(20).days_since(start), 20);
    }

    #[test]
    fn civil_date_matches_known_instants() {
        assert_eq!(Timestamp::UNIX_EPOCH.civil_date(), "1970-01-01");
        assert_eq!(
            Timestamp::from_epoch_ms(1_700_000_000_000).civil_date(),
            "2023-11-14"
        );
        // A leap day, which is the case a hand-rolled conversion gets wrong.
        assert_eq!(
            Timestamp::from_epoch_ms(1_709_164_800_000).civil_date(),
            "2024-02-29"
        );
    }

    #[test]
    fn civil_date_handles_instants_before_the_epoch() {
        // Truncating division would report 1969-12-31 as 1970-01-01.
        assert_eq!(Timestamp::from_epoch_ms(-1).civil_date(), "1969-12-31");
    }
}
