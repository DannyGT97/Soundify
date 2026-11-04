//! The picture picked in «Descarga por URL», written into the downloaded file.
//! A loose download is not registered anywhere, so its file is the only place
//! that picture can live. yt-dlp has already embedded the video's own
//! thumbnail; this replaces it, in plain Rust (it runs on Android too).

use std::io;
use std::path::Path;

use lofty::config::WriteOptions;
use lofty::file::TaggedFileExt;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::tag::{Tag, TagExt};

use crate::cover_art::looks_like_image;

/// Whether a picked picture can go into a file of `format`. WAV has no usual
/// place for one, and Matroska, AVI and FLV are outside what `lofty` writes:
/// there the file keeps what yt-dlp gave it. `src/components/form.ts` says so.
pub fn can_embed(format: &str) -> bool {
    !matches!(format.to_uppercase().as_str(), "WAV" | "MKV" | "WEBM" | "AVI" | "FLV")
}

/// Makes `image` the only picture of `file`.
pub fn embed_cover(file: &Path, image: &Path) -> io::Result<()> {
    let bytes = std::fs::read(image)?;
    if !looks_like_image(&bytes) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("Not an image: {}", image.display())));
    }
    let picture = Picture::unchecked(bytes.clone()).pic_type(PictureType::CoverFront).mime_type(mime_of(&bytes)).build();

    let tagged = lofty::read_from_path(file).map_err(io::Error::other)?;
    let tag_type = tagged.primary_tag_type();
    let mut tag = tagged.primary_tag().cloned().unwrap_or_else(|| Tag::new(tag_type));
    while !tag.pictures().is_empty() {
        tag.remove_picture(0);
    }
    tag.push_picture(picture);
    tag.save_to_path(file, WriteOptions::default()).map_err(io::Error::other)
}

fn mime_of(bytes: &[u8]) -> MimeType {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        MimeType::Jpeg
    } else if bytes.starts_with(&[0x89, b'P']) {
        MimeType::Png
    } else if bytes.starts_with(b"GIF") {
        MimeType::Gif
    } else if bytes.starts_with(b"BM") {
        MimeType::Bmp
    } else {
        MimeType::Unknown("image/webp".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use lofty::file::TaggedFileExt;

    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3, 4];

    /// A few silent MPEG-1 Layer III frames: all `lofty` needs to call it an MP3.
    fn mp3() -> Vec<u8> {
        let mut frame = vec![0xFF, 0xFB, 0x90, 0x00];
        frame.resize(417, 0);
        frame.repeat(8)
    }

    fn pictures_of(file: &Path) -> Vec<Vec<u8>> {
        let tagged = lofty::read_from_path(file).unwrap();
        tagged.primary_tag().map_or(Vec::new(), |tag| tag.pictures().iter().map(|p| p.data().to_vec()).collect())
    }

    #[test]
    fn embeds_the_picture_and_replaces_the_one_already_there() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("song.mp3");
        std::fs::write(&file, mp3()).unwrap();
        let first = dir.path().join("first.jpg");
        std::fs::write(&first, JPEG).unwrap();
        embed_cover(&file, &first).unwrap();
        assert_eq!(pictures_of(&file), vec![JPEG.to_vec()]);

        let second_bytes = [JPEG, &[9, 9]].concat();
        let second = dir.path().join("second.jpg");
        std::fs::write(&second, &second_bytes).unwrap();
        embed_cover(&file, &second).unwrap();
        assert_eq!(pictures_of(&file), vec![second_bytes]);
    }

    #[test]
    fn a_file_that_is_not_a_picture_leaves_the_song_alone() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("song.mp3");
        std::fs::write(&file, mp3()).unwrap();
        let fake = dir.path().join("fake.jpg");
        std::fs::write(&fake, b"not a picture").unwrap();
        assert!(embed_cover(&file, &fake).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), mp3());
    }

    #[test]
    fn only_the_formats_with_a_place_for_it_take_a_picture() {
        for format in ["MP3", "flac", "OPUS", "M4A", "MP4"] {
            assert!(can_embed(format), "{format}");
        }
        for format in ["WAV", "mkv", "WEBM", "AVI", "FLV"] {
            assert!(!can_embed(format), "{format}");
        }
    }
}
