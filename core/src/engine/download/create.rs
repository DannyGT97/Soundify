//! «Crear Playlist» (the Java `NewPlaylistController.doCreate`): asks YouTube
//! for the title and the videos and registers the playlist WITHOUT
//! downloading, so it is already in the Gestor when the screen gets there.
//! «Crear y descargar» does this first and then runs a normal update.

use std::collections::HashMap;
use std::io;

use indexmap::IndexMap;

use crate::archive::{NumberMode, io as archive_io};
use crate::engine::download::Engine;
use crate::location::Location;
use crate::paths;
use crate::settings;
use crate::ytdlp::query::{self, PlaylistEntry};

/// A playlist just registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    /// Its archive: the block key the Gestor and an update go by.
    pub json: Location,
    pub title: String,
    /// The videos listed when it was created.
    pub count: usize,
    /// The real title could not be had and the generic one was used.
    pub generic_title: bool,
    pub number_mode: NumberMode,
}

/// What YouTube said about a playlist about to be registered.
pub(super) struct NewPlaylist {
    pub url: String,
    pub title: String,
    pub generic_title: bool,
    pub entries: IndexMap<String, PlaylistEntry>,
}

#[derive(Debug)]
pub enum CreateError {
    /// The configured cookies no longer give access (told apart so the user
    /// is not sent to check things that are not the cause).
    CookiesExpired,
    /// Bad URL, no connection, a private playlist without cookies...
    QueryFailed,
    Io(io::Error),
}

impl From<io::Error> for CreateError {
    fn from(e: io::Error) -> Self {
        CreateError::Io(e)
    }
}

// === PUBLIC API ===

impl Engine<'_> {
    pub fn create_playlist(&self, raw_url: &str, format: &str, base: &Location, numbered: bool) -> Result<Created, CreateError> {
        let storage = self.archives.storage();
        let NewPlaylist { url, title, generic_title, entries } = self.look_up_new_playlist(raw_url, format)?;

        let dir = paths::playlist_archive_dir(storage, self.archives.archives(), &title, base, format)?;
        storage.create_dir_all(&dir)?;
        let json = storage.child(&dir, archive_io::DATA_JSON)?;
        // Immutable once fixed: an existing playlist+folder keeps its own mode
        // and the checkbox is ignored (said in the message, not silently).
        let number_mode = self.archives.resolve_number_mode(&json, numbered);
        // Created now, as the Java app did: the manifest is written into it, and
        // a folder that is not there would read as a playlist that went away.
        let folder = paths::playlist_folder(storage, base, &title, format)?;
        storage.create_dir_all(&folder)?;

        let items: Vec<(String, String)> =
            entries.iter().map(|(id, e)| (id.clone(), e.title.clone().unwrap_or_default())).collect();
        self.archives.register_playlist(&json, Some(&url), Some(&folder), &items, number_mode.number_files)?;
        // Stored from the start: it is the order «Ver archivos» shows an
        // unnumbered playlist in.
        let order: HashMap<String, i32> =
            entries.iter().map(|(id, e)| (id.clone(), i32::try_from(e.position).unwrap_or(i32::MAX))).collect();
        self.archives.refresh_playlist_order(&json, &order)?;

        log::info!(
            "Playlist \"{title}\" registered from «Crear Playlist» ({} ids, {}, by {:?}).",
            entries.len(),
            if number_mode.number_files { "numbered" } else { "not numbered" },
            number_mode.source
        );
        Ok(Created { json, title, count: entries.len(), generic_title, number_mode })
    }

    /// What registering a new playlist asks YouTube first, wherever its music
    /// will live: its title (generic when it cannot be had) and its videos.
    pub(super) fn look_up_new_playlist(&self, raw_url: &str, format: &str) -> Result<NewPlaylist, CreateError> {
        let settings = settings::load(self.archives.storage(), self.settings_file);
        let cookies = Some(settings.cookies_file).filter(|c| !c.trim().is_empty());
        let cookies = cookies.as_deref();
        // See `Engine::download`: "watch?v=X&list=Y" would be seen as one video.
        let url = paths::normalize_playlist_url(raw_url.trim());

        let output = self.query(query::playlist_title_args(&url, cookies)).map(|(_, l)| l.join("\n")).unwrap_or_default();
        let real_title = query::playlist_title(&output);
        let generic_title = real_title.is_none();
        let title = real_title.unwrap_or_else(|| format!("soundify_playlist_{}", paths::format_extension(format)));

        let (entries, output) = self.fetch_entries(&url, cookies);
        if entries.is_empty() {
            return Err(if query::failed_due_to_cookies(&output) { CreateError::CookiesExpired } else { CreateError::QueryFailed });
        }
        Ok(NewPlaylist { url, title, generic_title, entries })
    }

    /// The video whose thumbnail is the default cover of the playlist typed in
    /// «Crear Playlist», before it is registered. A URL that names a video
    /// ("watch?v=X&list=Y") gives it straight away; a plain playlist one needs
    /// a quick query.
    pub fn first_video_id(&self, raw_url: &str) -> Option<String> {
        if let Some(id) = paths::extract_video_id(raw_url) {
            return Some(id);
        }
        let settings = settings::load(self.archives.storage(), self.settings_file);
        let cookies = Some(settings.cookies_file).filter(|c| !c.trim().is_empty());
        let url = paths::normalize_playlist_url(raw_url.trim());
        let (_, lines) = self.query(query::first_video_id_args(&url, cookies.as_deref()))?;
        query::first_video_id(&lines.join("\n"))
    }

    /// The address of the thumbnail of what the URL names: the playlist's own,
    /// or one video's on any site yt-dlp can read.
    pub fn url_thumbnail(&self, raw_url: &str, playlist: bool) -> Option<String> {
        let settings = settings::load(self.archives.storage(), self.settings_file);
        let cookies = Some(settings.cookies_file).filter(|c| !c.trim().is_empty());
        if playlist {
            let url = paths::normalize_playlist_url(raw_url.trim());
            let (_, lines) = self.query(query::playlist_thumbnail_args(&url, cookies.as_deref()))?;
            query::playlist_thumbnail(&lines.join("\n"))
        } else {
            let (_, lines) = self.query(query::video_thumbnail_args(raw_url.trim(), cookies.as_deref()))?;
            query::video_thumbnail(&lines.join("\n"))
        }
    }
}
