use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::body::{Body, Bytes, HttpBody};
use axum::extract::{Path as AxumPath, RawQuery, Request, State};
use axum::http::{HeaderMap, HeaderName, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{delete, get, post};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::watch;
use tracing::{Level, debug, enabled, info};

use crate::consts;
use crate::gnuplex::Gnuplex;
use crate::gotime::{go_float_string, to_go_json};
use crate::models::{
    CastBody, FilterBody, MediaItemsRes, PlayMediaBody, PosBody, SkipBody, SubBody, VersionInfo,
    VolBody,
};
use crate::upgrade::upgrade_gnuplex;

pub struct AppState {
    pub gnuplex: Arc<Gnuplex>,
    pub exe: String,
    pub shutdown: watch::Sender<bool>,
}

pub type SharedState = Arc<AppState>;

pub fn router(state: SharedState) -> Router {
    Router::new()
        .route("/", get(root_redirect))
        .route("/gnuplex", get(root_redirect))
        .route("/home", get(home_redirect))
        .route("/home/", get(home_index))
        .route("/home/{*path}", get(home_file))
        .route("/screenshots", get(screenshots_redirect))
        .route("/screenshots/", get(screenshots_index))
        .route("/screenshots/{*path}", get(screenshots_file))
        .route("/api/version", get(api_version))
        .route("/api/play", post(api_play))
        .route("/api/pause", post(api_pause))
        .route("/api/playpause", post(api_playpause))
        .route("/api/skip", post(api_skip))
        .route("/api/nowplaying", get(api_now_playing))
        .route("/api/playmedia", post(api_play_media))
        .route("/api/cast", post(api_cast))
        .route("/api/paused", get(api_paused))
        .route("/api/vol", get(api_get_vol).post(api_set_vol))
        .route(
            "/api/mediadirs",
            get(api_get_media_dirs).post(api_set_media_dirs),
        )
        .route(
            "/api/file_exts",
            get(api_get_file_exts).post(api_set_file_exts),
        )
        .route("/api/pos", get(api_get_pos).post(api_set_pos))
        .route("/api/timeremaining", get(api_time_remaining))
        .route("/api/last25", get(api_last_25))
        .route("/api/mediaitems", get(api_media_items))
        .route("/api/scanlib", post(api_scan_lib))
        .route("/api/sub", get(api_get_subs).post(api_set_subs))
        .route(
            "/api/sub_delay",
            get(api_get_sub_delay).post(api_set_sub_delay),
        )
        .route("/api/sub_seek", post(api_sub_seek))
        .route("/api/sub_save", post(api_sub_save))
        .route("/api/upgrade", post(api_upgrade))
        .route("/api/filter", post(api_filter))
        .route("/api/queue_entry", delete(api_delete_queue_entry))
        .route(
            "/api/screenshots",
            get(api_get_screenshots).post(api_take_screenshot),
        )
        .fallback(not_found)
        .method_not_allowed_fallback(not_found)
        .layer(middleware::from_fn(log_request))
        .with_state(state)
}

/// Log every request at INFO with its method, path, query string, status and
/// duration. At DEBUG, also include request headers and the response size.
async fn log_request(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let version = request.version();
    let headers = if enabled!(Level::DEBUG) {
        Some(request.headers().clone())
    } else {
        None
    };
    let start = Instant::now();
    let response = next.run(request).await;
    let elapsed = start.elapsed();
    let status = response.status();

    info!(
        method = %method,
        path = %uri.path(),
        query = uri.query().unwrap_or_default(),
        status = status.as_u16(),
        duration = ?elapsed,
        "request"
    );

    if let Some(headers) = headers {
        debug!(
            method = %method,
            path = %uri.path(),
            query = uri.query().unwrap_or_default(),
            http_version = ?version,
            user_agent = header_value(&headers, header::USER_AGENT),
            referer = header_value(&headers, header::REFERER),
            content_type = header_value(&headers, header::CONTENT_TYPE),
            content_length = header_value(&headers, header::CONTENT_LENGTH),
            response_size = response.body().size_hint().exact().unwrap_or_default(),
            "request details"
        );
    }

    response
}

fn header_value(headers: &HeaderMap, name: HeaderName) -> &str {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
}

fn json_response<T: Serialize + ?Sized>(status: StatusCode, value: &T) -> Response {
    match to_go_json(value) {
        Ok(body) => response(status, Some("application/json; charset=utf-8"), body),
        Err(_) => empty_response(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

fn text_response(status: StatusCode, text: &str) -> Response {
    response(
        status,
        Some("text/plain; charset=utf-8"),
        text.as_bytes().to_vec(),
    )
}

fn html_response(status: StatusCode, html: String) -> Response {
    response(status, Some("text/html; charset=utf-8"), html.into_bytes())
}

fn empty_response(status: StatusCode) -> Response {
    response(status, None, Vec::new())
}

fn response(status: StatusCode, content_type: Option<&str>, body: Vec<u8>) -> Response {
    let mut builder = Response::builder().status(status);
    if let Some(content_type) = content_type {
        builder = builder.header(header::CONTENT_TYPE, content_type);
    }
    builder.body(Body::from(body)).unwrap()
}

fn json_internal_error() -> Response {
    json_response(StatusCode::INTERNAL_SERVER_ERROR, &0)
}

fn json_zero_internal_error() -> Response {
    json_response(StatusCode::INTERNAL_SERVER_ERROR, &0)
}

async fn not_found(State(_state): State<SharedState>) -> Response {
    response(
        StatusCode::NOT_FOUND,
        Some("text/plain"),
        b"404 page not found".to_vec(),
    )
}

fn redirect(location: &str) -> Response {
    let body = format!(
        "<a href=\"{}\">Moved Permanently</a>.\n\n",
        html_escape(location)
    );
    let mut response = html_response(StatusCode::MOVED_PERMANENTLY, body);
    response.headers_mut().insert(
        header::LOCATION,
        location
            .parse()
            .unwrap_or_else(|_| "/home".parse().unwrap()),
    );
    response
}

async fn root_redirect() -> Response {
    redirect("/home")
}

async fn home_redirect() -> Response {
    redirect("/home/")
}

async fn screenshots_redirect() -> Response {
    redirect("/screenshots/")
}

async fn home_index(State(state): State<SharedState>) -> Response {
    serve_static(&state, "", false)
}

async fn home_file(State(state): State<SharedState>, AxumPath(path): AxumPath<String>) -> Response {
    serve_static(&state, &path, false)
}

async fn screenshots_index(State(state): State<SharedState>) -> Response {
    serve_static_screenshots(&state, "")
}

async fn screenshots_file(
    State(state): State<SharedState>,
    AxumPath(path): AxumPath<String>,
) -> Response {
    serve_static_screenshots(&state, &path)
}

fn serve_static(state: &SharedState, relative: &str, list_dirs: bool) -> Response {
    serve_dir(&state.gnuplex.static_files, relative, list_dirs)
}

fn serve_static_screenshots(state: &SharedState, relative: &str) -> Response {
    serve_dir(&state.gnuplex.screenshots_dir, relative, true)
}

fn serve_dir(root: &Path, relative: &str, list_dirs: bool) -> Response {
    let relative = percent_decode(relative, false);
    if relative
        .split('/')
        .any(|component| component == ".." || component == "." && relative != ".")
    {
        return empty_response(StatusCode::NOT_FOUND);
    }
    let path = root.join(relative.trim_start_matches('/'));
    let metadata = match std::fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(_) => return empty_response(StatusCode::NOT_FOUND),
    };
    if metadata.is_dir() {
        if !relative.is_empty() && !relative.ends_with('/') {
            // This is net/http's localRedirect: a relative Location header and
            // no response body.
            let base = relative.rsplit('/').next().unwrap_or("");
            let mut response = empty_response(StatusCode::MOVED_PERMANENTLY);
            response.headers_mut().insert(
                header::LOCATION,
                format!("{base}/")
                    .parse()
                    .unwrap_or_else(|_| "/".parse().unwrap()),
            );
            return response;
        }
        let index = path.join("index.html");
        if let Ok(index_metadata) = std::fs::metadata(&index)
            && index_metadata.is_file()
        {
            return serve_file(&index, index_metadata);
        }
        let status = if list_dirs {
            StatusCode::OK
        } else {
            StatusCode::NOT_FOUND
        };
        let mut response = html_response(status, directory_listing(&path, list_dirs));
        if let Ok(modified) = metadata.modified() {
            response.headers_mut().insert(
                header::LAST_MODIFIED,
                http_date(modified.into()).parse().unwrap(),
            );
        }
        return response;
    }
    serve_file(&path, metadata)
}

fn serve_file(path: &Path, metadata: std::fs::Metadata) -> Response {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return empty_response(StatusCode::NOT_FOUND),
    };
    let content_type = mime_type(path);
    let mut response = response(StatusCode::OK, Some(content_type), bytes);
    if let Ok(modified) = metadata.modified() {
        response.headers_mut().insert(
            header::LAST_MODIFIED,
            http_date(modified.into()).parse().unwrap(),
        );
    }
    response
        .headers_mut()
        .insert(header::ACCEPT_RANGES, "bytes".parse().unwrap());
    response
}

fn directory_listing(dir: &Path, list_dirs: bool) -> String {
    let mut entries: Vec<_> = if list_dirs {
        match std::fs::read_dir(dir) {
            Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
            Err(_) => Vec::new(),
        }
    } else {
        // gin.Static wraps the filesystem with OnlyFilesFS, whose Readdir
        // always returns no entries.
        Vec::new()
    };
    entries.sort_by_key(|a| a.file_name());
    let mut body = String::from(
        "<!doctype html>\n<meta name=\"viewport\" content=\"width=device-width\">\n<pre>\n",
    );
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let display = if is_dir {
            format!("{name}/")
        } else {
            name.clone()
        };
        let href = if is_dir {
            format!("{}/", path_escape_component(&name))
        } else {
            path_escape_component(&name)
        };
        body.push_str(&format!(
            "<a href=\"{}\">{}</a>\n",
            href,
            html_escape(&display)
        ));
    }
    body.push_str("</pre>\n");
    body
}

fn mime_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/vnd.microsoft.icon",
        "ogg" => "audio/ogg",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "txt" => "text/plain; charset=utf-8",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

fn http_date(dt: DateTime<Utc>) -> String {
    dt.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

fn html_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&#34;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Escape a single URL path segment the way net/url.PathEscape does.
fn path_escape_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    // url.URL.String prefixes a relative path with "./" when its first segment
    // contains a colon, so it cannot be mistaken for a scheme.
    if value.contains(':') {
        out.push_str("./");
    }
    for byte in value.as_bytes() {
        let b = *byte;
        let allowed = b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'-' | b'.' | b'_' | b'~' | b'$' | b'&' | b'+' | b',' | b';' | b':' | b'=' | b'@'
            );
        if allowed {
            out.push(b as char);
        } else {
            out.push('%');
            out.push_str(&format!("{b:02X}"));
        }
    }
    out
}

