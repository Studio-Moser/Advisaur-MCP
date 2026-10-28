//! Streamable HTTP transport with discovery, health, bearer authentication, and OAuth.
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{any, get};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService,
    session::{SessionManager, local::LocalSessionManager},
};
use serde_json::json;

use crate::VERSION;
use crate::config::ServeConfig;
use crate::server::{MemoryServer, SharedMemory};

fn discovery() -> serde_json::Value {
    json!({
        "name": "shelbymcp",
        "version": VERSION,
        "description": "Knowledge-graph memory server for AI tools via MCP",
        "transport": "streamable-http",
        "endpoint": "/mcp",
        "capabilities": { "tools": 12, "prompts": 3, "resources": 1, "logging": true, "completions": true }
    })
}

async fn oauth_not_configured() -> Response {
    (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": "oauth_not_configured", "error_description": "Set SHELBY_API_KEY to enable OAuth" }))).into_response()
}

#[derive(Clone)]
struct Auth {
    api_key: Option<String>,
}

async fn bearer_guard(State(auth): State<Auth>, req: Request<Body>, next: Next) -> Response {
    if let Some(key) = &auth.api_key {
        let resource = crate::oauth::mcp_resource(req.headers());
        let token = req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");
        if !crate::oauth::verify_bearer_token(token, key, &resource) {
            let mut response = (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "jsonrpc": "2.0", "error": { "code": -32001, "message": "Unauthorized" }, "id": null })),
            )
                .into_response();
            let challenge = format!(
                "Bearer resource_metadata=\"{}\"",
                crate::oauth::protected_resource_metadata_url(req.headers())
            );
            if let Ok(value) = challenge.parse() {
                response
                    .headers_mut()
                    .insert(header::WWW_AUTHENTICATE, value);
            }
            return response;
        }
    }
    next.run(req).await
}

/// A transport shutdown handle, not proof that every detached rmcp task has exited.
/// Keep this handle on closure failure for Retry. After closing sessions, callers
/// must stop/join their HTTP server, drop routers/control, and obtain actual unique
/// memory ownership before closing or replacing the database.
#[derive(Clone)]
pub struct HttpShutdown {
    // Retain the existing config token without a new direct tokio-util dependency.
    config: StreamableHttpServerConfig,
    sessions: Arc<LocalSessionManager>,
    admission: Arc<tokio::sync::RwLock<()>>,
}
impl HttpShutdown {
    /// Stop new request admission and cancel rmcp response streams.
    pub fn request_shutdown(&self) {
        self.config.cancellation_token.cancel();
    }

    /// Drain admitted HTTP request setup and close the actual session registry.
    /// rmcp enqueues each transport Close; its detached service tasks may still
    /// be finishing. Success here does not certify release of SharedMemory.
    pub async fn close_sessions(&self) -> std::io::Result<()> {
        self.request_shutdown();
        let _exclusive = self.admission.write().await;
        let ids: Vec<_> = self
            .sessions
            .sessions
            .read()
            .await
            .keys()
            .cloned()
            .collect();
        for id in ids {
            self.sessions
                .close_session(&id)
                .await
                .map_err(|_| std::io::Error::other("MCP session closure failed"))?;
        }
        Ok(())
    }
}

async fn shutdown_guard(
    State(shutdown): State<HttpShutdown>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if shutdown.config.cancellation_token.is_cancelled() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let _admitted = shutdown.admission.read().await;
    if shutdown.config.cancellation_token.is_cancelled() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    // Hold admission through response creation: a pre-shutdown initialize must
    // finish registering its session before close_sessions snapshots the registry.
    next.run(req).await
}

pub fn router(memory: SharedMemory, cfg: &ServeConfig) -> axum::Router {
    managed_router(memory, cfg).0
}

