use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use axum::Json;
use axum::Router;
use axum::body::{Body, HttpBody};
use axum::extract::{Query, Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
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
    CastBody, FileExtension, FilterBody, MediaDir, MediaItem, MediaItemsRes, PlayMediaBody,
    PosBody, Screenshot, SkipBody, SubBody, Track, VersionInfo, VolBody,
};
use crate::upgrade::upgrade_gnuplex;
use crate::url::escape_relative_segment;

pub struct AppState {
    pub gnuplex: Arc<Gnuplex>,
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
    let home =
        Router::<SharedState>::new().fallback_service(ServeDir::new(state.gnuplex.static_files()));
    let screenshots = Router::<SharedState>::new()
        .route("/", get(screenshots_index))
        .fallback_service(ServeDir::new(state.gnuplex.screenshots_dir()));

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

fn response(status: StatusCode, content_type: Option<&'static str>, body: Vec<u8>) -> Response {
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    if let Some(content_type) = content_type {
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    }
    response
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

/// Run blocking work (SQLite, the mpv IPC socket, git) on the blocking pool so
/// it cannot stall the async runtime.
async fn run_blocking<F, T>(work: F) -> AppResult<T>
where
    F: FnOnce() -> AppResult<T> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|err| Error::Task(err.to_string()))?
}

async fn with_gnuplex<T, F>(state: &SharedState, work: F) -> AppResult<T>
where
    F: FnOnce(&Gnuplex) -> AppResult<T> + Send + 'static,
    T: Send + 'static,
{
    let gnuplex = Arc::clone(&state.gnuplex);
    run_blocking(move || work(&gnuplex)).await
}

async fn with_mpv<T, F>(state: &SharedState, work: F) -> AppResult<T>
where
    F: FnOnce(&crate::mpv::Mpv) -> AppResult<T> + Send + 'static,
    T: Send + 'static,
{
    let mpv = Arc::clone(state.gnuplex.mpv());
    run_blocking(move || work(&mpv)).await
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
    let location =
        HeaderValue::from_str(location).unwrap_or_else(|_| HeaderValue::from_static("/home"));
    response.headers_mut().insert(header::LOCATION, location);
    response
}

async fn root_redirect() -> Response {
    redirect("/home")
}

async fn screenshots_index(State(state): State<SharedState>) -> AppResult<Response> {
    let body = directory_listing(state.gnuplex.screenshots_dir()).await?;
    Ok(html_response(StatusCode::OK, body))
}

