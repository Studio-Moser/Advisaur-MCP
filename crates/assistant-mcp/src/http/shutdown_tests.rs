use super::*;
use assistant_memory::Memory;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

async fn request(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &str,
    body: &str,
) -> (String, TcpStream) {
    let mut socket = TcpStream::connect(address).await.unwrap();
    socket.write_all(format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n{body}", body.len()).as_bytes()).await.unwrap();
    let mut response = Vec::new();
    while !response.ends_with(b"\r\n\r\n") {
        assert!(response.len() < 16_384);
        response.push(socket.read_u8().await.unwrap());
    }
    (String::from_utf8(response).unwrap(), socket)
}

async fn loopback_shutdown(api_key: Option<&str>) {
    let memory = Arc::new(Mutex::new(Memory::open_in_memory().unwrap()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let cfg = ServeConfig {
        db_path: ":memory:".into(),
        verbose: false,
        transport: crate::config::Transport::Http,
        http_port: address.port(),
        http_host: "127.0.0.1".into(),
        api_key: api_key.map(str::to_owned),
    };
    let (app, shutdown) = managed_router(memory.clone(), &cfg);
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let auth = api_key
        .map(|key| format!("Authorization: Bearer {key}\r\n"))
        .unwrap_or_default();
    let (discovery, _) = request(address, "GET", "/.well-known/mcp.json", "", "").await;
    assert!(discovery.starts_with("HTTP/1.1 200"));
    let (oauth, _) = request(
        address,
        "GET",
        "/.well-known/oauth-authorization-server",
        "",
        "",
    )
    .await;
    assert!(oauth.starts_with(if api_key.is_some() {
        "HTTP/1.1 200"
    } else {
        "HTTP/1.1 503"
    }));
    if api_key.is_some() {
        let (rejected, _) = request(
            address,
            "POST",
            "/mcp",
            "Content-Type: application/json\r\n",
            "{}",
        )
        .await;
        assert!(rejected.starts_with("HTTP/1.1 401"));
        assert!(
            rejected
                .to_lowercase()
                .contains("www-authenticate: bearer resource_metadata=")
        );
    }
    let initialize = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"shutdown-proof","version":"1"}}}).to_string();
    let headers = format!(
        "{auth}Content-Type: application/json\r\nAccept: application/json, text/event-stream\r\n"
    );
    let (initialized, mut initialization_body) =
        request(address, "POST", "/mcp", &headers, &initialize).await;
    assert!(initialized.starts_with("HTTP/1.1 200"), "{initialized}");
    let session = initialized
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("mcp-session-id"))
                .map(|(_, value)| value.trim().to_owned())
        })
        .unwrap();
    let mut response = Vec::new();
    initialization_body
        .read_to_end(&mut response)
        .await
        .unwrap();
    assert!(String::from_utf8(response).unwrap().contains("serverInfo"));
    drop(initialization_body);
    let session_headers =
        format!("{headers}Mcp-Session-Id: {session}\r\nMCP-Protocol-Version: 2025-06-18\r\n");
    let (notified, _) = request(
        address,
        "POST",
        "/mcp",
        &session_headers,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
    )
    .await;
    assert!(notified.starts_with("HTTP/1.1 202"), "{notified}");
    let (sse_headers, mut sse) = request(address, "GET", "/mcp", &session_headers, "").await;
    assert!(sse_headers.starts_with("HTTP/1.1 200"), "{sse_headers}");
    assert_eq!(shutdown.sessions.sessions.read().await.len(), 1);
    // Keep SSE connected while cancellation and registry closure run.
    shutdown.request_shutdown();
    for path in ["/mcp", "/health", "/register", "/.well-known/mcp.json"] {
        let (rejected, _) = request(address, "GET", path, &auth, "").await;
        assert!(rejected.starts_with("HTTP/1.1 503"), "{path}: {rejected}");
    }
    shutdown.close_sessions().await.unwrap();
    assert!(shutdown.sessions.sessions.read().await.is_empty());
    shutdown.close_sessions().await.unwrap();
    let mut remaining = Vec::new();
    sse.read_to_end(&mut remaining).await.unwrap();
    drop(sse);
    stop.send(()).unwrap();
    server.await.unwrap();
    drop(shutdown);
    // rmcp owns detached worker tasks; registry/listener completion is not an owner proof.
    let mut retained = memory;
    loop {
        match Arc::try_unwrap(retained) {
            Ok(memory) => {
                drop(memory.into_inner().unwrap());
                break;
            }
            Err(memory) => {
                retained = memory;
                tokio::task::yield_now().await;
            }
        }
    }
}

#[tokio::test]
async fn loopback_sessions_and_sse_shutdown_release_the_actual_memory_owner() {
    tokio::time::timeout(Duration::from_secs(10), loopback_shutdown(None))
        .await
        .unwrap();
}
#[tokio::test]
async fn authenticated_loopback_shutdown_preserves_oauth_until_admission_closes() {
    tokio::time::timeout(
        Duration::from_secs(10),
        loopback_shutdown(Some("synthetic-key")),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn shutdown_waits_for_an_admitted_initialize_before_draining_sessions() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let memory = Arc::new(Mutex::new(Memory::open_in_memory().unwrap()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let cfg = ServeConfig {
            db_path: ":memory:".into(), verbose: false,
            transport: crate::config::Transport::Http,
            http_port: address.port(), http_host: "127.0.0.1".into(), api_key: None,
        };
        let (app, shutdown) = managed_router(memory.clone(), &cfg);
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).with_graceful_shutdown(async { let _ = stopped.await; }).await.unwrap();
        });
        let mut socket = TcpStream::connect(address).await.unwrap();
        let initialize = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"shutdown-race","version":"1"}}}).to_string();
        socket.write_all(format!("POST /mcp HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\n\r\n{}", initialize.len(), &initialize[..initialize.len()-1]).as_bytes()).await.unwrap();
        // Prove this actual network request owns admission while its body is incomplete.
        while shutdown.admission.try_write().is_ok() { tokio::task::yield_now().await; }
        shutdown.request_shutdown();
        let closing = shutdown.clone();
        let mut close = tokio::spawn(async move { closing.close_sessions().await });
        assert!(tokio::time::timeout(Duration::from_millis(20), &mut close).await.is_err());
        socket.write_all(&initialize.as_bytes()[initialize.len()-1..]).await.unwrap();
        close.await.unwrap().unwrap();
        assert!(shutdown.sessions.sessions.read().await.is_empty());
        drop(socket);
        stop.send(()).unwrap();
        server.await.unwrap();
        drop(shutdown);
        let mut retained = memory;
        loop {
            match Arc::try_unwrap(retained) {
                Ok(memory) => { drop(memory.into_inner().unwrap()); break; }
                Err(memory) => { retained = memory; tokio::task::yield_now().await; }
            }
        }
    }).await.unwrap();
}