/// Build the existing routes with explicit, caller-controlled transport shutdown.
/// Dropping the handle alone does not cancel a router built through `router`.
pub fn managed_router(memory: SharedMemory, cfg: &ServeConfig) -> (axum::Router, HttpShutdown) {
    let mut config = StreamableHttpServerConfig::default();
    if cfg.http_host == "0.0.0.0" || cfg.http_host == "::" {
        // Container/remote deployments: the Host header is whatever the operator exposes.
        config = config.disable_allowed_hosts();
    } else {
        config = config.with_allowed_hosts([
            "localhost".to_string(),
            "127.0.0.1".into(),
            "::1".into(),
            cfg.http_host.clone(),
            format!("{}:{}", cfg.http_host, cfg.http_port),
            format!("localhost:{}", cfg.http_port),
            format!("127.0.0.1:{}", cfg.http_port),
        ]);
    }
    let shutdown = HttpShutdown {
        config: config.clone(),
        sessions: Arc::new(LocalSessionManager::default()),
        admission: Arc::new(tokio::sync::RwLock::new(())),
    };
    let handler_memory = memory.clone();
    let mcp = StreamableHttpService::new(
        move || Ok(MemoryServer::new(handler_memory.clone())),
        shutdown.sessions.clone(),
        config,
    );
    let auth = Auth {
        api_key: cfg.api_key.clone(),
    };
    let oauth: axum::Router = match &cfg.api_key {
        Some(key) => {
            crate::oauth::router(crate::oauth::OAuthState::new(key.clone(), memory.clone()))
        }
        None => axum::Router::new()
            .route(
                "/.well-known/oauth-authorization-server",
                get(oauth_not_configured),
            )
            .route(
                "/.well-known/oauth-protected-resource",
                get(oauth_not_configured),
            )
            .route(
                "/.well-known/oauth-protected-resource/mcp",
                get(oauth_not_configured),
            )
            .route("/register", any(oauth_not_configured))
            .route("/authorize", any(oauth_not_configured))
            .route("/token", any(oauth_not_configured)),
    };
    let router = axum::Router::new()
        .route("/health", get(|| async { Json(json!({ "status": "ok" })) }))
        .route("/.well-known/mcp.json", get(|| async { Json(discovery()) }))
        .route(
            "/.well-known/mcp/server.json",
            get(|| async { Json(discovery()) }),
        )
        .merge(oauth)
        .nest_service(
            "/mcp",
            axum::Router::new()
                .fallback_service(mcp)
                .layer(middleware::from_fn_with_state(auth, bearer_guard)),
        )
        .fallback(|| async { (StatusCode::NOT_FOUND, Json(json!({ "error": "Not found" }))) })
        .layer(middleware::from_fn_with_state(
            shutdown.clone(),
            shutdown_guard,
        ));
    (router, shutdown)
}

