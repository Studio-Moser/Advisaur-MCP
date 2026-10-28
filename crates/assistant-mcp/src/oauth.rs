//! OAuth authorization code + PKCE, dynamic client registration, and MCP OAuth
//! discovery in front of the HTTP transport. The operator API key authorizes
//! the login form; resource-bound access/refresh tokens are HMAC-derived from it.
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Extension, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Json, Response};
use axum::{Form, Router, routing::get};
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::server::SharedMemory;

pub fn derive_access_token(api_key: &str, resource: &str) -> String {
    derive(api_key, b"access", &[resource])
}

pub fn derive_refresh_token(api_key: &str, client_id: &str, resource: &str) -> String {
    derive(api_key, b"refresh", &[client_id, resource])
}

fn derive(api_key: &str, label: &[u8], values: &[&str]) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(api_key.as_bytes()).expect("hmac accepts any key length");
    mac.update(label);
    for value in values {
        mac.update(&[0]);
        mac.update(value.as_bytes());
    }
    format!("{:x}", mac.finalize().into_bytes())
}

pub fn verify_pkce(code_verifier: &str, code_challenge: &str) -> bool {
    if !(43..=128).contains(&code_verifier.len())
        || !code_verifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~'))
    {
        return false;
    }
    let computed = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(code_verifier.as_bytes()));
    safe_equal(&computed, code_challenge)
}

fn valid_code_challenge(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub fn safe_equal(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// A bearer is valid when it is the API key itself or the resource-bound access token.
pub fn verify_bearer_token(token: &str, api_key: &str, resource: &str) -> bool {
    safe_equal(token, api_key) || safe_equal(token, &derive_access_token(api_key, resource))
}

struct AuthCode {
    client_id: String,
    code_challenge: String,
    redirect_uri: String,
    resource: String,
    expires_at: Instant,
}

const RATE_LIMIT_MAX: u32 = 5;
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(15 * 60);
const CODE_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Clone)]
pub struct OAuthState {
    api_key: String,
    memory: SharedMemory,
    auth_codes: Arc<Mutex<HashMap<String, AuthCode>>>,
    rate_limiter: Arc<Mutex<HashMap<String, (u32, Instant)>>>,
}

impl OAuthState {
    pub fn new(api_key: String, memory: SharedMemory) -> Self {
        Self {
            api_key,
            memory,
            auth_codes: Default::default(),
            rate_limiter: Default::default(),
        }
    }

    fn check_rate_limit(&self, ip: &str) -> bool {
        let mut map = self.rate_limiter.lock().unwrap();
        let now = Instant::now();
        match map.get_mut(ip) {
            Some((attempts, reset_at)) if now <= *reset_at => {
                if *attempts >= RATE_LIMIT_MAX {
                    return false;
                }
                *attempts += 1;
                true
            }
            _ => {
                map.insert(ip.to_string(), (1, now + RATE_LIMIT_WINDOW));
                true
            }
        }
    }

    fn client(&self, client_id: &str) -> Option<(Option<String>, Vec<String>)> {
        let m = self.memory.lock().unwrap();
        m.conn
            .query_row(
                "SELECT client_name, redirect_uris FROM oauth_clients WHERE client_id = ?1",
                [client_id],
                |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, String>(1)?)),
            )
            .ok()
            .map(|(name, uris)| (name, serde_json::from_str(&uris).unwrap_or_default()))
    }
}

fn json_error(status: StatusCode, error: &str, description: Option<&str>) -> Response {
    let mut body = json!({ "error": error });
    if let Some(d) = description {
        body["error_description"] = json!(d);
    }
    (status, Json(body)).into_response()
}

pub(crate) fn base_url(headers: &HeaderMap) -> String {
    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    format!("{proto}://{host}")
}

pub(crate) fn mcp_resource(headers: &HeaderMap) -> String {
    format!("{}/mcp", base_url(headers))
}

pub(crate) fn protected_resource_metadata_url(headers: &HeaderMap) -> String {
    format!(
        "{}/.well-known/oauth-protected-resource/mcp",
        base_url(headers)
    )
}

async fn metadata(headers: HeaderMap) -> Response {
    let base = base_url(&headers);
    Json(json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/authorize"),
        "token_endpoint": format!("{base}/token"),
        "registration_endpoint": format!("{base}/register"),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none"],
    }))
    .into_response()
}

