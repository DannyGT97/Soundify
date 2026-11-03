//! Extracts from a ZIP only the entries that matter. The ffmpeg and Deno
//! installers use it: they download a whole ZIP and keep one or two
//! executables from inside it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Walks the ZIP and extracts the entries `target_for` accepts, returning how
/// many were really extracted.
///
/// `target_for` receives the entry's name inside the ZIP and returns where to
/// write it, or `None` to skip it. It is the caller who decides the
/// destination path, so a ZIP with malicious names ("../..") cannot write
/// outside its folder.
pub fn extract_matching(zip: &Path, target_for: impl Fn(&str) -> Option<PathBuf>) -> io::Result<usize> {
    let mut archive = zip::ZipArchive::new(fs::File::open(zip)?).map_err(io::Error::other)?;
    let mut extracted = 0;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(io::Error::other)?;
        if is_directory_entry(entry.name(), entry.is_dir(), entry.size()) {
            continue;
        }
        let Some(target) = target_for(entry.name()) else { continue };
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = fs::File::create(&target)?;
        io::copy(&mut entry, &mut out)?;
        extracted += 1;
        log::info!("Extracted: {}", target.display());
    }

    Ok(extracted)
}

/// Whether this entry is a folder, accepting a backslash as well.
///
/// The ZIP format only allows "/", and that is all most readers look for. But
/// Windows PowerShell 5.1's `Compress-Archive` writes the names with "\", and
/// with that a folder passed for a file: a file was written carrying the
/// folder's name, and the next entry inside it blew up with NoSuchFileException.
///
/// This is a real and expensive bug: it is how every Java release up to 1.7.11
/// was packaged, and it is why the automatic update never got installed. This
/// stays as a safety net, because a ZIP can have been made by anyone.
fn is_directory_entry(name: &str, is_dir: bool, size: u64) -> bool {
    is_dir || (name.ends_with('\\') && size == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    struct Fixture {
        dir: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            Self { dir: tempfile::tempdir().unwrap() }
        }

        fn zip(&self) -> PathBuf {
            self.dir.path().join("tool.zip")
        }

        fn out(&self, name: &str) -> PathBuf {
            self.dir.path().join("bin").join(name)
        }

        /// Builds the zip: `Some(contents)` is a file, `None` a folder entry
        /// written exactly with that name.
        fn write_zip(&self, entries: &[(&str, Option<&str>)]) {
            let mut writer = zip::ZipWriter::new(fs::File::create(self.zip()).unwrap());
            for (name, contents) in entries {
                match contents {
                    Some(text) => {
                        writer.start_file(*name, SimpleFileOptions::default()).unwrap();
                        writer.write_all(text.as_bytes()).unwrap();
                    }
                    None if name.ends_with('/') => writer.add_directory(*name, SimpleFileOptions::default()).unwrap(),
                    None => writer.start_file(*name, SimpleFileOptions::default()).unwrap(),
                }
            }
            writer.finish().unwrap();
        }
    }

    #[test]
    fn regression_extracts_every_requested_entry_not_just_the_first() {
        // The Java code wrapped the ZipInputStream in a try-with-resources
        // inside the loop: copying the first entry closed the whole zip and the
        // second one never arrived. ffmpeg needs ffmpeg.exe AND ffprobe.exe, so
        // the installation failed.
        let f = Fixture::new();
        f.write_zip(&[
            ("ffmpeg-7.1-essentials_build/bin/ffmpeg.exe", Some("soy ffmpeg")),
            ("ffmpeg-7.1-essentials_build/bin/ffprobe.exe", Some("soy ffprobe")),
        ]);

        let extracted = extract_matching(&f.zip(), |name| {
            if name.ends_with("bin/ffmpeg.exe") {
                Some(f.out("ffmpeg.exe"))
            } else if name.ends_with("bin/ffprobe.exe") {
                Some(f.out("ffprobe.exe"))
            } else {
                None
            }
        })
        .unwrap();

        assert_eq!(extracted, 2);
        assert_eq!(fs::read_to_string(f.out("ffmpeg.exe")).unwrap(), "soy ffmpeg");
        assert_eq!(fs::read_to_string(f.out("ffprobe.exe")).unwrap(), "soy ffprobe");
    }

    #[test]
    fn skips_what_is_not_wanted() {
        let f = Fixture::new();
        f.write_zip(&[
            ("deno-x86_64-pc-windows-msvc/README.md", Some("documentación")),
            ("deno-x86_64-pc-windows-msvc/LICENSE", Some("licencia")),
            ("deno-x86_64-pc-windows-msvc/deno.exe", Some("soy deno")),
        ]);

        let extracted = extract_matching(&f.zip(), |name| name.ends_with("deno.exe").then(|| f.out("deno.exe"))).unwrap();

        assert_eq!(extracted, 1);
        assert!(f.out("deno.exe").exists());
        assert!(!f.out("README.md").exists());
    }

    #[test]
    fn reports_zero_instead_of_pretending_it_worked() {
        let f = Fixture::new();
        f.write_zip(&[("otra-cosa.txt", Some("nada útil"))]);
        assert_eq!(extract_matching(&f.zip(), |_| None).unwrap(), 0);
    }

    #[test]
    fn creates_missing_folders() {
        let f = Fixture::new();
        f.write_zip(&[("bin/deno.exe", Some("soy deno"))]);
        let target = f.dir.path().join("no/existe/todavia/deno.exe");

        extract_matching(&f.zip(), |_| Some(target.clone())).unwrap();

        assert!(target.exists());
    }

    #[test]
    fn overwrites_an_earlier_version() {
        let f = Fixture::new();
        let target = f.out("deno.exe");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "versión vieja").unwrap();
        f.write_zip(&[("bin/deno.exe", Some("versión nueva"))]);

        extract_matching(&f.zip(), |_| Some(target.clone())).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "versión nueva");
    }

    #[test]
    fn zip_directory_entries_are_not_treated_as_files() {
        // A real zip also carries directory entries ("bin/"): treating them as
        // files would write a folder as if it were a binary.
        let f = Fixture::new();
        f.write_zip(&[("bin/", None), ("bin/deno.exe", Some("soy deno"))]);

        let extracted = extract_matching(&f.zip(), |name| name.ends_with("deno.exe").then(|| f.out("deno.exe"))).unwrap();

        assert_eq!(extracted, 1);
        assert_eq!(fs::read_to_string(f.out("deno.exe")).unwrap(), "soy deno");
    }

    #[test]
    fn regression_a_folder_entry_ending_in_a_backslash_is_a_folder_too() {
        // Compress-Archive writes entry names with backslashes, which the ZIP
        // format does not allow, so "runtime\legal\" passed for a file: a file
        // named "legal" got written, and the next entry inside it blew up.
        // This is how every Java release up to 1.7.11 was packaged, and it is
        // why the automatic update never installed for anyone.
        let f = Fixture::new();
        f.write_zip(&[("runtime\\legal\\", None), ("runtime\\legal\\java.base\\LICENSE", Some("license"))]);

        let licence = f.out("legal/java.base/LICENSE");
        let extracted = extract_matching(&f.zip(), |name| name.ends_with("LICENSE").then(|| licence.clone())).unwrap();

        assert_eq!(extracted, 1);
        assert_eq!(fs::read_to_string(&licence).unwrap(), "license", "the folder entry must not have been written as a file");
    }

    #[test]
    fn a_backslash_name_with_contents_is_still_a_file() {
        assert!(is_directory_entry("runtime\\legal\\", false, 0));
        assert!(!is_directory_entry("runtime\\odd\\", false, 12));
        assert!(!is_directory_entry("bin/deno.exe", false, 0));
    }
}
