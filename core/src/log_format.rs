//! The shape of the two log files' lines, as the Java app wrote them (same
//! `dd/MM/yyyy HH:mm:ss` date), so old and new logs read the same. Writing and
//! rotating the files is the logging setup's job; this is only the text.

use chrono::NaiveDateTime;

const DATE: &str = "%d/%m/%Y %H:%M:%S";

/// The system's own line separator, as Java's `%n` was.
#[cfg(windows)]
const NL: &str = "\r\n";
#[cfg(not(windows))]
const NL: &str = "\n";

/// One line of the activity log (yt-dlp's raw output): just the date and the
/// message. No source: every line comes from the same place, so it would be
/// repeated thousands of times per download without adding anything — it only
/// gets in the way of reading the real line.
pub fn activity_line(at: NaiveDateTime, message: &str) -> String {
    format!("{} {message}{NL}", at.format(DATE))
}

/// One entry of the app's diagnostic log: date and source on one line, then
/// `LEVEL: message` on the next.
pub fn app_entry(at: NaiveDateTime, source: &str, level: &str, message: &str) -> String {
    format!("{} {source}{NL}{level}: {message}{NL}", at.format(DATE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn known() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 3, 15).unwrap().and_hms_opt(14, 30, 45).unwrap()
    }

    #[test]
    fn activity_is_just_the_date_and_the_message_on_one_line() {
        assert_eq!(activity_line(known(), "hello"), format!("15/03/2026 14:30:45 hello{NL}"));
    }

    #[test]
    fn app_entry_is_date_and_source_then_level_and_message_on_two_lines() {
        assert_eq!(app_entry(known(), "Foo.bar", "WARNING", "oops"),
                   format!("15/03/2026 14:30:45 Foo.bar{NL}WARNING: oops{NL}"));
    }

    #[test]
    fn single_digit_days_and_hours_are_zero_padded() {
        let at = NaiveDate::from_ymd_opt(2026, 1, 2).unwrap().and_hms_opt(3, 4, 5).unwrap();
        assert!(activity_line(at, "x").starts_with("02/01/2026 03:04:05 "));
    }
}
