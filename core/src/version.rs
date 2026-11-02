//! Comparing app versions ("1.7" against a release tag, which can come as
//! "v1.7.1"). Kept apart so it can be pinned down by tests: if it fails, the
//! app announces updates that do not exist (or keeps quiet when there is one),
//! and in neither case is it noticed until it is too late.

/// Is the installed version up to date with respect to the latest published one?
///
/// It compares component by component, padding the missing ones with zeros, so
/// "1.7" and "1.7.0" are the same version, and "1.10" comes after "1.9"
/// (comparing as text would go wrong right there).
///
/// If either of the two has no recognisable version shape, the answer is "up to
/// date": not enough is known to bother the user with an update that might not
/// exist.
pub fn is_up_to_date(current: Option<&str>, latest: Option<&str>) -> bool {
    let current = parts(current);
    let latest = parts(latest);
    if current.is_empty() || latest.is_empty() {
        return true;
    }

    for i in 0..current.len().max(latest.len()) {
        let c = current.get(i).copied().unwrap_or(0);
        let l = latest.get(i).copied().unwrap_or(0);
        if l != c {
            return l < c;
        }
    }
    true
}

/// Is `candidate` later than `reference`? The exact reverse of [`is_up_to_date`],
/// so releases can be SORTED without trusting the server to return them in the
/// expected order. With an unrecognisable version it answers false: when in
/// doubt, nothing gets promoted to "newer".
pub fn is_newer_than(candidate: Option<&str>, reference: Option<&str>) -> bool {
    !is_up_to_date(reference, candidate)
}

fn parts(version: Option<&str>) -> Vec<u64> {
    let Some(version) = version else { return Vec::new() };
    let mut cleaned = version.trim();
    // Release tags usually carry a "v" in front ("v1.7"). ONLY the leading one
    // is stripped: a plain replace("v", "") would wreck anything else carrying a v.
    if let Some(rest) = cleaned.strip_prefix(['v', 'V']) {
        cleaned = rest.trim();
    }

    // Only the numeric part at the START: "1.7.2-beta3" is 1.7.2, and the 3 in
    // "beta3" is not a version component. Taking every loose group of digits
    // would give 1.7.2.3, which compares wrongly.
    let mut result = Vec::new();
    for piece in cleaned.split('.') {
        let digits: &str = &piece[..piece.find(|c: char| !c.is_ascii_digit()).unwrap_or(piece.len())];
        if digits.is_empty() {
            break;
        }
        // An absurdly long component (more than fits) must not bring down the whole check.
        result.push(digits.parse().unwrap_or(0));
        if digits.len() < piece.len() {
            break;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn up(current: &str, latest: &str) -> bool {
        is_up_to_date(Some(current), Some(latest))
    }

    #[test]
    fn a_later_version_means_there_is_an_update() {
        assert!(!up("1.7", "1.8"));
        assert!(!up("1.7", "2.0"));
        assert!(!up("1.7.0", "1.7.1"));
    }

    #[test]
    fn the_same_version_is_up_to_date() {
        assert!(up("1.7", "1.7"));
    }

    #[test]
    fn a_version_newer_than_the_published_one_is_up_to_date_too() {
        // Happens in development: working on 1.8 while the latest release is 1.7.
        assert!(up("1.8", "1.7"));
    }

    #[test]
    fn missing_components_count_as_zero() {
        assert!(up("1.7", "1.7.0"));
        assert!(up("1.7.0", "1.7"));
        assert!(!up("1.7", "1.7.0.1"));
    }

    #[test]
    fn compares_numbers_not_text() {
        assert!(!up("1.9", "1.10"));
        assert!(up("1.10", "1.9"));
    }

    #[test]
    fn two_digits_in_the_last_component_too() {
        // This is the real 1.7.10 case: compared as text, "9" > "10", and anyone
        // on 1.7.9 would never see the update.
        assert!(!up("1.7.9", "1.7.10"));
        assert!(up("1.7.10", "1.7.9"));
        assert!(is_newer_than(Some("1.7.10"), Some("1.7.9")));
        assert!(!is_newer_than(Some("1.7.9"), Some("1.7.10")));
    }

    #[test]
    fn accepts_the_prefix_of_release_tags() {
        assert!(up("1.7", "v1.7"));
        assert!(!up("1.7", "v1.8"));
        assert!(up("v1.7", "V1.7"));
    }

    #[test]
    fn only_the_leading_v_is_stripped() {
        // The old normalize did a plain replace("v", ""): "1.7-preview" turned
        // into "1.7-preiew" and could break the parsing.
        assert!(up("1.7-preview", "1.7"));
        assert!(!up("1.7-preview", "1.8"));
    }

    #[test]
    fn ignores_whatever_follows_the_numeric_part() {
        // "1.7.2-beta3": the version is 1.7.2 — the 3 in "beta3" is not one more component.
        assert!(up("1.7.2-beta3", "1.7.2"));
        assert!(up("1.7.2", "1.7.2-beta3"));
    }

    #[test]
    fn with_no_readable_version_no_update_is_announced() {
        // "dev null" was the real value the Java app returned when running from the IDE.
        assert!(up("dev null", "1.8"));
        assert!(up("desconocida", "1.8"));
        assert!(is_up_to_date(None, Some("1.8")));
        assert!(up("", "1.8"));
    }

    #[test]
    fn a_release_tag_with_no_numbers_does_not_trigger_anything_either() {
        assert!(up("1.7", "latest"));
        assert!(is_up_to_date(Some("1.7"), None));
    }

    #[test]
    fn surrounding_whitespace_does_not_matter() {
        assert!(up("  1.7  ", " v1.7 "));
    }

    #[test]
    fn never_panics() {
        up("...", "..");
        up("99999999999999999999999", "1.0");
        up("1.7", "1.7.");
        up("v", "v");
    }
}