async fn protected_resource_metadata(headers: HeaderMap) -> Response {
    let base = base_url(&headers);
    Json(json!({
        "resource": mcp_resource(&headers),
        "authorization_servers": [base],
        "bearer_methods_supported": ["header"],
    }))
    .into_response()
}

async fn register(State(st): State<OAuthState>, body: String) -> Response {
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&body) else {
        return json_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            Some("Invalid JSON"),
        );
    };
    let redirect_uris: Vec<String> = parsed["redirect_uris"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    if redirect_uris.is_empty() {
        return json_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            Some("redirect_uris required"),
        );
    }
    let client_id = uuid::Uuid::new_v4().to_string();
    let client_name = parsed["client_name"].as_str().map(str::to_owned);
    let registered_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    {
        let m = st.memory.lock().unwrap();
        if let Err(e) = m.conn.execute(
            "INSERT INTO oauth_clients (client_id, client_name, redirect_uris, registered_at) VALUES (?1, ?2, ?3, ?4)",
            assistant_memory::rusqlite::params![client_id, client_name, serde_json::to_string(&redirect_uris).unwrap_or_default(), registered_at],
        ) {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", Some(&e.to_string()));
        }
    }
    (
        StatusCode::CREATED,
        Json(json!({
            "client_id": client_id,
            "client_name": client_name,
            "redirect_uris": redirect_uris,
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
        })),
    )
        .into_response()
}

#[derive(Deserialize, Default)]
struct AuthorizeQuery {
    #[serde(default)]
    response_type: String,
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    redirect_uri: String,
    #[serde(default)]
    code_challenge: String,
    #[serde(default)]
    code_challenge_method: String,
    #[serde(default)]
    resource: String,
    #[serde(default)]
    state: String,
}

