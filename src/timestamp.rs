//! The one clock reading a run takes.
//!
//! Only the `// generated-at:` line of a generated file uses it, and that line
//! is deliberately excluded when a file on disk is compared against what this
//! run would write - see [`crate::generator::same_but_for_timestamp`]. A
//! timestamp is for a human reading a file, never for deciding whether to
//! rewrite it.

use std::time::{SystemTime, UNIX_EPOCH};

/// The current UTC time, as `YYYY-MM-DDThh:mm:ssZ`.
pub fn now_utc() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    format_utc(seconds)
}

/// Formats a Unix timestamp as `YYYY-MM-DDThh:mm:ssZ`.
pub fn format_utc(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let time = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);

    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        time / 3600,
        (time % 3600) / 60,
        time % 60,
    )
}

/// Days since 1970-01-01 → `(year, month, day)`, by Howard Hinnant's
/// `civil_from_days`. Leap years included, no table, no dependency.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // Shift the era so that the leap-day lands at the end of a 400-year cycle.
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;

    (year + i64::from(month <= 2), month, day)
}

#[cfg(test)]
mod tests {
    use super::format_utc;

    #[test]
    fn formats_known_instants() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(1_000_000_000), "2001-09-09T01:46:40Z");
        // A leap day, and the last second before one.
        assert_eq!(format_utc(1_709_164_800), "2024-02-29T00:00:00Z");
        assert_eq!(format_utc(1_709_164_799), "2024-02-28T23:59:59Z");
        assert_eq!(format_utc(1_800_000_000), "2027-01-15T08:00:00Z");
    }
}
