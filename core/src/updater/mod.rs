//! Updating the app itself: which version is published on GitHub, downloading
//! it in a checkable way and swapping it in (the Java `AppUpdater`,
//! `UpdateVerifier`, `UpdateDownloader`, `UpdateInstaller` and `ReleaseNotes`).
//!
//! **Not `tauri-plugin-updater`**: the plugin installs NSIS/MSI packages
//! signed with minisign and reads a `latest.json` of its own, while Soundify
//! ships a portable ZIP signed with the Ed25519 key the Java app already
//! trusts. Porting the Java design keeps the release shape, the key and the
//! "nothing unverified gets installed" rule.
//!
//! **GitHub, not the Java app's GitLab project**: this app's code lives on
//! GitHub, and sharing the GitLab project would have mixed both apps' tags
//! (the Java app already published a `v1.0`) and let the Java portable,
//! which looks for its ZIP by prefix, pick up this one's.
//!
//! **The repository is public**, so the query carries no token and no
//! credential: all that leaves the machine is a GET to the public API with the
//! `User-Agent: Soundify/<version>` header. GitHub unavoidably sees the IP and
//! the time, which is why the automatic check is daily and can be turned off
//! (it also keeps far below the 60 unauthenticated requests an hour).
//!
//! The expected shape of a release: a `v1.0.0` tag and three assets whose file
//! names end in [`ZIP_SUFFIX`], [`HASH_SUFFIX`] and [`SIGNATURE_SUFFIX`]
//! (`scripts/package-portable.mjs` makes all three). Without all three there
//! is no possible update: with no hash and no signature there is no way of
//! knowing that what arrives is what was published.
//!
//! **Why `-tauri` in the suffixes**: the Java portable is also a
//! `Soundify-<version>.zip` holding a `Soundify.exe` that would be recognised
//! as the app, so one attached by mistake would install the Java app over
//! this one. Only the releases carrying this app's own three assets count.

mod download;
mod install;
mod release_notes;
mod verify;

use chrono::NaiveDate;
use log::{info, warn};
use serde::{Deserialize, Serialize};

use crate::http::Http;
use crate::version;

pub use download::download;
pub use install::{
    APPLY_FLAG, OLD_SUFFIX, app_dir_from_args, clean_up_after_update, find_app_root, safe_target, stage, swap_into,
    wait_until_writable,
};
pub use release_notes::to_plain_text;
pub use verify::{is_trusted_source, sha256_file};

/// The app's public GitHub repository, as `owner/name`.
pub const REPOSITORY: &str = "DannyGT97/Soundify";

const RELEASES_URL: &str = "https://api.github.com/repos/DannyGT97/Soundify/releases";

pub const ZIP_SUFFIX: &str = "-tauri.zip";
pub const HASH_SUFFIX: &str = "-tauri.zip.sha256";
pub const SIGNATURE_SUFFIX: &str = "-tauri.zip.sig";

pub const APK_SUFFIX: &str = "-tauri.apk";
pub const APK_HASH_SUFFIX: &str = "-tauri.apk.sha256";
pub const APK_SIGNATURE_SUFFIX: &str = "-tauri.apk.sig";

/// What a release ships for one platform: the portable ZIP on Windows, the
/// APK on Android. Both come with the same two companions (hash and Ed25519
/// signature) and go through the same verification; only the names differ,
/// and each platform only ever looks at its own, so a release can carry both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Package {
    Zip,
    Apk,
}

impl Package {
    /// The package this build can install.
    pub const fn current() -> Self {
        if cfg!(target_os = "android") { Self::Apk } else { Self::Zip }
    }