fn percent_decode(value: &str, plus_as_space: bool) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    out.push((hi * 16 + lo) as u8);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b'+' if plus_as_space => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

fn query_param(raw: &RawQuery, key: &str) -> Option<String> {
    let raw = raw.0.as_deref()?;
    for pair in raw.split('&') {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        if percent_decode(name, true) == key {
            return Some(percent_decode(value, true));
        }
    }
    None
}

fn parse_body<T: DeserializeOwned + Default>(body: &Bytes) -> Result<T, ()> {
    let value: Value = serde_json::from_slice(body).map_err(|_| ())?;
    if value.is_null() {
        return Ok(T::default());
    }
    serde_json::from_value(value).map_err(|_| ())
}

/// Go's encoding/json refuses to unmarshal a JSON array into a struct, while
/// serde happily treats one as a sequence of struct fields. Request bodies
/// destined for Go structs must therefore be objects (or null).
fn parse_object_body<T: DeserializeOwned + Default>(body: &Bytes) -> Result<T, ()> {
    let value: Value = serde_json::from_slice(body).map_err(|_| ())?;
    if value.is_null() {
        return Ok(T::default());
    }
    if !value.is_object() {
        return Err(());
    }
    serde_json::from_value(value).map_err(|_| ())
}

async fn api_version() -> Response {
    json_response(
        StatusCode::OK,
        &VersionInfo {
            version: consts::VERSION.to_string(),
            source_hash: consts::SOURCE_HASH.to_string(),
            platform: consts::PLATFORM.to_string(),
            go_version: consts::COMPILER_VERSION.to_string(),
        },
    )
}