async fn directory_listing(dir: &Path) -> AppResult<String> {
    let mut entries = Vec::new();
    let mut read_dir = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = read_dir.next_entry().await? {
        entries.push(entry);
    }
    entries.sort_by_key(tokio::fs::DirEntry::file_name);

    let mut body = String::from(
        "<!doctype html>\n<meta name=\"viewport\" content=\"width=device-width\">\n<pre>\n",
    );
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry
            .file_type()
            .await
            .is_ok_and(|file_type| file_type.is_dir());
        let display = if is_dir {
            format!("{name}/")
        } else {
            name.clone()
        };
        let href = if is_dir {
            format!("{}/", escape_relative_segment(&name))
        } else {
            escape_relative_segment(&name)
        };
        writeln!(body, "<a href=\"{}\">{}</a>", href, html_escape(&display))
            .expect("writing to a String cannot fail");
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

async fn api_version() -> Json<VersionInfo> {
    Json(VersionInfo {
        version: consts::VERSION.to_string(),
        source_hash: consts::SOURCE_HASH.to_string(),
        platform: consts::PLATFORM.to_string(),
        compiler_version: consts::COMPILER_VERSION.to_string(),
    })
}

async fn api_play(State(state): State<SharedState>) -> AppResult<StatusCode> {
    with_mpv(&state, |mpv| mpv.play()).await?;
    Ok(StatusCode::OK)
}

async fn api_pause(State(state): State<SharedState>) -> AppResult<StatusCode> {
    with_mpv(&state, |mpv| mpv.pause()).await?;
    Ok(StatusCode::OK)
}

async fn api_playpause(State(state): State<SharedState>) -> AppResult<StatusCode> {
    with_mpv(&state, |mpv| mpv.play_pause()).await?;
    Ok(StatusCode::OK)
}

async fn api_skip(State(state): State<SharedState>) -> AppResult<StatusCode> {
    with_mpv(&state, |mpv| mpv.skip()).await?;
    Ok(StatusCode::OK)
}

async fn api_now_playing(
    State(state): State<SharedState>,
) -> AppResult<Json<Option<Vec<MediaItem>>>> {
    let media = with_gnuplex(&state, |gnuplex| gnuplex.get_now_playing()).await?;
    Ok(Json(media))
}

async fn api_play_media(
    State(state): State<SharedState>,
    Json(body): Json<PlayMediaBody>,
) -> AppResult<StatusCode> {
    with_gnuplex(&state, move |gnuplex| {
        gnuplex.play_by_id(body.id, body.play_next, body.play_last)
    })
    .await?;
    Ok(StatusCode::OK)
}

async fn api_cast(
    State(state): State<SharedState>,
    Json(body): Json<CastBody>,
) -> AppResult<StatusCode> {
    with_gnuplex(&state, move |gnuplex| {
        gnuplex.cast(&body.url, body.temp, body.play_next, body.play_last)
    })
    .await?;
    Ok(StatusCode::OK)
}

async fn api_paused(State(state): State<SharedState>) -> AppResult<Json<bool>> {
    let paused = with_mpv(&state, |mpv| mpv.get_paused()).await?;
    Ok(Json(paused))
}

async fn api_get_vol(State(state): State<SharedState>) -> AppResult<Json<i64>> {
    let vol = with_mpv(&state, |mpv| mpv.get_vol()).await?;
    Ok(Json(vol))
}

async fn api_set_vol(
    State(state): State<SharedState>,
    Json(body): Json<VolBody>,
) -> AppResult<StatusCode> {
    with_mpv(&state, move |mpv| mpv.set_vol(body.vol)).await?;
    Ok(StatusCode::OK)
}

async fn api_get_media_dirs(State(state): State<SharedState>) -> AppResult<Json<Vec<MediaDir>>> {
    let dirs = with_gnuplex(&state, |gnuplex| gnuplex.media_dirs()).await?;
    Ok(Json(dirs))
}

async fn api_set_media_dirs(
    State(state): State<SharedState>,
    Json(body): Json<Vec<String>>,
) -> AppResult<StatusCode> {
    with_gnuplex(&state, move |gnuplex| gnuplex.set_media_dirs(&body)).await?;
    Ok(StatusCode::OK)
}

async fn api_get_file_exts(
    State(state): State<SharedState>,
) -> AppResult<Json<Vec<FileExtension>>> {
    let extensions = with_gnuplex(&state, |gnuplex| gnuplex.file_exts()).await?;
    Ok(Json(extensions))
}

async fn api_set_file_exts(
    State(state): State<SharedState>,
    Json(body): Json<Vec<String>>,
) -> AppResult<StatusCode> {
    with_gnuplex(&state, move |gnuplex| gnuplex.set_file_exts(&body)).await?;
    Ok(StatusCode::OK)
}

async fn api_get_pos(State(state): State<SharedState>) -> AppResult<Json<i64>> {
    let position = with_mpv(&state, |mpv| mpv.get_pos()).await?;
    Ok(Json(position))
}

async fn api_time_remaining(State(state): State<SharedState>) -> AppResult<Json<i64>> {
    let remaining = with_mpv(&state, |mpv| mpv.get_time_remaining()).await?;
    Ok(Json(remaining))
}

async fn api_set_pos(
    State(state): State<SharedState>,
    Json(body): Json<PosBody>,
) -> AppResult<StatusCode> {
    with_mpv(&state, move |mpv| mpv.set_pos(body.pos)).await?;
    Ok(StatusCode::OK)
}

async fn api_last_25(State(state): State<SharedState>) -> AppResult<Json<Vec<MediaItem>>> {
    let items = with_gnuplex(&state, |gnuplex| gnuplex.last_25_played()).await?;
    Ok(Json(items))
}

async fn api_media_items(
    State(state): State<SharedState>,
    Query(query): Query<MediaItemsQuery>,
) -> AppResult<Json<MediaItemsRes>> {
    let (items, count) = with_gnuplex(&state, move |gnuplex| {
        gnuplex.media_items(&query.search, query.offset)
    })
    .await?;
    Ok(Json(MediaItemsRes { res: items, count }))
}

async fn api_scan_lib(State(state): State<SharedState>) -> AppResult<StatusCode> {
    with_gnuplex(&state, |gnuplex| gnuplex.scan_lib()).await?;
    Ok(StatusCode::OK)
}

async fn api_get_subs(State(state): State<SharedState>) -> AppResult<Json<Option<Vec<Track>>>> {
    let subs = with_gnuplex(&state, |gnuplex| gnuplex.get_subs()).await?;
    Ok(Json(subs))
}

async fn api_set_subs(
    State(state): State<SharedState>,
    Json(body): Json<SubBody>,
) -> AppResult<StatusCode> {
    with_gnuplex(&state, move |gnuplex| {
        if body.id == -1 {
            gnuplex.set_sub_visibility(body.visible)
        } else {
            gnuplex.set_sub_track(body.id)?;
            gnuplex.set_sub_visibility(true)
        }
    })
    .await?;
    Ok(StatusCode::OK)
}

async fn api_get_sub_delay(State(state): State<SharedState>) -> AppResult<Json<f64>> {
    let delay = with_mpv(&state, |mpv| mpv.get_sub_delay()).await?;
    Ok(Json(delay))
}

async fn api_set_sub_delay(
    State(state): State<SharedState>,
    Json(body): Json<SubBody>,
) -> AppResult<StatusCode> {
    with_mpv(&state, move |mpv| mpv.set_sub_delay(body.delay)).await?;
    Ok(StatusCode::OK)
}

async fn api_sub_seek(
    State(state): State<SharedState>,
    Json(body): Json<SkipBody>,
) -> AppResult<StatusCode> {
    with_mpv(&state, move |mpv| mpv.sub_seek(body.skip)).await?;
    Ok(StatusCode::OK)
}

async fn api_sub_save(State(state): State<SharedState>) -> AppResult<StatusCode> {
    with_gnuplex(&state, |gnuplex| gnuplex.save_sub_delay()).await?;
    Ok(StatusCode::OK)
}

async fn api_upgrade(State(state): State<SharedState>) -> AppResult<StatusCode> {
    let exe = state.gnuplex.exe().to_path_buf();
    if run_blocking(move || upgrade_gnuplex(&exe, false)).await? {
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
    with_mpv(&state, move |mpv| mpv.set_filter(&body.filter)).await?;
    Ok(StatusCode::OK)
}

async fn api_delete_queue_entry(
    State(state): State<SharedState>,
    Json(queue_index): Json<i64>,
) -> AppResult<StatusCode> {
    if queue_index < 0 {
        return Err(Error::BadRequest("negative index passed".to_string()));
    }
    with_mpv(&state, move |mpv| mpv.delete_queue_entry(queue_index)).await?;
    Ok(StatusCode::OK)
}

async fn api_get_screenshots(
    State(state): State<SharedState>,
    Query(query): Query<ScreenshotsQuery>,
) -> AppResult<Json<Vec<Screenshot>>> {
    let limit = if query.limit == 0 { 10 } else { query.limit };
    let screenshots = with_gnuplex(&state, move |gnuplex| {
        gnuplex.list_recent_screenshots(limit)
    })
    .await?;
    Ok(Json(screenshots))
}

async fn api_take_screenshot(State(state): State<SharedState>) -> AppResult<Json<Screenshot>> {
    let screenshot = with_gnuplex(&state, |gnuplex| gnuplex.take_screenshot()).await?;
    Ok(Json(screenshot))
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
