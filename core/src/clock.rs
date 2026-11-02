//! "Now", asked through a function so tests can fix the date.

use std::sync::Arc;

use chrono::{Local, NaiveDate, NaiveDateTime};

/// The archives' date format, the Java app's `yyyy-MM-dd HH:mm`.
pub const STAMP_FORMAT: &str = "%Y-%m-%d %H:%M";

pub type Clock = Arc<dyn Fn() -> NaiveDateTime + Send + Sync>;

pub fn system() -> Clock {
    Arc::new(|| Local::now().naive_local())
}

/// Always the same moment.
pub fn fixed(stamp: &str) -> Clock {
    let at = NaiveDateTime::parse_from_str(stamp, STAMP_FORMAT).expect("a valid stamp");
    Arc::new(move || at)
}

pub fn stamp(at: NaiveDateTime) -> String {
    at.format(STAMP_FORMAT).to_string()
}

/// The day of a stamp, or None if missing or unreadable.
pub fn parse_day(stamp: Option<&str>) -> Option<NaiveDate> {
    let stamp = stamp?.trim();
    NaiveDateTime::parse_from_str(stamp, STAMP_FORMAT).ok().map(|at| at.date())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_back_the_day_of_its_own_stamps() {
        let now = fixed("2026-11-02 23:59")();
        assert_eq!(stamp(now), "2026-11-02 23:59");
        assert_eq!(parse_day(Some(" 2026-11-02 23:59 ")), NaiveDate::from_ymd_opt(2026, 11, 2));
        assert_eq!(parse_day(Some("yesterday")), None);
        assert_eq!(parse_day(None), None);
    }
}
