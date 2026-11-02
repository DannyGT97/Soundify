use std::path::{Path, PathBuf};

/// Where a file or a folder lives.
///
/// On desktop it is a plain path. On Android the music folder comes from the
/// Storage Access Framework as a `content://` URI, which is NOT a path: it
/// cannot be joined, canonicalized or opened with `std::fs`. That is why the
/// model never holds a `PathBuf` for a folder, and why joining a name onto a
/// folder is asked of [`crate::storage::Storage::child`] instead of done here.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Location {
    Path(PathBuf),
    Uri(String),
}

impl Location {
    pub fn path(path: impl Into<PathBuf>) -> Self {
        Location::Path(path.into())
    }

    pub fn uri(uri: impl Into<String>) -> Self {
        Location::Uri(uri.into())
    }

    /// The path, when this is one. Code that needs it has a desktop-only reason.
    pub fn as_path(&self) -> Option<&Path> {
        match self {
            Location::Path(path) => Some(path),
            Location::Uri(_) => None,
        }
    }

    /// How an archive stores it: `content://` is a URI, anything else a path.
    pub fn parse(stored: &str) -> Self {
        if stored.starts_with("content://") {
            Location::Uri(stored.to_string())
        } else {
            Location::Path(PathBuf::from(stored))
        }
    }

    /// The last name in it ("Chill (mp3)"). A SAF URI encodes the whole
    /// document path in its last segment ("primary%3AMusic%2FChill"), so it is
    /// decoded and cut at its own separators too.
    pub fn name(&self) -> String {
        match self {
            Location::Path(path) => path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            Location::Uri(uri) => {
                let last = uri.rsplit('/').next().unwrap_or_default();
                let decoded = percent_decode(last);
                decoded.rsplit(['/', ':']).next().unwrap_or_default().to_string()
            }
        }
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok());
        if bytes[i] == b'%'
            && let Some(byte) = hex.and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl std::fmt::Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Location::Path(path) => write!(f, "{}", path.display()),
            Location::Uri(uri) => f.write_str(uri),
        }
    }
}

/// Crosses to the front (and back) as the same plain text an archive stores.
impl serde::Serialize for Location {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for Location {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(|text| Location::parse(&text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crosses_as_plain_text() {
        let uri = Location::uri("content://tree/x");
        let json = serde_json::to_string(&uri).unwrap();
        assert_eq!(json, "\"content://tree/x\"");
        assert_eq!(serde_json::from_str::<Location>(&json).unwrap(), uri);
    }

    #[test]
    fn only_a_path_location_gives_a_path() {
        assert_eq!(Location::path("music").as_path(), Some(Path::new("music")));
        assert_eq!(Location::uri("content://tree/music").as_path(), None);
    }

    #[test]
    fn a_stored_content_uri_is_not_taken_for_a_path() {
        assert_eq!(Location::parse("content://tree/x"), Location::uri("content://tree/x"));
        assert_eq!(Location::parse("D:\\Music\\a.mp3"), Location::path("D:\\Music\\a.mp3"));
    }

    #[test]
    fn the_name_of_a_saf_uri_is_its_last_folder() {
        let uri = "content://com.android.externalstorage.documents/tree/primary%3AMusic%2FChill%20(mp3)";
        assert_eq!(Location::uri(uri).name(), "Chill (mp3)");
        assert_eq!(Location::uri("content://tree/primary%3AMusic").name(), "Music");
        assert_eq!(Location::path("/music/Chill (mp3)").name(), "Chill (mp3)");
    }

    #[test]
    fn displays_the_uri_untouched() {
        let uri = "content://com.android.externalstorage.documents/tree/primary%3AMusic";
        assert_eq!(Location::uri(uri).to_string(), uri);
    }
}
