//! Turns the yt-dlp error of a video that cannot be downloaded into a short,
//! readable reason ("private video", "the author's account was closed"...), so
//! it can be shown right next to the status.
//!
//! The status used to be just "Unavailable" and the reason stayed in a tooltip:
//! you had to go looking for it file by file to understand why they were missing.
//!
//! The phrases looked for here are NOT made up: they come from really querying
//! the untitled ids of a real playlist (39 out of 721), and they are the ones
//! YouTube returns today. If YouTube changes the wording, this stops recognising
//! it and falls back to the generic reason — it never fails, it only loses detail.
//!
//! The archive stores yt-dlp's raw error, and this answers with an i18n KEY: the
//! text is the front's, so it follows the language in use.

/// The reason when it is known to be unavailable but not why.
pub const GENERIC: &str = "unavailable.generic";

/// Does this error mean the video is NOT coming back on its own?
///
/// This is the question deciding whether it gets marked unavailable, and being
/// marked is almost final: it takes the file out of the pending list and stops
/// it being attempted. So only the causes that really are permanent count.
///
/// Everything temporary is deliberately left OUT, and very particularly the
/// bot-check ("Sign in to confirm you're not a bot"): when YouTube throttles the
/// session, EVERY video fails with that error, and without this filter a single
/// sync with a blocked session would mark a whole playlist unavailable. Age
/// restriction and members-only content are also left out, as they depend on the
/// session and may work with other cookies.
pub fn is_permanent(yt_dlp_error: Option<&str>) -> bool {
    let Some(error) = yt_dlp_error.filter(|e| !e.trim().is_empty()) else { return false };
    let error = error.to_lowercase();

    // The bot-check beats anything else: it can come alongside phrases that do look final.
    if error.contains("not a bot") || error.contains("sign in to confirm you") {
        return false;
    }

    error.contains("private video")
        || error.contains("video unavailable")
        || error.contains("has been removed")
        || error.contains("account associated with this video has been terminated")
        || error.contains("account has been terminated")
        || error.contains("copyright")
}

/// The i18n key of the short reason. Never empty: if it is not recognised, [`GENERIC`].
pub fn reason_key(yt_dlp_error: Option<&str>) -> &'static str {
    let Some(error) = yt_dlp_error.filter(|e| !e.trim().is_empty()) else { return GENERIC };
    let error = error.to_lowercase();

    // The order matters: YouTube's messages overlap. "Video unavailable. This
    // video is no longer available because the YouTube account associated with
    // this video has been terminated" starts with the generic one, so the
    // specific must be looked at first.
    if error.contains("account associated with this video has been terminated")
        || error.contains("account has been terminated")
    {
        return "unavailable.accountTerminated";
    }
    if error.contains("copyright") {
        return "unavailable.copyright";
    }
    if error.contains("private video") {
        return "unavailable.privateVideo";
    }
    if error.contains("removed by the uploader") {
        return "unavailable.removedByUploader";
    }
    if error.contains("members-only") || error.contains("members only") {
        return "unavailable.membersOnly";
    }
    if error.contains("confirm your age") || error.contains("age-restricted") {
        return "unavailable.ageRestricted";
    }
    // A bare "available in your country" covers both wordings YouTube uses:
    // "...has not made this video available in your country" and "...is not
    // available in your country". Looking for the whole phrase of one of them
    // left the other out.
    if error.contains("available in your country") || error.contains("blocked it in your country") {
        return "unavailable.geoBlocked";
    }
    if error.contains("this live event has ended") {
        return "unavailable.liveEnded";
    }
    GENERIC
}

#[cfg(test)]
mod tests {
    use super::*;

    // These messages are copied EXACTLY from really querying the untitled ids of
    // a real playlist (39 out of 721) — they are not made up. If YouTube changes
    // the wording, this is where it shows.

