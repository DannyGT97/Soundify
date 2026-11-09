//! Reads yt-dlp's output lines to work out a download's overall progress
//! (combining the playlist's current item with the current item's %).
//!
//! **The item total can be locked from outside** ([`ProgressParser::lock_total_items`])
//! before starting a bounded run with `--playlist-items` — then it stops trusting
//! "... items of N" in yt-dlp's own output. The reason (real bug, reported: "it was
//! on download 12/64 and showed 2%"): with `--playlist-items` bounding the run to a
//! subset (64 pending items out of a 707-item playlist, say), yt-dlp still reports
//! "N" as the size of the WHOLE playlist on that line, not that of the subset
//! actually being processed — using that value inflates the denominator brutally
//! and sinks the real % (11/707 = about 2%, when the real progress was 11/64 =
//! about 17%). The app ALREADY knows in advance how many items a bounded run will
//! process, so that is the only reliable source there. For an UNBOUNDED run (a
//! whole playlist, or the prior query failed), what yt-dlp reports is still
//! trusted, since in that case it is correct.

#[derive(Debug, Default)]
pub struct ProgressParser {
    current_item: u32,
    total_items: u32,
    total_items_locked: bool,
    /// Items already done before this run: they make the percentage that of the
    /// whole playlist and not that of the remaining chunk (see [`Self::lock_total_items`]).
    already_done_items: u32,
    /// Items yt-dlp went through and gave up on (see [`Self::set_skipped`]).
    skipped_items: u32,
}

impl ProgressParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Clears the state before every new download (a different playlist, or starting over).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Forces the real item total of THIS bounded run, also counting what was
    /// already done before it started. From here on, any "... items of N" yt-dlp
    /// reports is ignored. Do not call it for an unbounded run: there the total
    /// yt-dlp reports is already the right one.
    ///
    /// Why `already_done` (reported): with `--playlist-items` the run only brings
    /// what is missing, so progress measured over it always starts at 0. If you
    /// stop a 700-item playlist at file 400 and sync again, the new run is 300 and
    /// the bar went back to 0% — it looked as if all the progress had been lost.
    /// Counting the earlier 400, it picks up at 57%. They count as done because
    /// they are not going to take any time.
    pub fn lock_total_items(&mut self, already_done: i64, items_in_this_run: i64) {
        self.already_done_items = clamp_count(already_done);
        self.total_items = self.already_done_items + clamp_count(items_in_this_run);
        self.total_items_locked = true;
        self.current_item = 0;
    }

    /// How many of the items it has already gone through FAILED. yt-dlp's
    /// "Downloading item N of M" counts the items it tried, not the ones that
    /// came down, so with most of them refused the bar said 98 % over a
    /// playlist of which a third was on disk (reported). A failed item does not
    /// advance the bar.
    pub fn set_skipped(&mut self, skipped: usize) {
        self.skipped_items = u32::try_from(skipped).unwrap_or(u32::MAX);
    }

    /// The overall progress (0..=1) this line moves the download to, if it moves it.
    pub fn parse(&mut self, line: &str) -> Option<f64> {
        let mut emitted = None;
        // As in Java, a line that cannot be read stops being looked at right
        // there: nothing after the failing part is taken from it.
        let _ = self.read(line, &mut emitted);
        emitted
    }

    fn read(&mut self, line: &str, emitted: &mut Option<f64>) -> Option<()> {
        if !self.total_items_locked && line.contains("items of") {
            let parts: Vec<&str> = line.split(' ').collect();
            if let Some(i) = parts.iter().position(|p| *p == "of") {
                self.total_items = parts.get(i + 1)?.parse().ok()?;
            }
        }

        if line.contains("Downloading item") {
            self.current_item = line.split(' ').nth(3)?.parse().ok()?;
            if self.total_items > 0 {
                *emitted = Some(self.fraction(self.current_item, 0.0));
            }
        }

        if line.starts_with("[download]") && let Some(percent_index) = line.find('%') {
            let start = line[..percent_index]
                .rfind(|c: char| !(c.is_ascii_digit() || c == '.'))
                .map_or(0, |i| i + 1);
            let raw = line[start..percent_index].trim();
            if !raw.is_empty() && self.total_items > 0 {
                let item_percent = raw.parse::<f64>().ok()? / 100.0;
                // The current item can still be 0 if this % line arrives before
                // the first "Downloading item": treating it as the first item
                // avoids a negative base.
                *emitted = Some(self.fraction(self.current_item.max(1), item_percent));
            }
        }
        Some(())
    }

    fn fraction(&self, item: u32, item_percent: f64) -> f64 {
        let total = f64::from(self.total_items);
        let done = f64::from(self.already_done_items) + f64::from(item) - 1.0 - f64::from(self.skipped_items);
        (done.max(0.0) / total + item_percent / total).clamp(0.0, 1.0)
    }
}

