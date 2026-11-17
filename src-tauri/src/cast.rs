//! «Escuchar en», the device that RECEIVES: what another device passed plays here, read from that device piece by piece
//! (`core::connect::cast::relay`). The page asks `soundify-cast://` for `media/<videoId>` or `cover/<videoId>` and this
//! adds where and with which ticket: the ticket never reaches the page.
//!
//! Costs nothing while nothing is passed: no thread, no timer. Each request of the player is one short thread that
//! carries at most one piece (`RELAY_PIECE`) and ends.

use std::sync::Arc;

use soundify_core::connect::cast::{self, Inbox, Part};
use tauri::http::{Request, Response, StatusCode};
use tauri::{AppHandle, Manager, Runtime, State, UriSchemeContext, UriSchemeResponder};

pub const SCHEME: &str = "soundify-cast";

/// The pass this device plays now. Managed from the start: the protocol may be asked before Connect is up.
#[derive(Default, Clone)]
pub struct CastInbox(pub Arc<Inbox>);

/// `soundify-cast://localhost/media/<videoId>` (or `http://soundify-cast.localhost/...` on Windows and Android).
pub fn handle<R: Runtime>(context: UriSchemeContext<'_, R>, request: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let app: AppHandle<R> = context.app_handle().clone();
    let path = request.uri().path().replace("%2F", "/").replace("%2f", "/");
    let range = request.headers().get("range").and_then(|v| v.to_str().ok()).map(str::to_string);
    let spawned = std::thread::Builder::new().name("cast-relay".into()).spawn(move || {
        let found = path.trim_start_matches('/').split_once('/').and_then(|(kind, id)| match kind {
            "media" => Some((Part::Media, id.to_string())),
            "cover" => Some((Part::Cover, id.to_string())),
            _ => None,
        });
        let source = app.try_state::<CastInbox>().and_then(|inbox| inbox.0.current());
        let relayed = match (found, source) {
            (Some((part, id)), Some(source)) => cast::relay(&source, part, &id, range.as_deref()),
            _ => cast::Relayed { status: 404, headers: Vec::new(), body: Vec::new() },
        };
        let mut answer = Response::builder().status(StatusCode::from_u16(relayed.status).unwrap_or(StatusCode::BAD_GATEWAY));
        for (name, value) in &relayed.headers {
            answer = answer.header(name.as_str(), value.as_str());
        }
        let answer = answer.header("cache-control", "no-store").body(relayed.body).unwrap_or_else(|_| Response::new(Vec::new()));
        responder.respond(answer);
    });
    if spawned.is_err() {
        log::warn!("Could not start a thread for a cast read");
    }
}

/// This device plays something of its own (or the pass ended here): nothing more is read from the other one.
#[tauri::command]
pub fn connect_cast_clear(state: State<'_, CastInbox>) {
    state.0.clear();
}