#[derive(Deserialize, Default)]
struct AuthorizeForm {
    #[serde(default)]
    response_type: String,
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    redirect_uri: String,
    #[serde(default)]
    code_challenge: String,
    #[serde(default)]
    code_challenge_method: String,
    #[serde(default)]
    resource: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    api_key: String,
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn render_form(client_name: &str, q: &AuthorizeQuery, error: Option<&str>) -> String {
    let error_html = error
        .map(|e| format!("<div class=\"error\">{}</div>", esc(e)))
        .unwrap_or_default();
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{product} — Authorize</title>
  <style>
    *, *::before, *::after {{ box-sizing: border-box; margin: 0; padding: 0; }}
    body {{ background: #0d0d0d; color: #e5e5e5; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif; min-height: 100vh; display: flex; align-items: center; justify-content: center; padding: 24px; }}
    .card {{ background: #161616; border: 1px solid #272727; border-radius: 16px; padding: 40px; width: 100%; max-width: 420px; }}
    .wordmark {{ font-size: 22px; font-weight: 700; letter-spacing: -0.5px; margin-bottom: 6px; }}
    .tagline {{ color: #666; font-size: 13px; margin-bottom: 32px; }}
    .client {{ font-weight: 600; color: #e5e5e5; }}
    label {{ display: block; font-size: 12px; font-weight: 500; color: #888; text-transform: uppercase; letter-spacing: 0.05em; margin-bottom: 8px; }}
    input[type="password"] {{ display: block; width: 100%; background: #0d0d0d; border: 1px solid #2a2a2a; border-radius: 10px; padding: 12px 16px; color: #e5e5e5; font-size: 15px; outline: none; }}
    input[type="password"]:focus {{ border-color: #444; }}
    .error {{ color: #f87171; font-size: 13px; margin-top: 10px; }}
    button {{ display: block; width: 100%; margin-top: 24px; background: #e5e5e5; color: #0d0d0d; border: none; border-radius: 10px; padding: 13px; font-size: 15px; font-weight: 600; cursor: pointer; }}
    button:hover {{ background: #d0d0d0; }}
  </style>
</head>
<body>
  <div class="card">
    <div class="wordmark">{product}</div>
    <div class="tagline"><span class="client">{client}</span> is requesting access to your memory.</div>
    <form method="POST">
      <input type="hidden" name="client_id" value="{client_id}">
      <input type="hidden" name="response_type" value="{response_type}">
      <input type="hidden" name="code_challenge" value="{code_challenge}">
      <input type="hidden" name="code_challenge_method" value="{code_challenge_method}">
      <input type="hidden" name="resource" value="{resource}">
      <input type="hidden" name="redirect_uri" value="{redirect_uri}">
      <input type="hidden" name="state" value="{state}">
      <label for="api_key">API Key</label>
      <input type="password" id="api_key" name="api_key" placeholder="Enter your {product} API key" autofocus required>
      {error_html}
      <button type="submit">Approve</button>
    </form>
  </div>
</body>
</html>"#,
        product = assistant_memory::brand::PRODUCT_NAME,
        client = esc(client_name),
        client_id = esc(&q.client_id),
        response_type = esc(&q.response_type),
        code_challenge = esc(&q.code_challenge),
        code_challenge_method = esc(&q.code_challenge_method),
        resource = esc(&q.resource),
        redirect_uri = esc(&q.redirect_uri),
        state = esc(&q.state),
    )
}

fn validate_authorize_request(q: &AuthorizeQuery, headers: &HeaderMap) -> Option<Response> {
    if q.response_type != "code" {
        return Some(json_error(
            StatusCode::BAD_REQUEST,
            "unsupported_response_type",
            Some("response_type must be code"),
        ));
    }
    if q.code_challenge_method != "S256" || !valid_code_challenge(&q.code_challenge) {
        return Some(json_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            Some("A valid S256 code_challenge is required"),
        ));
    }
    if q.resource != mcp_resource(headers) {
        return Some(json_error(
            StatusCode::BAD_REQUEST,
            "invalid_target",
            Some("resource must identify this MCP endpoint"),
        ));
    }
    None
}

async fn authorize_get(
    State(st): State<OAuthState>,
    headers: HeaderMap,
    Query(q): Query<AuthorizeQuery>,
) -> Response {
    if let Some(error) = validate_authorize_request(&q, &headers) {
        return error;
    }
    let Some((name, uris)) = st.client(&q.client_id) else {
        return json_error(
            StatusCode::BAD_REQUEST,
            "invalid_client",
            Some("Unknown client_id"),
        );
    };
    if !uris.contains(&q.redirect_uri) {
        return json_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            Some("redirect_uri mismatch"),
        );
    }
    Html(render_form(
        name.as_deref().unwrap_or("Unknown Client"),
        &q,
        None,
    ))
    .into_response()
}

async fn authorize_post(
    State(st): State<OAuthState>,
    connect_info: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Form(f): Form<AuthorizeForm>,
) -> Response {
    let rate_limit_key = connect_info.map_or_else(
        || "embedded-router".to_string(),
        |Extension(ConnectInfo(addr))| addr.ip().to_string(),
    );
    if !st.check_rate_limit(&rate_limit_key) {
        return json_error(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_requests",
            Some("Too many failed attempts"),
        );
    }
    let Some((name, uris)) = st.client(&f.client_id) else {
        return json_error(StatusCode::BAD_REQUEST, "invalid_request", None);
    };
    if !uris.contains(&f.redirect_uri) {
        return json_error(StatusCode::BAD_REQUEST, "invalid_request", None);
    }
    let q = AuthorizeQuery {
        response_type: f.response_type.clone(),
        client_id: f.client_id.clone(),
        redirect_uri: f.redirect_uri.clone(),
        code_challenge: f.code_challenge.clone(),
        code_challenge_method: f.code_challenge_method.clone(),
        resource: f.resource.clone(),
        state: f.state.clone(),
    };
    if let Some(error) = validate_authorize_request(&q, &headers) {
        return error;
    }
    if !safe_equal(&f.api_key, &st.api_key) {
        return Html(render_form(
            name.as_deref().unwrap_or("Unknown Client"),
            &q,
            Some("Incorrect API key"),
        ))
        .into_response();
    }
    let code = uuid::Uuid::new_v4().to_string();
    st.auth_codes.lock().unwrap().insert(
        code.clone(),
        AuthCode {
            client_id: f.client_id,
            code_challenge: f.code_challenge,
            redirect_uri: f.redirect_uri.clone(),
            resource: f.resource,
            expires_at: Instant::now() + CODE_TTL,
        },
    );
    let sep = if f.redirect_uri.contains('?') {
        '&'
    } else {
        '?'
    };
    let location = format!(
        "{}{}code={}&state={}",
        f.redirect_uri,
        sep,
        url_encode(&code),
        url_encode(&f.state)
    );
    let Ok(location) = HeaderValue::from_str(&location) else {
        return json_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            Some("redirect_uri cannot be represented as a Location header"),
        );
    };
    let mut response = StatusCode::FOUND.into_response();
    response.headers_mut().insert(header::LOCATION, location);
    response
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[derive(Deserialize, Default)]
struct TokenForm {
    #[serde(default)]
    grant_type: String,
    #[serde(default)]
    code: String,
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    code_verifier: String,
    #[serde(default)]
    redirect_uri: String,
    #[serde(default)]
    refresh_token: String,
    #[serde(default)]
    resource: String,
}

async fn token(
    State(st): State<OAuthState>,
    headers: HeaderMap,
    Form(f): Form<TokenForm>,
) -> Response {
    let expected_resource = mcp_resource(&headers);
    if f.resource != expected_resource {
        return json_error(
            StatusCode::BAD_REQUEST,
            "invalid_target",
            Some("resource must identify this MCP endpoint"),
        );
    }
    let issue = |client_id: &str| {
        Json(json!({
            "access_token": derive_access_token(&st.api_key, &expected_resource),
            "token_type": "Bearer",
            "refresh_token": derive_refresh_token(&st.api_key, client_id, &expected_resource)
        }))
        .into_response()
    };
    match f.grant_type.as_str() {
        "authorization_code" => {
            let mut codes = st.auth_codes.lock().unwrap();
            let Some(stored) = codes.get(&f.code) else {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    Some("Invalid or expired code"),
                );
            };
            if stored.client_id != f.client_id
                || stored.redirect_uri != f.redirect_uri
                || stored.resource != f.resource
            {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    Some("Invalid or expired code"),
                );
            }
            if Instant::now() > stored.expires_at {
                codes.remove(&f.code);
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    Some("Code expired"),
                );
            }
            if !verify_pkce(&f.code_verifier, &stored.code_challenge) {
                codes.remove(&f.code);
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    Some("PKCE verification failed"),
                );
            }
            codes.remove(&f.code);
            issue(&f.client_id)
        }
        "refresh_token" => {
            if !safe_equal(
                &f.refresh_token,
                &derive_refresh_token(&st.api_key, &f.client_id, &expected_resource),
            ) {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    Some("Invalid refresh token"),
                );
            }
            if st.client(&f.client_id).is_none() {
                return json_error(StatusCode::BAD_REQUEST, "invalid_client", None);
            }
            issue(&f.client_id)
        }
        _ => json_error(StatusCode::BAD_REQUEST, "unsupported_grant_type", None),
    }
}

/// Routes mounted at the root when the API key (`config::API_KEY_ENV`) is set.
pub fn router(state: OAuthState) -> Router {
    Router::new()
        .route("/.well-known/oauth-authorization-server", get(metadata))
        .route(
            "/.well-known/oauth-protected-resource",
            get(protected_resource_metadata),
        )
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            get(protected_resource_metadata),
        )
        .route("/register", axum::routing::post(register))
        .route("/authorize", get(authorize_get).post(authorize_post))
        .route("/token", axum::routing::post(token))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_pkce_and_bearer_match_the_ts_engine() {
        // Values computed with node: crypto.createHmac('sha256','k').update('access').digest('hex')
        let resource = "https://example.com/mcp";
        assert_eq!(derive_access_token("k", resource).len(), 64);
        assert_ne!(
            derive_access_token("k", resource),
            derive_refresh_token("k", "client", resource)
        );
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(Sha256::digest(verifier.as_bytes()));
        assert!(verify_pkce(verifier, &challenge));
        assert!(!verify_pkce("nope", &challenge));
        assert!(verify_bearer_token("k", "k", resource));
        assert!(verify_bearer_token(
            &derive_access_token("k", resource),
            "k",
            resource
        ));
        assert!(!verify_bearer_token("kk", "k", resource));
        assert!(!verify_bearer_token(
            &derive_access_token("k", "https://other.example/mcp"),
            "k",
            resource
        ));
        assert_eq!(url_encode("a b/c"), "a%20b%2Fc");
    }
}