    #[test]
    fn recognises_the_real_youtube_messages() {
        assert_eq!(reason_key(Some(
            "ERROR: [youtube] 97rea-g1KXA: Video unavailable. This video is no longer available because \
             the YouTube account associated with this video has been terminated.")), "unavailable.accountTerminated");
        assert_eq!(reason_key(Some(
            "ERROR: [youtube] 3wNQHnEXBLs: Video unavailable. It was removed following a copyright \
             removal request.")), "unavailable.copyright");
        assert_eq!(reason_key(Some(
            "ERROR: [youtube] NQQgXk7YIow: Private video. Sign in if you've been granted access to this video")),
            "unavailable.privateVideo");
    }

    #[test]
    fn specific_beats_generic_because_youtube_messages_overlap() {
        // "closed account" and "copyright" START with "Video unavailable", so
        // looking at the generic one first would swallow them both.
        let terminated = "Video unavailable. This video is no longer available because the YouTube \
                          account associated with this video has been terminated.";
        assert_ne!(reason_key(Some(terminated)), GENERIC);
        let copyright = "Video unavailable. It was removed following a copyright removal request.";
        assert_ne!(reason_key(Some(copyright)), GENERIC);
    }

    #[test]
    fn recognises_the_other_common_cases() {
        assert_eq!(reason_key(Some("Video unavailable. This video has been removed by the uploader")), "unavailable.removedByUploader");
        assert_eq!(reason_key(Some("Join this channel to get access to members-only content")), "unavailable.membersOnly");
        assert_eq!(reason_key(Some("Sign in to confirm your age. This video may be inappropriate for some users.")), "unavailable.ageRestricted");
        assert_eq!(reason_key(Some("Video unavailable. The uploader has not made this video available in your country")), "unavailable.geoBlocked");
        assert_eq!(reason_key(Some("This live event has ended.")), "unavailable.liveEnded");
    }

    #[test]
    fn what_is_not_recognised_falls_back_to_the_generic_reason() {
        assert_eq!(reason_key(None), GENERIC);
        assert_eq!(reason_key(Some("")), GENERIC);
        assert_eq!(reason_key(Some("   ")), GENERIC);
        assert_eq!(reason_key(Some("something YouTube will say two years from now")), GENERIC);
    }

    #[test]
    fn a_bot_check_is_never_permanent_marking_it_would_take_down_the_whole_playlist() {
        // With YouTube throttling the session EVERY video fails with this error,
        // the good ones included. If it counted as permanent, a single sync with
        // a blocked session would mark the whole playlist unavailable and those
        // files would never be attempted again.
        let bot_check = "ERROR: [youtube] ttcYCchFajo: Sign in to confirm you're not a bot. \
                         Use --cookies-from-browser or --cookies for the authentication.";
        assert!(!is_permanent(Some(bot_check)));
        // Not even if it comes alongside something that sounds final.
        assert!(!is_permanent(Some("ERROR: Video unavailable. Sign in to confirm you're not a bot.")));
    }

    #[test]
    fn what_is_actually_permanent() {
        assert!(is_permanent(Some("Private video. Sign in if you've been granted access")));
        assert!(is_permanent(Some(
            "Video unavailable. This video is no longer available because the YouTube account \
             associated with this video has been terminated.")));
        assert!(is_permanent(Some("Video unavailable. It was removed following a copyright removal request.")));
        assert!(is_permanent(Some("This video has been removed by the uploader")));
    }

    #[test]
    fn anything_temporary_or_session_dependent_is_not_marked() {
        // With other cookies these CAN work: marking them would mean giving them
        // up forever over a session problem.
        assert!(!is_permanent(Some("Sign in to confirm your age")));
        assert!(!is_permanent(Some("Join this channel to get access to members-only content")));
        assert!(!is_permanent(Some("HTTP Error 429: Too Many Requests")));
        assert!(!is_permanent(Some("Unable to download webpage: timed out")));
        assert!(!is_permanent(None));
        assert!(!is_permanent(Some("")));
    }

    #[test]
    fn is_case_insensitive() {
        assert_eq!(reason_key(Some("PRIVATE VIDEO")), "unavailable.privateVideo");
        assert_eq!(reason_key(Some("private video")), "unavailable.privateVideo");
    }
}
