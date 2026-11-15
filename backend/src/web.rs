use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use axum::Json;
use axum::Router;
use axum::body::{Body, HttpBody};
use axum::extract::{Query, Request, State};
use axum::http::{HeaderMap, HeaderName, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use serde::Deserialize;
use tokio::sync::watch;
use tower_http::services::ServeDir;
use tracing::{Level, debug, enabled, error, info};

use crate::consts;
use crate::error::{Error, Result as AppResult};
use crate::gnuplex::Gnuplex;
use crate::models::{
    CastBody, FilterBody, MediaItemsRes, PlayMediaBody, PosBody, SkipBody, SubBody, VersionInfo,
    VolBody,
};
use crate::upgrade::upgrade_gnuplex;

pub struct AppState {
    pub gnuplex: Arc<Gnuplex>,
    pub exe: std::path::PathBuf,
    pub shutdown: watch::Sender<bool>,
}

pub type SharedState = Arc<AppState>;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct MediaItemsQuery {
    search: String,
    offset: i64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ScreenshotsQuery {
    limit: i64,
}

pub fn router(state: SharedState) -> Router {
    let home = Router::<SharedState>::new()
        .fallback_service(ServeDir::new(state.gnuplex.static_files.clone()));
    let screenshots = Router::<SharedState>::new()
        .route("/", get(screenshots_index))
        .fallback_service(ServeDir::new(state.gnuplex.screenshots_dir.clone()));

    Router::new()
        .route("/", get(root_redirect))
        .route("/gnuplex", get(root_redirect))
        .nest("/home", home)
        .nest("/screenshots", screenshots)
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

fn response(status: StatusCode, content_type: Option<&str>, body: Vec<u8>) -> Response {
    let mut builder = Response::builder().status(status);
    if let Some(content_type) = content_type {
        builder = builder.header(header::CONTENT_TYPE, content_type);
    }
    builder.body(Body::from(body)).unwrap()
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match self {
            Error::BadRequest(_) => StatusCode::BAD_REQUEST,
            Error::NotFound(_) => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            error!(error = %self, "request failed");
            return text_response(status, "some problem doing that");
        }
        text_response(status, &self.to_string())
    }
}

async fn not_found(State(_state): State<SharedState>) -> Response {
    text_response(StatusCode::NOT_FOUND, "404 page not found")
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

async fn screenshots_index(State(state): State<SharedState>) -> AppResult<Response> {
    let body = directory_listing(&state.gnuplex.screenshots_dir).await?;
    Ok(html_response(StatusCode::OK, body))
}

async fn directory_listing(dir: &Path) -> AppResult<String> {
    let mut entries = Vec::new();
    let mut read_dir = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = read_dir.next_entry().await? {
        entries.push(entry);
    }
    entries.sort_by_key(|entry| entry.file_name());

    let mut body = String::from(
        "<!doctype html>\n<meta name=\"viewport\" content=\"width=device-width\">\n<pre>\n",
    );
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
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
    Ok(body)
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

async fn api_version() -> Json<VersionInfo> {
    Json(VersionInfo {
        version: consts::VERSION.to_string(),
        source_hash: consts::SOURCE_HASH.to_string(),
        platform: consts::PLATFORM.to_string(),
        go_version: consts::COMPILER_VERSION.to_string(),
    })
}

async fn api_play(State(state): State<SharedState>) -> AppResult<StatusCode> {
    state.gnuplex.mpv.play()?;
    Ok(StatusCode::OK)
}

async fn api_pause(State(state): State<SharedState>) -> AppResult<StatusCode> {
    state.gnuplex.mpv.pause()?;
    Ok(StatusCode::OK)
}

async fn api_playpause(State(state): State<SharedState>) -> AppResult<StatusCode> {
    state.gnuplex.mpv.play_pause()?;
    Ok(StatusCode::OK)
}

async fn api_skip(State(state): State<SharedState>) -> AppResult<StatusCode> {
    state.gnuplex.mpv.skip()?;
    Ok(StatusCode::OK)
}

async fn api_now_playing(
    State(state): State<SharedState>,
) -> AppResult<Json<Option<Vec<crate::models::MediaItem>>>> {
    Ok(Json(state.gnuplex.get_now_playing()?))
}

async fn api_play_media(
    State(state): State<SharedState>,
    Json(body): Json<PlayMediaBody>,
) -> AppResult<StatusCode> {
    state
        .gnuplex
        .play_by_id(body.id, body.play_next, body.play_last)?;
    Ok(StatusCode::OK)
}

async fn api_cast(
    State(state): State<SharedState>,
    Json(body): Json<CastBody>,
) -> AppResult<StatusCode> {
    state
        .gnuplex
        .cast(&body.url, body.temp, body.play_next, body.play_last)?;
    Ok(StatusCode::OK)
}

async fn api_paused(State(state): State<SharedState>) -> AppResult<Json<bool>> {
    Ok(Json(state.gnuplex.mpv.get_paused()?))
}

async fn api_get_vol(State(state): State<SharedState>) -> AppResult<Json<i64>> {
    Ok(Json(state.gnuplex.mpv.get_vol()?))
}

async fn api_set_vol(
    State(state): State<SharedState>,
    Json(body): Json<VolBody>,
) -> AppResult<StatusCode> {
    state.gnuplex.mpv.set_vol(body.vol)?;
    Ok(StatusCode::OK)
}

async fn api_get_media_dirs(
    State(state): State<SharedState>,
) -> AppResult<Json<Vec<crate::models::MediaDir>>> {
    Ok(Json(state.gnuplex.db.get_media_dirs()?))
}

async fn api_set_media_dirs(
    State(state): State<SharedState>,
    Json(body): Json<Vec<String>>,
) -> AppResult<StatusCode> {
    state.gnuplex.db.set_media_dirs(&body)?;
    Ok(StatusCode::OK)
}

async fn api_get_file_exts(
    State(state): State<SharedState>,
) -> AppResult<Json<Vec<crate::models::FileExtension>>> {
    Ok(Json(state.gnuplex.db.get_file_exts()?))
}

async fn api_set_file_exts(
    State(state): State<SharedState>,
    Json(body): Json<Vec<String>>,
) -> AppResult<StatusCode> {
    state.gnuplex.db.set_file_exts(&body)?;
    Ok(StatusCode::OK)
}

async fn api_get_pos(State(state): State<SharedState>) -> AppResult<Json<i64>> {
    Ok(Json(state.gnuplex.mpv.get_pos()?))
}

async fn api_time_remaining(State(state): State<SharedState>) -> AppResult<Json<i64>> {
    Ok(Json(state.gnuplex.mpv.get_time_remaining()?))
}

async fn api_set_pos(
    State(state): State<SharedState>,
    Json(body): Json<PosBody>,
) -> AppResult<StatusCode> {
    state.gnuplex.mpv.set_pos(body.pos)?;
    Ok(StatusCode::OK)
}

async fn api_last_25(
    State(state): State<SharedState>,
) -> AppResult<Json<Vec<crate::models::MediaItem>>> {
    Ok(Json(state.gnuplex.db.get_last_25_played()?))
}

async fn api_media_items(
    State(state): State<SharedState>,
    Query(query): Query<MediaItemsQuery>,
) -> AppResult<Json<MediaItemsRes>> {
    let (items, count) = state
        .gnuplex
        .db
        .get_media_items(&query.search, query.offset)?;
    Ok(Json(MediaItemsRes { res: items, count }))
}

async fn api_scan_lib(State(state): State<SharedState>) -> AppResult<StatusCode> {
    state.gnuplex.scan_lib()?;
    Ok(StatusCode::OK)
}

async fn api_get_subs(
    State(state): State<SharedState>,
) -> AppResult<Json<Option<Vec<crate::models::Track>>>> {
    Ok(Json(state.gnuplex.get_subs()?))
}

async fn api_set_subs(
    State(state): State<SharedState>,
    Json(body): Json<SubBody>,
) -> AppResult<StatusCode> {
    if body.id == -1 {
        state.gnuplex.set_sub_visibility(body.visible)?;
    } else {
        state.gnuplex.set_sub_track(body.id)?;
        state.gnuplex.set_sub_visibility(true)?;
    }
    Ok(StatusCode::OK)
}

async fn api_get_sub_delay(State(state): State<SharedState>) -> AppResult<Json<f64>> {
    Ok(Json(state.gnuplex.mpv.get_sub_delay()?))
}

async fn api_set_sub_delay(
    State(state): State<SharedState>,
    Json(body): Json<SubBody>,
) -> AppResult<StatusCode> {
    state.gnuplex.mpv.set_sub_delay(body.delay)?;
    Ok(StatusCode::OK)
}

async fn api_sub_seek(
    State(state): State<SharedState>,
    Json(body): Json<SkipBody>,
) -> AppResult<StatusCode> {
    state.gnuplex.mpv.sub_seek(body.skip)?;
    Ok(StatusCode::OK)
}

async fn api_sub_save(State(state): State<SharedState>) -> AppResult<StatusCode> {
    state.gnuplex.save_sub_delay()?;
    Ok(StatusCode::OK)
}

async fn api_upgrade(State(state): State<SharedState>) -> AppResult<StatusCode> {
    if upgrade_gnuplex(&state.exe, false)? {
        info!("Upgraded and going into shutdown");
        let _ = state.shutdown.send(true);
    } else {
        info!("Already up to date");
    }
    Ok(StatusCode::OK)
}

async fn api_filter(
    State(state): State<SharedState>,
    Json(body): Json<FilterBody>,
) -> AppResult<StatusCode> {
    state.gnuplex.mpv.set_filter(&body.filter)?;
    Ok(StatusCode::OK)
}

async fn api_delete_queue_entry(
    State(state): State<SharedState>,
    Json(queue_index): Json<i64>,
) -> AppResult<StatusCode> {
    if queue_index < 0 {
        return Err(Error::BadRequest("negative index passed".to_string()));
    }
    state.gnuplex.mpv.delete_queue_entry(queue_index)?;
    Ok(StatusCode::OK)
}

async fn api_get_screenshots(
    State(state): State<SharedState>,
    Query(query): Query<ScreenshotsQuery>,
) -> AppResult<Json<Vec<crate::models::Screenshot>>> {
    let limit = if query.limit == 0 { 10 } else { query.limit };
    Ok(Json(state.gnuplex.list_recent_screenshots(limit)?))
}

async fn api_take_screenshot(
    State(state): State<SharedState>,
) -> AppResult<Json<crate::models::Screenshot>> {
    Ok(Json(state.gnuplex.take_screenshot()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_static_services_do_not_conflict() {
        let _router: Router = Router::new()
            .route("/", get(root_redirect))
            .route("/gnuplex", get(root_redirect))
            .nest(
                "/home",
                Router::new().fallback_service(ServeDir::new("static")),
            )
            .nest(
                "/screenshots",
                Router::new()
                    .route("/", get(|| async { "" }))
                    .fallback_service(ServeDir::new("static")),
            );
    }
}