fn clamp_count(n: i64) -> u32 {
    u32::try_from(n.max(0)).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The sample lines are copied from the real format yt-dlp emits, not made up.

    fn close(actual: Option<f64>, expected: f64) {
        let actual = actual.expect("no progress was emitted");
        assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
    }

    // === AN UNBOUNDED RUN: the total is yt-dlp's word ===

    #[test]
    fn takes_the_total_from_the_playlist_line() {
        let mut p = ProgressParser::new();
        p.parse("[youtube:tab] Playlist Canciones: Downloading 10 items of 10");
        // On starting item 3 there are 2 complete -> 20 %.
        close(p.parse("[download] Downloading item 3 of 10"), 0.2);
    }

    #[test]
    fn combines_the_current_item_with_that_item_percentage() {
        let mut p = ProgressParser::new();
        p.parse("[youtube:tab] Playlist Canciones: Downloading 10 items of 10");
        p.parse("[download] Downloading item 3 of 10");
        // 2 complete items + half an item = 0.20 + 0.05
        close(p.parse("[download]  50.0% of    3.00MiB at    1.00MiB/s ETA 00:01"), 0.25);
    }

    #[test]
    fn the_first_item_starts_at_zero() {
        let mut p = ProgressParser::new();
        p.parse("[youtube:tab] Playlist Canciones: Downloading 4 items of 4");
        close(p.parse("[download] Downloading item 1 of 4"), 0.0);
    }

    #[test]
    fn the_last_item_at_one_hundred_percent_reaches_one() {
        let mut p = ProgressParser::new();
        p.parse("[youtube:tab] Playlist Canciones: Downloading 4 items of 4");
        p.parse("[download] Downloading item 4 of 4");
        close(p.parse("[download] 100% of    3.00MiB in 00:03"), 1.0);
    }

    // === A BOUNDED RUN: the total is set by the app ===

    #[test]
    fn regression_with_playlist_items_yt_dlp_own_total_is_ignored_the_12_of_64_shown_as_2_percent_bug() {
        // yt-dlp keeps reporting the size of the WHOLE playlist (707) even though
        // the run has been bounded to 64 items: using it sank the percentage.
        let mut p = ProgressParser::new();
        p.lock_total_items(0, 64);
        p.parse("[youtube:tab] Playlist Canciones: Downloading 64 items of 707");
        let last = p.parse("[download] Downloading item 12 of 707");
        close(last, 11.0 / 64.0);
        assert!(last.unwrap() > 0.15, "progress went back to being computed over the whole playlist");
    }

    #[test]
    fn regression_resuming_a_half_done_playlist_does_not_send_the_bar_back_to_0() {
        // A 700-item playlist stopped at file 400: the new run is 300, and
        // measured over it alone the bar started at 0% even though more than
        // half the playlist was already there (reported).
        let mut p = ProgressParser::new();
        p.lock_total_items(400, 300);
        let last = p.parse("[download] Downloading item 1 of 300");
        close(last, 400.0 / 700.0);
        assert!(last.unwrap() > 0.5, "it should pick up above 50%, not at 0");
    }

    #[test]
    fn the_bar_reaches_one_hundred_when_the_run_ends() {
        let mut p = ProgressParser::new();
        p.lock_total_items(400, 300);
        p.parse("[download] Downloading item 300 of 300");
        close(p.parse("[download] 100% of 3.00MiB"), 1.0);
    }

    #[test]
    fn with_nothing_done_before_it_behaves_exactly_as_it_always_did() {
        let mut p = ProgressParser::new();
        p.lock_total_items(0, 10);
        close(p.parse("[download] Downloading item 3 of 10"), 0.2);
    }

    #[test]
    fn a_negative_offset_does_not_break_the_calculation() {
        let mut p = ProgressParser::new();
        p.lock_total_items(-5, 10);
        close(p.parse("[download] Downloading item 3 of 10"), 0.2);
    }

    #[test]
    fn reset_hands_the_total_back_to_yt_dlp() {
        let mut p = ProgressParser::new();
        p.lock_total_items(0, 64);
        p.reset();
        p.parse("[youtube:tab] Playlist Canciones: Downloading 10 items of 10");
        close(p.parse("[download] Downloading item 3 of 10"), 0.2);
    }

    // === ROBUSTNESS ===

    #[test]
    fn with_no_known_total_it_emits_nothing() {
        // An individual download has no playlist line: the overall progress
        // cannot be worked out, and must not be invented.
        let mut p = ProgressParser::new();
        assert_eq!(p.parse("[download] Downloading item 1 of 1"), None);
        assert_eq!(p.parse("[download]  50.0% of 3.00MiB"), None);
    }

    #[test]
    fn a_percentage_arriving_before_the_first_downloading_item_gives_no_negative_progress() {
        let mut p = ProgressParser::new();
        p.lock_total_items(0, 4);
        close(p.parse("[download]  50.0% of 3.00MiB"), 0.125);
    }

    #[test]
    fn non_progress_lines_are_ignored_without_breaking() {
        let mut p = ProgressParser::new();
        p.parse("[youtube:tab] Playlist Canciones: Downloading 4 items of 4");
        for line in ["", "[ExtractAudio] Destination: 001 - Canción.mp3", "ERROR: [youtube] abc: Video unavailable",
                     "[download] Downloading item de la muerte", "[download] %"] {
            assert_eq!(p.parse(line), None, "{line}");
        }
    }

    #[test]
    fn progress_never_leaves_the_zero_to_one_range() {
        let mut p = ProgressParser::new();
        p.lock_total_items(0, 2);
        let values = [
            p.parse("[download] Downloading item 9 of 2"), // deliberately inconsistent
            p.parse("[download] 100% of 3.00MiB"),
        ];
        for value in values.into_iter().flatten() {
            assert!((0.0..=1.0).contains(&value), "out of range: {value}");
        }
    }

    #[test]
    fn items_that_failed_do_not_advance_the_bar() {
        let mut p = ProgressParser::new();
        p.lock_total_items(0, 10);
        close(p.parse("[download] Downloading item 6 of 10"), 0.5);
        // Five tried, two of them refused: only three are on disk.
        p.set_skipped(2);
        close(p.parse("[download] Downloading item 6 of 10"), 0.3);
    }

    #[test]
    fn the_reported_run_does_not_read_98_percent() {
        let mut p = ProgressParser::new();
        p.lock_total_items(122, 489);
        p.set_skipped(595 - 122);
        // 122 before + 595 tried - 473 refused = 244 on disk, not 98 %.
        close(p.parse("[download] Downloading item 596 of 611"), 244.0 / 611.0);
    }
}