async fn api_play(State(state): State<SharedState>) -> Response {
    match state.gnuplex.mpv.play() {
        Ok(()) => empty_response(StatusCode::OK),
        Err(_) => text_response(StatusCode::INTERNAL_SERVER_ERROR, "some problem doing that"),
    }
}

async fn api_pause(State(state): State<SharedState>) -> Response {
    match state.gnuplex.mpv.pause() {
        Ok(()) => empty_response(StatusCode::OK),
        Err(_) => text_response(StatusCode::INTERNAL_SERVER_ERROR, "some problem doing that"),
    }
}

async fn api_playpause(State(state): State<SharedState>) -> Response {
    match state.gnuplex.mpv.play_pause() {
        Ok(()) => empty_response(StatusCode::OK),
        Err(_) => text_response(StatusCode::INTERNAL_SERVER_ERROR, "some problem doing that"),
    }
}

async fn api_skip(State(state): State<SharedState>) -> Response {
    match state.gnuplex.mpv.skip() {
        Ok(()) => empty_response(StatusCode::OK),
        Err(_) => text_response(StatusCode::INTERNAL_SERVER_ERROR, "some problem doing that"),
    }
}

async fn api_now_playing(State(state): State<SharedState>) -> Response {
    match state.gnuplex.get_now_playing() {
        Ok(media) => json_response(StatusCode::OK, &media),
        Err(_) => json_response(StatusCode::INTERNAL_SERVER_ERROR, &""),
    }
}

