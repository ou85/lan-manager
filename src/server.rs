use crate::{
    auth,
    model::{Device, Inventory, ServicePort, Subnet},
    storage::Store,
};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use include_dir::{Dir, include_dir};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;

static ASSETS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/web/dist");
const SESSION_LIFETIME: Duration = Duration::from_secs(8 * 60 * 60);
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    attempts: Arc<Mutex<VecDeque<Instant>>>,
    password_gate: Arc<Semaphore>,
    secure_cookie: bool,
}
struct Session {
    csrf: String,
    expires: Instant,
}
impl AppState {
    pub fn new(store: Store, secure_cookie: bool) -> Self {
        Self {
            store: Arc::new(store),
            sessions: Default::default(),
            attempts: Default::default(),
            password_gate: Arc::new(Semaphore::new(1)),
            secure_cookie,
        }
    }
}
type ApiResult<T> = Result<T, ApiError>;
pub struct ApiError(StatusCode, String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({"error":self.1}))).into_response()
    }
}
fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!("{e}");
    ApiError(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Could not access the database. Check server logs and try again.".into(),
    )
}
fn bad(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, e.to_string())
}
fn unauthorized() -> ApiError {
    ApiError(StatusCode::UNAUTHORIZED, "Please sign in.".into())
}
fn cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|part| {
            part.trim()
                .strip_prefix("homelab_session=")
                .map(str::to_owned)
        })
}
fn session_info(state: &AppState, headers: &HeaderMap) -> ApiResult<String> {
    let token = cookie(headers).ok_or_else(unauthorized)?;
    let mut sessions = state.sessions.lock().map_err(internal)?;
    sessions.retain(|_, s| s.expires > Instant::now());
    sessions
        .get(&auth::digest(&token))
        .map(|s| s.csrf.clone())
        .ok_or_else(unauthorized)
}
fn same_origin(headers: &HeaderMap) -> bool {
    if headers.get("sec-fetch-site").and_then(|h| h.to_str().ok()) == Some("cross-site") {
        return false;
    }
    if let Some(origin) = headers.get(header::ORIGIN) {
        let Ok(origin) = origin.to_str() else {
            return false;
        };
        let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
            return false;
        };
        return origin == format!("http://{host}") || origin == format!("https://{host}");
    }
    true
}
async fn protect(State(state): State<AppState>, req: Request, next: Next) -> ApiResult<Response> {
    let csrf = session_info(&state, req.headers())?;
    if !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS)
        && (!same_origin(req.headers())
            || req
                .headers()
                .get("x-csrf-token")
                .and_then(|x| x.to_str().ok())
                != Some(csrf.as_str()))
    {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "Invalid request token. Reload the page.".into(),
        ));
    }
    Ok(next.run(req).await)
}
async fn security_headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    h.insert("content-security-policy",HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; font-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}
#[derive(Deserialize)]
struct Login {
    username: String,
    password: String,
}
#[derive(Serialize)]
struct SessionReply {
    username: String,
    csrf: String,
}
fn session_cookie(token: &str, secure: bool) -> String {
    format!(
        "homelab_session={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=28800{}",
        if secure { "; Secure" } else { "" }
    )
}
async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<Login>,
) -> ApiResult<Response> {
    if !same_origin(&headers) {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "Cross-origin login is not allowed.".into(),
        ));
    }
    if input.username.len() > 100 || input.password.len() > 256 {
        return Err(unauthorized());
    }
    {
        let mut attempts = state.attempts.lock().map_err(internal)?;
        attempts.retain(|t| t.elapsed() < Duration::from_secs(60));
        if attempts.len() >= 5 {
            return Err(ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "Too many login attempts. Wait a minute.".into(),
            ));
        }
        attempts.push_back(Instant::now());
    }
    let permit = state
        .password_gate
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "A login is in progress. Try again shortly.".into(),
            )
        })?;
    let store = state.store.clone();
    let username = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let c = store.read()?.credentials;
        let password_ok = auth::verify(&input.password, &c.password_hash);
        Ok::<_, anyhow::Error>((password_ok && input.username == c.username).then_some(c.username))
    })
    .await
    .map_err(internal)?
    .map_err(internal)?
    .ok_or_else(|| {
        ApiError(
            StatusCode::UNAUTHORIZED,
            "Invalid username or password.".into(),
        )
    })?;
    let token = auth::token();
    let csrf = auth::token();
    {
        let mut sessions = state.sessions.lock().map_err(internal)?;
        sessions.retain(|_, s| s.expires > Instant::now());
        if sessions.len() >= 32 {
            sessions.clear();
        }
        sessions.insert(
            auth::digest(&token),
            Session {
                csrf: csrf.clone(),
                expires: Instant::now() + SESSION_LIFETIME,
            },
        );
    }
    let mut response = Json(SessionReply { username, csrf }).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&session_cookie(&token, state.secure_cookie)).map_err(internal)?,
    );
    Ok(response)
}
async fn me(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Json<SessionReply>> {
    let csrf = session_info(&state, &headers)?;
    let store = state.store.clone();
    let username =
        tokio::task::spawn_blocking(move || store.read().map(|s| s.credentials.username))
            .await
            .map_err(internal)?
            .map_err(internal)?;
    Ok(Json(SessionReply { username, csrf }))
}
async fn logout(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    if let Some(token) = cookie(&headers) {
        state
            .sessions
            .lock()
            .map_err(internal)?
            .remove(&auth::digest(&token));
    }
    let mut r = StatusCode::NO_CONTENT.into_response();
    r.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static("homelab_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0"),
    );
    Ok(r)
}
async fn inventory(State(state): State<AppState>) -> ApiResult<Json<Inventory>> {
    let s = tokio::task::spawn_blocking(move || state.store.read())
        .await
        .map_err(internal)?
        .map_err(internal)?;
    Ok(Json(s.into()))
}
async fn save_device(
    State(state): State<AppState>,
    Json(mut d): Json<Device>,
) -> ApiResult<Json<Inventory>> {
    if d.id.is_empty() {
        d.id = uuid::Uuid::new_v4().to_string();
    }
    d.name = d.name.trim().into();
    d.ip = d.ip.trim().into();
    let s = tokio::task::spawn_blocking(move || {
        state.store.update(|s| {
            if let Some(old) = s.devices.iter_mut().find(|x| x.id == d.id) {
                *old = d;
            } else {
                s.devices.push(d);
            }
            Ok(())
        })
    })
    .await
    .map_err(internal)?
    .map_err(bad)?;
    Ok(Json(s.into()))
}
async fn delete_device(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Inventory>> {
    let s = tokio::task::spawn_blocking(move || {
        state.store.update(|s| {
            s.devices.retain(|d| d.id != id);
            for d in &mut s.devices {
                if d.parent == id {
                    d.parent.clear();
                }
            }
            Ok(())
        })
    })
    .await
    .map_err(internal)?
    .map_err(bad)?;
    Ok(Json(s.into()))
}
async fn save_subnet(
    State(state): State<AppState>,
    Json(mut subnet): Json<Subnet>,
) -> ApiResult<Json<Inventory>> {
    if subnet.id.is_empty() {
        subnet.id = uuid::Uuid::new_v4().to_string();
    }
    subnet.name = subnet.name.trim().into();
    subnet.cidr = subnet.cidr.trim().into();
    let s = tokio::task::spawn_blocking(move || {
        state.store.update(|s| {
            if let Some(old) = s.subnets.iter_mut().find(|x| x.id == subnet.id) {
                *old = subnet;
            } else {
                s.subnets.push(subnet);
            }
            Ok(())
        })
    })
    .await
    .map_err(internal)?
    .map_err(bad)?;
    Ok(Json(s.into()))
}
async fn delete_subnet(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Inventory>> {
    let s = tokio::task::spawn_blocking(move || {
        state.store.update(|s| {
            if s.devices.iter().any(|d| d.subnet_id == id) {
                anyhow::bail!("Move assigned devices out of this subnet before deleting it");
            }
            s.subnets.retain(|n| n.id != id);
            Ok(())
        })
    })
    .await
    .map_err(internal)?
    .map_err(bad)?;
    Ok(Json(s.into()))
}
async fn save_port(
    State(state): State<AppState>,
    Json(mut port): Json<ServicePort>,
) -> ApiResult<Json<Inventory>> {
    if port.id.is_empty() {
        port.id = uuid::Uuid::new_v4().to_string();
    }
    port.protocol = port.protocol.trim().to_ascii_uppercase();
    port.host = port.host.trim().into();
    port.service = port.service.trim().into();
    port.access = port.access.trim().into();
    port.url = port.url.trim().into();
    port.status = port.status.trim().into();
    let s = tokio::task::spawn_blocking(move || {
        state.store.update(|s| {
            if let Some(old) = s.ports.iter_mut().find(|x| x.id == port.id) {
                *old = port;
            } else {
                s.ports.push(port);
            }
            Ok(())
        })
    })
    .await
    .map_err(internal)?
    .map_err(bad)?;
    Ok(Json(s.into()))
}
async fn delete_port(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Inventory>> {
    let s = tokio::task::spawn_blocking(move || {
        state.store.update(|s| {
            s.ports.retain(|p| p.id != id);
            Ok(())
        })
    })
    .await
    .map_err(internal)?
    .map_err(bad)?;
    Ok(Json(s.into()))
}
async fn assets(req: Request) -> Response {
    if !matches!(*req.method(), Method::GET | Method::HEAD) {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let path = req.uri().path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let Some(file) = ASSETS.get_file(path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mime = if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if path.ends_with(".js") {
        "text/javascript; charset=utf-8"
    } else if path.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else if path.ends_with(".woff2") {
        "font/woff2"
    } else {
        "application/octet-stream"
    };
    Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, mime)
        .body(if *req.method() == Method::HEAD {
            Body::empty()
        } else {
            Body::from(file.contents())
        })
        .unwrap()
}
pub fn app(state: AppState) -> Router {
    let protected = Router::new()
        .route("/session", get(me))
        .route("/logout", post(logout))
        .route("/inventory", get(inventory))
        .route("/devices", post(save_device))
        .route("/devices/{id}", axum::routing::delete(delete_device))
        .route("/subnets", post(save_subnet))
        .route("/subnets/{id}", axum::routing::delete(delete_subnet))
        .route("/ports", post(save_port))
        .route("/ports/{id}", axum::routing::delete(delete_port))
        .route_layer(middleware::from_fn_with_state(state.clone(), protect));
    Router::new()
        .nest("/api", protected.route("/login", post(login)))
        .route("/healthz", get(|| async { "ok" }))
        .fallback(assets)
        .layer(DefaultBodyLimit::max(32 * 1024))
        .layer(middleware::from_fn(security_headers))
        .with_state(state)
}