    /// The end of the package's file name (`Soundify-<version><suffix>`).
    pub const fn suffix(self) -> &'static str {
        match self {
            Self::Zip => ZIP_SUFFIX,
            Self::Apk => APK_SUFFIX,
        }
    }

    pub const fn hash_suffix(self) -> &'static str {
        match self {
            Self::Zip => HASH_SUFFIX,
            Self::Apk => APK_HASH_SUFFIX,
        }
    }

    pub const fn signature_suffix(self) -> &'static str {
        match self {
            Self::Zip => SIGNATURE_SUFFIX,
            Self::Apk => APK_SIGNATURE_SUFFIX,
        }
    }
}

// === ERRORS ===

/// Why an update could not be checked or installed. The interface shows it
/// through [`UpdateError::key`] and [`UpdateError::args`] (the Java app's keys).
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateError {
    UntrustedSource(String),
    BadHashFile,
    HashMismatch { expected: String, actual: String },
    SignatureEmpty,
    SignatureInvalid,
    KeyUnreadable(String),
    EmptyZip,
    NoAppInZip(String),
    Io(String),
}

impl UpdateError {
    pub fn key(&self) -> &'static str {
        match self {
            Self::UntrustedSource(_) => "update.untrustedSource",
            Self::BadHashFile => "update.badHashFile",
            Self::HashMismatch { .. } => "update.hashMismatch",
            Self::SignatureEmpty => "update.signatureEmpty",
            Self::SignatureInvalid => "update.signatureInvalid",
            Self::KeyUnreadable(_) => "update.keyUnreadable",
            Self::EmptyZip => "update.emptyZip",
            Self::NoAppInZip(_) => "update.noAppInZip",
            Self::Io(_) => "update.ioError",
        }
    }

    pub fn args(&self) -> Vec<String> {
        match self {
            Self::UntrustedSource(text) | Self::KeyUnreadable(text) | Self::NoAppInZip(text) | Self::Io(text) => {
                vec![text.clone()]
            }
            Self::HashMismatch { expected, actual } => vec![expected.clone(), actual.clone()],
            Self::BadHashFile | Self::SignatureEmpty | Self::SignatureInvalid | Self::EmptyZip => vec![],
        }
    }
}

impl From<std::io::Error> for UpdateError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

// === MODEL ===

/// A published version, with the three pieces needed to install it in a
/// checkable way. `notes` is already plain text (see [`to_plain_text`]).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    pub version: String,
    pub notes: String,
    pub package: Package,
    /// The ZIP or the APK, depending on [`Update::package`].
    pub package_url: String,
    pub hash_url: String,
    pub signature_url: String,
}