async fn api_play_media(State(state): State<SharedState>, body: Bytes) -> Response {
    let body: PlayMediaBody = match parse_object_body(&body) {
        Ok(body) => body,
        Err(()) => return text_response(StatusCode::BAD_REQUEST, "bad body format"),
    };
    match state
        .gnuplex
        .play_by_id(body.id, body.play_next, body.play_last)
    {
        Ok(()) => empty_response(StatusCode::OK),
        Err(_) => text_response(StatusCode::INTERNAL_SERVER_ERROR, "some problem doing that"),
    }
}

async fn api_cast(State(state): State<SharedState>, body: Bytes) -> Response {
    let body: CastBody = match parse_object_body(&body) {
        Ok(body) => body,
        Err(()) => return text_response(StatusCode::BAD_REQUEST, "bad body format"),
    };
    match state
        .gnuplex
        .cast(&body.url, body.temp, body.play_next, body.play_last)
    {
        Ok(()) => empty_response(StatusCode::OK),
        Err(_) => text_response(StatusCode::INTERNAL_SERVER_ERROR, "some problem doing that"),
    }
}

async fn api_paused(State(state): State<SharedState>) -> Response {
    match state.gnuplex.mpv.get_paused() {
        Ok(paused) => json_response(StatusCode::OK, &paused),
        Err(_) => json_response(StatusCode::INTERNAL_SERVER_ERROR, &0),
    }
}

async fn api_get_vol(State(state): State<SharedState>) -> Response {
    match state.gnuplex.mpv.get_vol() {
        Ok(vol) => json_response(StatusCode::OK, &vol),
        Err(_) => json_internal_error(),
    }
}

async fn api_set_vol(State(state): State<SharedState>, body: Bytes) -> Response {
    let body: VolBody = match parse_object_body(&body) {
        Ok(body) => body,
        Err(()) => return text_response(StatusCode::BAD_REQUEST, "bad body format"),
    };
    match state.gnuplex.mpv.set_vol(body.vol) {
        Ok(()) => json_response(StatusCode::OK, &true),
        Err(_) => text_response(StatusCode::INTERNAL_SERVER_ERROR, "some problem doing that"),
    }
}