pub async fn serve(memory: SharedMemory, cfg: &ServeConfig) -> std::io::Result<()> {
    let app = router(memory, cfg);
    let listener = tokio::net::TcpListener::bind((cfg.http_host.as_str(), cfg.http_port)).await?;
    eprintln!(
        "[INFO] advisaur-mcp running on http://{}:{}/mcp",
        cfg.http_host, cfg.http_port
    );
    if cfg.api_key.is_some() {
        eprintln!("[INFO] Bearer token auth enabled");
    } else {
        eprintln!("[WARN] No SHELBY_API_KEY set — running without auth");
    }
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use assistant_memory::Memory;
    use axum::body::{Body, to_bytes};
    use axum::http::{HeaderMap, Method, Request};
    use serde_json::Value;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Mutex;
    use tower::ServiceExt;

    const API_KEY: &str = "test-api-key";
    const HOST: &str = "shelby.test";
    const RESOURCE: &str = "http://shelby.test/mcp";
    const REDIRECT_URI: &str = "https://client.example/callback";
    const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
    const TEST_RATE_LIMIT_MAX: u32 = 5;

    fn test_router(api_key: Option<&str>) -> axum::Router {
        let memory = Arc::new(Mutex::new(Memory::open_in_memory().unwrap()));
        let cfg = ServeConfig {
            db_path: ":memory:".into(),
            verbose: false,
            transport: crate::config::Transport::Http,
            http_port: 80,
            http_host: HOST.into(),
            api_key: api_key.map(str::to_owned),
        };
        router(memory, &cfg)
    }

    fn request(
        method: Method,
        uri: &str,
        content_type: Option<&str>,
        body: String,
    ) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, HOST);
        if let Some(content_type) = content_type {
            builder = builder.header(header::CONTENT_TYPE, content_type);
        }
        let mut request = builder.body(Body::from(body)).unwrap();
        request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                4242,
            )));
        request
    }

    async fn body(response: Response) -> (StatusCode, HeaderMap, String) {
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, headers, String::from_utf8(bytes.to_vec()).unwrap())
    }

    async fn register_client(app: &axum::Router) -> String {
        let response = app
            .clone()
            .oneshot(request(
                Method::POST,
                "/register",
                Some("application/json"),
                json!({
                    "client_name": "Test client",
                    "redirect_uris": [REDIRECT_URI]
                })
                .to_string(),
            ))
            .await
            .unwrap();
        let (status, _, text) = body(response).await;
        assert_eq!(status, StatusCode::CREATED, "{text}");
        serde_json::from_str::<Value>(&text).unwrap()["client_id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn authorize_form(client_id: &str, api_key: &str) -> String {
        format!(
            "response_type=code&client_id={client_id}&redirect_uri=https%3A%2F%2Fclient.example%2Fcallback&code_challenge={CHALLENGE}&code_challenge_method=S256&resource=http%3A%2F%2Fshelby.test%2Fmcp&state=csrf-state&api_key={api_key}"
        )
    }

    async fn authorize_code(app: &axum::Router, client_id: &str) -> String {
        let response = app
            .clone()
            .oneshot(request(
                Method::POST,
                "/authorize",
                Some("application/x-www-form-urlencoded"),
                authorize_form(client_id, API_KEY),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FOUND);
        let location = response.headers()[header::LOCATION].to_str().unwrap();
        location
            .split("code=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap()
            .to_string()
    }

    #[tokio::test]
    async fn oauth_discovery_and_unauthorized_challenge_identify_the_mcp_resource() {
        let app = test_router(Some(API_KEY));
        let response = app
            .clone()
            .oneshot(request(
                Method::GET,
                "/.well-known/oauth-protected-resource/mcp",
                None,
                String::new(),
            ))
            .await
            .unwrap();
        let (status, _, text) = body(response).await;
        assert_eq!(status, StatusCode::OK, "{text}");
        let metadata: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(metadata["resource"], RESOURCE);
        assert_eq!(
            metadata["authorization_servers"],
            json!(["http://shelby.test"])
        );

        let response = app
            .oneshot(request(
                Method::POST,
                "/mcp",
                Some("application/json"),
                "{}".into(),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers()[header::WWW_AUTHENTICATE],
            "Bearer resource_metadata=\"http://shelby.test/.well-known/oauth-protected-resource/mcp\""
        );

        for path in [
            "/.well-known/oauth-authorization-server",
            "/.well-known/oauth-protected-resource/mcp",
        ] {
            let response = test_router(None)
                .oneshot(request(Method::GET, path, None, String::new()))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        }
    }

    #[tokio::test]
    async fn oauth_metadata_registration_and_consent_form_are_route_complete() {
        let app = test_router(Some(API_KEY));
        let response = app
            .clone()
            .oneshot(request(
                Method::GET,
                "/.well-known/oauth-authorization-server",
                None,
                String::new(),
            ))
            .await
            .unwrap();
        let (status, _, text) = body(response).await;
        assert_eq!(status, StatusCode::OK);
        let metadata: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(metadata["issuer"], "http://shelby.test");
        assert_eq!(
            metadata["authorization_endpoint"],
            "http://shelby.test/authorize"
        );
        assert_eq!(metadata["token_endpoint"], "http://shelby.test/token");
        assert_eq!(
            metadata["registration_endpoint"],
            "http://shelby.test/register"
        );
        assert_eq!(
            metadata["code_challenge_methods_supported"],
            json!(["S256"])
        );

        let client_id = register_client(&app).await;
        let authorize = format!(
            "/authorize?response_type=code&client_id={client_id}&redirect_uri=https%3A%2F%2Fclient.example%2Fcallback&code_challenge={CHALLENGE}&code_challenge_method=S256&resource=http%3A%2F%2Fshelby.test%2Fmcp&state=csrf-state"
        );
        let response = app
            .clone()
            .oneshot(request(Method::GET, &authorize, None, String::new()))
            .await
            .unwrap();
        let (status, _, html) = body(response).await;
        assert_eq!(status, StatusCode::OK);
        assert!(html.contains("<form"));
        assert!(html.contains("name=\"resource\" value=\"http://shelby.test/mcp\""));
        assert!(html.contains("name=\"code_challenge_method\" value=\"S256\""));

        let unknown = authorize.replace(&client_id, "unknown-client");
        assert_eq!(
            app.clone()
                .oneshot(request(Method::GET, &unknown, None, String::new()))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
        let mismatch = authorize.replace(
            "https%3A%2F%2Fclient.example%2Fcallback",
            "https%3A%2F%2Fevil.example%2Fcallback",
        );
        assert_eq!(
            app.oneshot(request(Method::GET, &mismatch, None, String::new()))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn authorization_requires_s256_pkce_and_the_exact_resource() {
        let app = test_router(Some(API_KEY));
        let client_id = register_client(&app).await;
        let base = format!(
            "/authorize?response_type=code&client_id={client_id}&redirect_uri=https%3A%2F%2Fclient.example%2Fcallback&code_challenge={CHALLENGE}&state=csrf-state"
        );
        for suffix in [
            "&resource=http%3A%2F%2Fshelby.test%2Fmcp",
            "&code_challenge_method=S256&resource=http%3A%2F%2Fevil.test%2Fmcp",
            "&code_challenge_method=plain&resource=http%3A%2F%2Fshelby.test%2Fmcp",
        ] {
            let response = app
                .clone()
                .oneshot(request(
                    Method::GET,
                    &format!("{base}{suffix}"),
                    None,
                    String::new(),
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }

        let invalid_post = authorize_form(&client_id, API_KEY)
            .replace("code_challenge_method=S256", "code_challenge_method=plain");
        let response = app
            .oneshot(request(
                Method::POST,
                "/authorize",
                Some("application/x-www-form-urlencoded"),
                invalid_post,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn oauth_code_flow_is_single_use_and_issues_a_resource_bound_bearer() {
        let app = test_router(Some(API_KEY));
        let client_id = register_client(&app).await;
        let code = authorize_code(&app, &client_id).await;
        let token_form = format!(
            "grant_type=authorization_code&code={code}&client_id={client_id}&code_verifier={VERIFIER}&redirect_uri=https%3A%2F%2Fclient.example%2Fcallback&resource=http%3A%2F%2Fshelby.test%2Fmcp"
        );
        let response = app
            .clone()
            .oneshot(request(
                Method::POST,
                "/token",
                Some("application/x-www-form-urlencoded"),
                token_form.clone(),
            ))
            .await
            .unwrap();
        let (status, _, text) = body(response).await;
        assert_eq!(status, StatusCode::OK, "{text}");
        let tokens: Value = serde_json::from_str(&text).unwrap();
        let access_token = tokens["access_token"].as_str().unwrap();

        let replay = app
            .clone()
            .oneshot(request(
                Method::POST,
                "/token",
                Some("application/x-www-form-urlencoded"),
                token_form,
            ))
            .await
            .unwrap();
        assert_eq!(replay.status(), StatusCode::BAD_REQUEST);

        let mut mcp = request(
            Method::POST,
            "/mcp",
            Some("application/json"),
            json!({
                "jsonrpc": "2.0",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": { "name": "test", "version": "0" }
                },
                "id": 1
            })
            .to_string(),
        );
        mcp.headers_mut().insert(
            header::AUTHORIZATION,
            format!("Bearer {access_token}").parse().unwrap(),
        );
        mcp.headers_mut().insert(
            header::ACCEPT,
            "application/json, text/event-stream".parse().unwrap(),
        );
        let response = app.clone().oneshot(mcp).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let wrong_resource_token =
            crate::oauth::derive_access_token(API_KEY, "http://other.test/mcp");
        let mut rejected = request(Method::POST, "/mcp", Some("application/json"), "{}".into());
        rejected.headers_mut().insert(
            header::AUTHORIZATION,
            format!("Bearer {wrong_resource_token}").parse().unwrap(),
        );
        assert_eq!(
            app.oneshot(rejected).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn token_exchange_rejects_bad_pkce_and_refreshes_only_for_the_bound_resource() {
        let app = test_router(Some(API_KEY));
        let client_id = register_client(&app).await;
        let code = authorize_code(&app, &client_id).await;
        let bad_pkce = format!(
            "grant_type=authorization_code&code={code}&client_id={client_id}&code_verifier=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa&redirect_uri=https%3A%2F%2Fclient.example%2Fcallback&resource=http%3A%2F%2Fshelby.test%2Fmcp"
        );
        let response = app
            .clone()
            .oneshot(request(
                Method::POST,
                "/token",
                Some("application/x-www-form-urlencoded"),
                bad_pkce,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let code = authorize_code(&app, &client_id).await;
        let response = app
            .clone()
            .oneshot(request(
                Method::POST,
                "/token",
                Some("application/x-www-form-urlencoded"),
                format!(
                    "grant_type=authorization_code&code={code}&client_id={client_id}&code_verifier={VERIFIER}&redirect_uri=https%3A%2F%2Fclient.example%2Fcallback&resource=http%3A%2F%2Fshelby.test%2Fmcp"
                ),
            ))
            .await
            .unwrap();
        let (status, _, text) = body(response).await;
        assert_eq!(status, StatusCode::OK, "{text}");
        let refresh_token = serde_json::from_str::<Value>(&text).unwrap()["refresh_token"]
            .as_str()
            .unwrap()
            .to_string();

        let wrong_resource = format!(
            "grant_type=refresh_token&refresh_token={refresh_token}&client_id={client_id}&resource=http%3A%2F%2Fother.test%2Fmcp"
        );
        let response = app
            .clone()
            .oneshot(request(
                Method::POST,
                "/token",
                Some("application/x-www-form-urlencoded"),
                wrong_resource,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let response = app
            .oneshot(request(
                Method::POST,
                "/token",
                Some("application/x-www-form-urlencoded"),
                format!(
                    "grant_type=refresh_token&refresh_token={refresh_token}&client_id={client_id}&resource=http%3A%2F%2Fshelby.test%2Fmcp"
                ),
            ))
            .await
            .unwrap();
        let (status, _, text) = body(response).await;
        assert_eq!(status, StatusCode::OK, "{text}");
        assert_eq!(
            serde_json::from_str::<Value>(&text).unwrap()["token_type"],
            "Bearer"
        );
    }

    #[tokio::test]
    async fn authorization_rate_limits_repeated_bad_api_keys() {
        let app = test_router(Some(API_KEY));
        let client_id = register_client(&app).await;
        for attempt in 0..=TEST_RATE_LIMIT_MAX {
            let response = app
                .clone()
                .oneshot(request(
                    Method::POST,
                    "/authorize",
                    Some("application/x-www-form-urlencoded"),
                    authorize_form(&client_id, "wrong-key"),
                ))
                .await
                .unwrap();
            if attempt < TEST_RATE_LIMIT_MAX {
                assert_eq!(response.status(), StatusCode::OK);
            } else {
                assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
            }
        }
    }

    #[tokio::test]
    async fn embedded_router_authorizes_without_connect_info() {
        let app = test_router(Some(API_KEY));
        let client_id = register_client(&app).await;
        let mut request = request(
            Method::POST,
            "/authorize",
            Some("application/x-www-form-urlencoded"),
            authorize_form(&client_id, API_KEY),
        );
        request
            .extensions_mut()
            .remove::<axum::extract::ConnectInfo<SocketAddr>>();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::FOUND);
    }
}

#[cfg(test)]
#[path = "http/shutdown_tests.rs"]
mod shutdown_tests;