/// The fields read from the GitHub releases API.
#[derive(Debug, Default, Deserialize)]
pub struct Release {
    #[serde(default)]
    pub tag_name: Option<String>,
    /// The release notes, in Markdown.
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Option<Vec<Option<Asset>>>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Asset {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub browser_download_url: Option<String>,
}

// === QUERY ===

/// Is it due to look today? Only if the user has it enabled and it has not
/// been checked today already.
///
/// One request a day instead of one per start. Only the DATE is stored, not
/// the time: deciding this needs no more precision, and less precision is
/// less data stored.
pub fn should_check_today(enabled: bool, last_check: &str, today: NaiveDate) -> bool {
    if !enabled {
        return false;
    }
    // An unreadable date (hand-edited, from another version) is checked and
    // written back properly, instead of getting stuck.
    NaiveDate::parse_from_str(last_check.trim(), "%Y-%m-%d") != Ok(today)
}

/// The date exactly as it is stored in `settings.json` (`lastAppUpdateCheck`).
pub fn iso_date(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// The highest installable version, if it is later than `current`. None if
/// already up to date or if no release carries this app's three assets.
pub fn find_newer(http: &dyn Http, current: &str, package: Package) -> Result<Option<Update>, UpdateError> {
    let body = http.get_string(RELEASES_URL, &[])?;
    let releases: Vec<Option<Release>> =
        serde_json::from_str(&body).map_err(|e| UpdateError::Io(e.to_string()))?;

    let Some(newest) = newest_installable(releases.iter().flatten(), package) else {
        // Warned about loudly: a release without its three pieces is a slip
        // when publishing, and from the outside it looks as if the app detects nothing.
        warn!(
            "No release on GitHub carries the three assets ({}, {}, {}).",
            package.suffix(),
            package.hash_suffix(),
            package.signature_suffix()
        );
        return Ok(None);
    };
    if !version::is_newer_than(Some(&newest.version), Some(current)) {
        info!("Soundify is up to date ({current}).");
        return Ok(None);
    }
    Ok(Some(newest))
}

// === PURE LOGIC ===

/// The installable release with the highest version. **The first in the list
/// is not taken**: the order is GitHub's decision and part of no contract, so
/// the versions are really compared. Releases without this app's three assets
/// (a slip when publishing) are not candidates at all.
pub fn newest_installable<'a>(
    releases: impl IntoIterator<Item = &'a Release>,
    package: Package,
) -> Option<Update> {
    let mut best: Option<Update> = None;
    for update in releases.into_iter().filter_map(|r| to_update(r, package)) {
        if best.as_ref().is_none_or(|b| version::is_newer_than(Some(&update.version), Some(&b.version))) {
            best = Some(update);
        }
    }
    best
}

/// A release's three URLs, if it has a tag and all three are there and can be
/// trusted. The origin is checked HERE, before anything is requested: the URLs
/// come from a JSON, and a JSON is something arriving from outside. A
/// pre-release is published to be tried by hand, never offered to everyone.
pub fn to_update(release: &Release, package: Package) -> Option<Update> {
    if release.draft || release.prerelease {
        return None;
    }
    let version = release.tag_name.as_deref().filter(|t| !t.trim().is_empty())?;
    Some(Update {
        version: version.to_string(),
        notes: to_plain_text(release.body.as_deref()),
        package,
        package_url: asset_url(release, package.suffix())?,
        hash_url: asset_url(release, package.hash_suffix())?,
        signature_url: asset_url(release, package.signature_suffix())?,
    })
}

/// The URL of the asset whose file ends in `suffix`.
///
/// The end of the URL is looked at and not the asset name: the name is free
/// text written by hand when publishing ("1.2 package"), whereas the URL ends
/// in the real file. If it still does not add up, the name is tried as a last resort.
pub fn asset_url(release: &Release, suffix: &str) -> Option<String> {
    let assets = release.assets.as_ref()?;
    let mut by_name = None;
    for asset in assets.iter().flatten() {
        let Some(url) = asset.browser_download_url.as_deref() else { continue };
        if !is_trusted_source(url) {
            continue;
        }
        if ends_with(url, suffix) {
            return Some(url.to_string());
        }
        if by_name.is_none() && asset.name.as_deref().is_some_and(|n| ends_with(n, suffix)) {
            by_name = Some(url.to_string());
        }
    }
    by_name
}

fn ends_with(value: &str, suffix: &str) -> bool {
    value.to_lowercase().ends_with(suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://github.com/DannyGT97/Soundify/releases/download/v1.8/";

    /// One asset as the GitHub API lists it.
    fn asset(name: &str, url: &str) -> String {
        format!(r#"{{"name":"{name}","size":1,"browser_download_url":"{url}"}}"#)
    }

    /// A release with the three assets hanging off the real repository, as
    /// JSON exactly as the GitHub API returns it: this also pins down the mapping.
    fn release(tag: &str) -> String {
        let version = tag.trim_start_matches('v');
        let name = format!("Soundify-{version}-tauri.zip");
        let file = format!("https://github.com/DannyGT97/Soundify/releases/download/{tag}/{name}");
        format!(
            r#"{{"tag_name":"{tag}","name":"Soundify {version}","draft":false,"prerelease":false,"assets":[{},{},{}]}}"#,
            asset(&name, &file),
            asset(&format!("{name}.sha256"), &format!("{file}.sha256")),
            asset(&format!("{name}.sig"), &format!("{file}.sig")),
        )
    }

    fn parse(json: &str) -> Vec<Release> {
        serde_json::from_str(json).unwrap()
    }

    fn newest_tag(json: &str) -> Option<String> {
        newest_installable(&parse(json), Package::Zip).map(|u| u.version)
    }

    // --- which release is picked ---

    #[test]
    fn the_highest_version_wins_not_the_first_in_the_list() {
        // GitHub usually returns them sorted, but that is no contract.
        let json = format!("[{},{},{}]", release("v1.2"), release("v1.9"), release("v1.7"));
        assert_eq!(newest_tag(&json).as_deref(), Some("v1.9"));
    }

    #[test]
    fn regression_1_10_is_later_than_1_9() {
        let json = format!("[{},{}]", release("v1.9"), release("v1.10"));
        assert_eq!(newest_tag(&json).as_deref(), Some("v1.10"));
    }

    #[test]
    fn three_component_tags_are_compared() {
        let json = format!("[{},{},{},{}]", release("v1.8.1"), release("v1.8.10"), release("v1.8.0"), release("v1.7.9"));
        assert_eq!(newest_tag(&json).as_deref(), Some("v1.8.10"));
    }

    #[test]
    fn a_trailing_zero_is_not_a_new_version() {
        assert!(!version::is_newer_than(Some("v1.8.0"), Some("1.8")));
        assert!(!version::is_newer_than(Some("v1.8"), Some("1.8.0")));
        assert!(version::is_newer_than(Some("v1.8.1"), Some("1.8")));
        assert!(version::is_newer_than(Some("v1.8.0"), Some("1.7.9")));
        assert!(!version::is_newer_than(Some("v1.2"), Some("1.7.7")));
    }

    #[test]
    fn with_no_usable_releases_none_is_chosen() {
        assert_eq!(newest_tag("[]"), None);
        assert_eq!(newest_tag(r#"[{"name":"sin tag"}]"#), None, "a release with no tag cannot be compared");
    }

    #[test]
    fn a_java_portable_is_never_offered_even_when_higher() {
        // The Java portable is a plain .zip with its own Soundify.exe: attached
        // by mistake, it must never be installed over this app.
        let java = format!(
            r#"{{"tag_name":"v1.4.0","assets":[{},{},{}]}}"#,
            asset("Soundify-1.4.0.zip", &format!("{BASE}Soundify-1.4.0.zip")),
            asset("Soundify-1.4.0.zip.sha256", &format!("{BASE}Soundify-1.4.0.zip.sha256")),
            asset("Soundify-1.4.0.zip.sig", &format!("{BASE}Soundify-1.4.0.zip.sig")),
        );
        let json = format!("[{java},{}]", release("v0.2.0"));
        assert_eq!(newest_tag(&json).as_deref(), Some("v0.2.0"));
    }

    #[test]
    fn drafts_and_pre_releases_are_never_offered() {
        let draft = release("v2.0").replace(r#""draft":false"#, r#""draft":true"#);
        let pre = release("v1.9").replace(r#""prerelease":false"#, r#""prerelease":true"#);
        let json = format!("[{draft},{pre},{}]", release("v1.2"));
        assert_eq!(newest_tag(&json).as_deref(), Some("v1.2"));
    }

    // --- the three pieces ---

    #[test]
    fn the_zip_its_hash_and_its_signature_are_found_by_the_end_of_the_url() {
        let update = to_update(&parse(&format!("[{}]", release("v1.8")))[0], Package::Zip).unwrap();

        assert_eq!(update.version, "v1.8");
        assert!(update.package_url.ends_with("Soundify-1.8-tauri.zip"), "{}", update.package_url);
        assert!(update.hash_url.ends_with(".zip.sha256"));
        assert!(update.signature_url.ends_with(".zip.sig"));
        assert_ne!(update.package_url, update.hash_url, "the .zip must not slip through as the .sha256");
    }

    #[test]
    fn no_signature_no_update() {
        let json = format!(
            r#"[{{"tag_name":"v1.8","assets":[{},{}]}}]"#,
            asset("Soundify-1.8-tauri.zip", &format!("{BASE}Soundify-1.8-tauri.zip")),
            asset("Soundify-1.8-tauri.zip.sha256", &format!("{BASE}Soundify-1.8-tauri.zip.sha256")),
        );
        assert_eq!(to_update(&parse(&json)[0], Package::Zip), None);
    }

    #[test]
    fn a_release_without_assets_does_not_count() {
        assert_eq!(to_update(&parse(r#"[{"tag_name":"v1.2","assets":[]}]"#)[0], Package::Zip), None);
        assert_eq!(to_update(&parse(r#"[{"tag_name":"v1.2"}]"#)[0], Package::Zip), None);
    }

    #[test]
    fn security_an_asset_hosted_outside_the_repository_is_discarded() {
        let json = format!(
            r#"[{{"tag_name":"v1.8","assets":[{},{},{}]}}]"#,
            asset("Soundify-1.8-tauri.zip", "https://malo.tld/Soundify-1.8-tauri.zip"),
            asset("Soundify-1.8-tauri.zip.sha256", &format!("{BASE}Soundify-1.8-tauri.zip.sha256")),
            asset("Soundify-1.8-tauri.zip.sig", &format!("{BASE}Soundify-1.8-tauri.zip.sig")),
        );
        let release = &parse(&json)[0];
        assert_eq!(asset_url(release, ZIP_SUFFIX), None);
        assert_eq!(to_update(release, Package::Zip), None, "with the trusted zip missing, there is no update to offer");
    }

    #[test]
    fn the_asset_name_works_as_a_fallback() {
        let json = format!(
            r#"[{{"tag_name":"v1.8","assets":[{},{},{}]}}]"#,
            asset("Soundify-1.8-tauri.zip", &format!("{BASE}descarga/1")),
            asset("Soundify-1.8-tauri.zip.sha256", &format!("{BASE}descarga/2")),
            asset("Soundify-1.8-tauri.zip.sig", &format!("{BASE}descarga/3")),
        );
        let update = to_update(&parse(&json)[0], Package::Zip).unwrap();
        assert!(update.package_url.ends_with("/1"));
        assert!(update.hash_url.ends_with("/2"));
        assert!(update.signature_url.ends_with("/3"));
    }

    #[test]
    fn the_notes_arrive_as_plain_text() {
        let json = format!("[{}]", release("v1.8").replacen('{', r###"{"body":"## Novedades\n- **Una**","###, 1));
        assert_eq!(to_update(&parse(&json)[0], Package::Zip).unwrap().notes, "NOVEDADES\n  • Una");
    }

    // --- the Android package ---

    /// A release carrying the APK's three assets, and nothing else.
    fn apk_release(tag: &str) -> String {
        let version = tag.trim_start_matches('v');
        let name = format!("Soundify-{version}-tauri.apk");
        let file = format!("https://github.com/DannyGT97/Soundify/releases/download/{tag}/{name}");
        format!(
            r#"{{"tag_name":"{tag}","assets":[{},{},{}]}}"#,
            asset(&name, &file),
            asset(&format!("{name}.sha256"), &format!("{file}.sha256")),
            asset(&format!("{name}.sig"), &format!("{file}.sig")),
        )
    }

    #[test]
    fn android_takes_the_apk_and_windows_the_zip_from_the_same_release() {
        // One release can carry both: each platform looks only at its own pieces.
        let apk_assets = format!(
            "{},{},{},",
            asset("Soundify-1.9-tauri.apk", &format!("{BASE}Soundify-1.9-tauri.apk")),
            asset("Soundify-1.9-tauri.apk.sha256", &format!("{BASE}Soundify-1.9-tauri.apk.sha256")),
            asset("Soundify-1.9-tauri.apk.sig", &format!("{BASE}Soundify-1.9-tauri.apk.sig")),
        );
        let both = release("v1.9").replacen("\"assets\":[", &format!("\"assets\":[{apk_assets}"), 1);
        let release = &parse(&format!("[{both}]"))[0];

        let apk = to_update(release, Package::Apk).unwrap();
        assert_eq!(apk.package, Package::Apk);
        assert!(apk.package_url.ends_with("Soundify-1.9-tauri.apk"), "{}", apk.package_url);
        assert!(apk.hash_url.ends_with(".apk.sha256"));
        assert!(apk.signature_url.ends_with(".apk.sig"));

        let zip = to_update(release, Package::Zip).unwrap();
        assert!(zip.package_url.ends_with("-tauri.zip"), "{}", zip.package_url);
    }

    #[test]
    fn a_zip_only_release_is_no_update_on_android_and_the_other_way_round() {
        let zip_only = parse(&format!("[{}]", release("v1.9")));
        let apk_only = parse(&format!("[{}]", apk_release("v1.9")));
        assert_eq!(newest_installable(&zip_only, Package::Apk), None);
        assert_eq!(newest_installable(&apk_only, Package::Zip), None);
        assert!(newest_installable(&apk_only, Package::Apk).is_some());
    }

    #[test]
    fn android_skips_a_newer_release_that_has_no_apk() {
        // 1.9 only shipped the ZIP: the tablet is offered 1.8, which has its APK.
        let releases = parse(&format!("[{},{}]", release("v1.9"), apk_release("v1.8")));
        let best = newest_installable(&releases, Package::Apk).unwrap();
        assert_eq!(best.version, "v1.8");
    }

    // --- when it is checked ---

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 8, 26).unwrap()
    }

    #[test]
    fn with_the_switch_off_it_never_queries() {
        assert!(!should_check_today(false, "", day()));
        assert!(!should_check_today(false, "2020-01-01", day()));
    }

    #[test]
    fn once_a_day() {
        assert!(should_check_today(true, "", day()), "never checked");
        assert!(should_check_today(true, "2026-08-25", day()), "yesterday it is due");
        assert!(!should_check_today(true, "2026-08-26", day()), "it was already checked today");
    }

    #[test]
    fn an_unreadable_date_does_not_block_the_check() {
        assert!(should_check_today(true, "ayer por la tarde", day()));
    }

    #[test]
    fn the_date_is_stored_in_iso_format() {
        assert_eq!(iso_date(day()), "2026-08-26");
        assert!(!should_check_today(true, &iso_date(day()), day()), "right after checking, it is no longer due");
    }

    // --- the query ---

    struct Releases(String);

    impl Http for Releases {
        fn get_string(&self, url: &str, _: &[(&str, &str)]) -> std::io::Result<String> {
            assert_eq!(url, RELEASES_URL);
            Ok(self.0.clone())
        }

        fn to_file(&self, _: &str, _: &std::path::Path, _: &mut dyn FnMut(f64)) -> std::io::Result<()> {
            unreachable!("checking downloads nothing")
        }
    }

    #[test]
    fn only_a_later_version_is_offered() {
        let http = Releases(format!("[{},{}]", release("v0.1.0"), release("v0.2.0")));
        assert_eq!(find_newer(&http, "0.1.0", Package::Zip).unwrap().map(|u| u.version).as_deref(), Some("v0.2.0"));
        assert_eq!(find_newer(&http, "0.2.0", Package::Zip).unwrap(), None);
    }

    #[test]
    fn a_broken_answer_is_an_error_not_an_empty_list() {
        assert!(matches!(find_newer(&Releases("<html>".into()), "0.1.0", Package::Zip), Err(UpdateError::Io(_))));
    }
}