async fn api_get_media_dirs(State(state): State<SharedState>) -> Response {
    match state.gnuplex.db.get_media_dirs() {
        Ok(dirs) => json_response(StatusCode::OK, &dirs),
        Err(_) => empty_response(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn api_set_media_dirs(State(state): State<SharedState>, body: Bytes) -> Response {
    let body: Vec<String> = match parse_body(&body) {
        Ok(body) => body,
        Err(()) => return text_response(StatusCode::BAD_REQUEST, "bad body format"),
    };
    match state.gnuplex.db.set_media_dirs(&body) {
        Ok(()) => empty_response(StatusCode::OK),
        Err(_) => text_response(StatusCode::INTERNAL_SERVER_ERROR, "some problem doing that"),
    }
}

async fn api_get_file_exts(State(state): State<SharedState>) -> Response {
    match state.gnuplex.db.get_file_exts() {
        Ok(exts) => json_response(StatusCode::OK, &exts),
        Err(_) => empty_response(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn api_set_file_exts(State(state): State<SharedState>, body: Bytes) -> Response {
    let body: Vec<String> = match parse_body(&body) {
        Ok(body) => body,
        Err(()) => return text_response(StatusCode::BAD_REQUEST, "bad body format"),
    };
    match state.gnuplex.db.set_file_exts(&body) {
        Ok(()) => empty_response(StatusCode::OK),
        Err(_) => text_response(StatusCode::INTERNAL_SERVER_ERROR, "some problem doing that"),
    }
}

async fn api_get_pos(State(state): State<SharedState>) -> Response {
    match state.gnuplex.mpv.get_pos() {
        Ok(pos) => json_response(StatusCode::OK, &pos),
        Err(_) => json_zero_internal_error(),
    }
}

async fn api_time_remaining(State(state): State<SharedState>) -> Response {
    match state.gnuplex.mpv.get_time_remaining() {
        Ok(remaining) => json_response(StatusCode::OK, &remaining),
        Err(_) => json_zero_internal_error(),
    }
}

async fn api_set_pos(State(state): State<SharedState>, body: Bytes) -> Response {
    let body: PosBody = match parse_object_body(&body) {
        Ok(body) => body,
        Err(()) => return text_response(StatusCode::BAD_REQUEST, "bad body format"),
    };
    match state.gnuplex.mpv.set_pos(body.pos) {
        Ok(()) => empty_response(StatusCode::OK),
        Err(_) => text_response(StatusCode::INTERNAL_SERVER_ERROR, "some problem doing that"),
    }
}

async fn api_last_25(State(state): State<SharedState>) -> Response {
    match state.gnuplex.db.get_last_25_played() {
        Ok(items) => json_response(StatusCode::OK, &items),
        Err(_) => empty_response(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn api_media_items(State(state): State<SharedState>, raw: RawQuery) -> Response {
    let search = query_param(&raw, "search").unwrap_or_default();
    let offset_str = query_param(&raw, "offset").unwrap_or_default();
    let offset = if offset_str.is_empty() {
        0
    } else {
        match offset_str.parse::<i64>() {
            Ok(offset) => offset,
            Err(_) => return empty_response(StatusCode::INTERNAL_SERVER_ERROR),
        }
    };
    match state.gnuplex.db.get_media_items(&search, offset) {
        Ok((items, count)) => json_response(StatusCode::OK, &MediaItemsRes { res: items, count }),
        Err(_) => empty_response(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn api_scan_lib(State(state): State<SharedState>) -> Response {
    match state.gnuplex.scan_lib() {
        Ok(()) => empty_response(StatusCode::OK),
        Err(_) => empty_response(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn api_get_subs(State(state): State<SharedState>) -> Response {
    match state.gnuplex.get_subs() {
        Ok(subs) => json_response(StatusCode::OK, &subs),
        Err(_) => empty_response(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn api_set_subs(State(state): State<SharedState>, body: Bytes) -> Response {
    let body: SubBody = match parse_object_body(&body) {
        Ok(body) => body,
        Err(()) => return text_response(StatusCode::BAD_REQUEST, "bad body format"),
    };
    if body.id == -1 {
        match state.gnuplex.set_sub_visibility(body.visible) {
            Ok(()) => empty_response(StatusCode::OK),
            Err(_) => text_response(StatusCode::INTERNAL_SERVER_ERROR, "some problem doing that"),
        }
    } else {
        if state.gnuplex.set_sub_track(body.id).is_err() {
            return text_response(StatusCode::INTERNAL_SERVER_ERROR, "some problem doing that");
        }
        match state.gnuplex.set_sub_visibility(true) {
            Ok(()) => empty_response(StatusCode::OK),
            Err(_) => text_response(StatusCode::INTERNAL_SERVER_ERROR, "some problem doing that"),
        }
    }
}

async fn api_get_sub_delay(State(state): State<SharedState>) -> Response {
    match state.gnuplex.mpv.get_sub_delay() {
        Ok(delay) => response(
            StatusCode::OK,
            Some("application/json; charset=utf-8"),
            go_float_string(delay).into_bytes(),
        ),
        Err(_) => json_zero_internal_error(),
    }
}

async fn api_set_sub_delay(State(state): State<SharedState>, body: Bytes) -> Response {
    let body: SubBody = match parse_object_body(&body) {
        Ok(body) => body,
        Err(()) => return text_response(StatusCode::BAD_REQUEST, "bad body format"),
    };
    match state.gnuplex.mpv.set_sub_delay(body.delay) {
        Ok(()) => empty_response(StatusCode::OK),
        Err(_) => text_response(StatusCode::INTERNAL_SERVER_ERROR, "some problem doing that"),
    }
}

async fn api_sub_seek(State(state): State<SharedState>, body: Bytes) -> Response {
    let body: SkipBody = match parse_object_body(&body) {
        Ok(body) => body,
        Err(()) => return text_response(StatusCode::BAD_REQUEST, "bad body format"),
    };
    match state.gnuplex.mpv.sub_seek(body.skip) {
        Ok(()) => empty_response(StatusCode::OK),
        Err(_) => text_response(StatusCode::INTERNAL_SERVER_ERROR, "some problem doing that"),
    }
}

async fn api_sub_save(State(state): State<SharedState>) -> Response {
    match state.gnuplex.save_sub_delay() {
        Ok(()) => empty_response(StatusCode::OK),
        Err(_) => text_response(StatusCode::INTERNAL_SERVER_ERROR, "some problem doing that"),
    }
}

async fn api_upgrade(State(state): State<SharedState>) -> Response {
    match upgrade_gnuplex(&state.exe, false) {
        Ok(upgraded) => {
            if upgraded {
                info!("Upgraded and going into shutdown");
                let _ = state.shutdown.send(true);
            } else {
                info!("Already up to date");
            }
            empty_response(StatusCode::OK)
        }
        Err(_) => text_response(StatusCode::INTERNAL_SERVER_ERROR, "some problem doing that"),
    }
}

async fn api_filter(State(state): State<SharedState>, body: Bytes) -> Response {
    let body: FilterBody = match parse_object_body(&body) {
        Ok(body) => body,
        Err(()) => return text_response(StatusCode::BAD_REQUEST, "bad body format"),
    };
    match state.gnuplex.mpv.set_filter(&body.filter) {
        Ok(()) => empty_response(StatusCode::OK),
        Err(_) => text_response(StatusCode::INTERNAL_SERVER_ERROR, "some problem doing that"),
    }
}

async fn api_delete_queue_entry(State(state): State<SharedState>, body: Bytes) -> Response {
    let queue_index: i64 = match parse_body(&body) {
        Ok(index) => index,
        Err(()) => return text_response(StatusCode::BAD_REQUEST, "bad body format"),
    };
    if queue_index < 0 {
        return text_response(StatusCode::BAD_REQUEST, "negative index passed");
    }
    match state.gnuplex.mpv.delete_queue_entry(queue_index) {
        Ok(()) => empty_response(StatusCode::OK),
        Err(_) => text_response(StatusCode::INTERNAL_SERVER_ERROR, "some problem doing that"),
    }
}

async fn api_get_screenshots(State(state): State<SharedState>, raw: RawQuery) -> Response {
    let limit_str = query_param(&raw, "limit").unwrap_or_default();
    let limit = if limit_str.is_empty() {
        10
    } else {
        match limit_str.parse::<i64>() {
            Ok(limit) => limit,
            Err(_) => return text_response(StatusCode::BAD_REQUEST, "bad limit"),
        }
    };
    match state.gnuplex.list_recent_screenshots(limit) {
        Ok(screenshots) => json_response(StatusCode::OK, &screenshots),
        Err(_) => text_response(StatusCode::INTERNAL_SERVER_ERROR, "some problem doing that"),
    }
}

async fn api_take_screenshot(State(state): State<SharedState>) -> Response {
    match state.gnuplex.take_screenshot() {
        Ok(screenshot) => json_response(StatusCode::OK, &screenshot),
        Err(_) => text_response(StatusCode::INTERNAL_SERVER_ERROR, "some problem doing that"),
    }
}
