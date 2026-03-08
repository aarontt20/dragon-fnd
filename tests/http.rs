#![cfg(feature = "http")]

use dragon_fnd::http::{HttpBuilder, HttpConfig, HttpError};
use dragon_fnd::shutdown::ShutdownBuilder;
use dragon_fnd::AppContext;

#[tokio::test]
async fn appcontext_with_http() {
    let ctx = AppContext::builder()
        .with_config(())
        .with_shutdown(ShutdownBuilder::new())
        .with_http(HttpBuilder::new().host("127.0.0.1").port(0))
        .build()
        .await
        .unwrap();

    assert!(ctx.http().is_some());
    assert!(ctx.shutdown().is_some());
}

#[tokio::test]
async fn appcontext_without_http() {
    // http feature is enabled but with_http() not called — accessor returns None
    let ctx = AppContext::builder()
        .with_config(())
        .with_shutdown(ShutdownBuilder::new())
        .build()
        .await
        .unwrap();

    assert!(ctx.http().is_none());
}

#[tokio::test]
async fn bind_to_port_0_assigns_port() {
    let ctx = AppContext::builder()
        .with_config(())
        .with_shutdown(ShutdownBuilder::new())
        .with_http(HttpBuilder::new().host("127.0.0.1").port(0))
        .build()
        .await
        .unwrap();

    let http = ctx.http().unwrap();
    let addr = http.local_addr();
    assert_ne!(addr.port(), 0);
    assert_eq!(addr.ip(), std::net::Ipv4Addr::new(127, 0, 0, 1));
}

#[tokio::test]
async fn bind_failure() {
    // Bind to a port that's already in use
    let first = AppContext::builder()
        .with_config(())
        .with_shutdown(ShutdownBuilder::new())
        .with_http(HttpBuilder::new().host("127.0.0.1").port(0))
        .build()
        .await
        .unwrap();

    let port = first.http().unwrap().local_addr().port();

    // Try to bind to the same port
    let result = AppContext::builder()
        .with_config(())
        .with_shutdown(ShutdownBuilder::new())
        .with_http(HttpBuilder::new().host("127.0.0.1").port(port))
        .build()
        .await;

    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("failed to bind"), "got: {msg}");
}

#[tokio::test]
async fn missing_shutdown_returns_error() {
    // Register http but not shutdown — should fail at build time.
    // with_http() on SyncBuild calls into_async(), so we get AsyncBuild
    // with shutdown = None.
    let result = AppContext::builder()
        .with_config(())
        .with_http(HttpBuilder::new().host("127.0.0.1").port(0))
        .build()
        .await;

    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("requires shutdown"), "got: {msg}");
}

#[tokio::test]
async fn serve_and_shutdown() {
    let ctx = AppContext::builder()
        .with_config(())
        .with_shutdown(ShutdownBuilder::new())
        .with_http(HttpBuilder::new().host("127.0.0.1").port(0))
        .build()
        .await
        .unwrap();

    let shutdown = ctx.shutdown().unwrap();
    let http = ctx.http().unwrap();

    // Trigger shutdown immediately so serve() returns
    shutdown.trigger();

    let router = axum::Router::new();
    http.serve(router).await.unwrap();
}

#[tokio::test]
async fn serve_twice_returns_already_serving() {
    let ctx = AppContext::builder()
        .with_config(())
        .with_shutdown(ShutdownBuilder::new())
        .with_http(HttpBuilder::new().host("127.0.0.1").port(0))
        .build()
        .await
        .unwrap();

    let shutdown = ctx.shutdown().unwrap();
    let http = ctx.http().unwrap();

    shutdown.trigger();
    http.serve(axum::Router::new()).await.unwrap();

    let result = http.serve(axum::Router::new()).await;
    assert!(matches!(result, Err(HttpError::AlreadyServing)));
}

#[tokio::test]
async fn config_deserialization() {
    let toml = r#"
        host = "10.0.0.1"
        port = 9090
    "#;
    let config: HttpConfig = toml::from_str(toml).unwrap();
    let builder = HttpBuilder::from_config(config);

    let ctx = AppContext::builder()
        .with_config(())
        .with_shutdown(ShutdownBuilder::new())
        .with_http(builder)
        .build()
        .await;

    // Bind may fail since 10.0.0.1 likely isn't a local address,
    // but the config deserialized correctly — that's what we're testing.
    // If it happens to succeed, great. If not, verify it's a bind error.
    match ctx {
        Ok(ctx) => assert!(ctx.http().is_some()),
        Err(e) => assert!(e.to_string().contains("failed to bind"), "got: {e}"),
    }
}

#[tokio::test]
async fn context_debug_with_http() {
    let ctx = AppContext::builder()
        .with_config(42u32)
        .with_shutdown(ShutdownBuilder::new())
        .with_http(HttpBuilder::new().host("127.0.0.1").port(0))
        .build()
        .await
        .unwrap();

    let debug = format!("{:?}", ctx);
    assert!(debug.contains("AppContext"));
    assert!(debug.contains("http: true"));
    assert!(debug.contains("shutdown: true"));
}

#[tokio::test]
async fn builder_debug_with_http() {
    let builder = AppContext::builder()
        .with_shutdown(ShutdownBuilder::new())
        .with_http(HttpBuilder::new());

    let debug = format!("{:?}", builder);
    assert!(debug.contains("AppContextBuilder"));
    assert!(debug.contains("http: true"));
    assert!(debug.contains("shutdown: true"));
}

#[tokio::test]
async fn serve_with_router_and_programmatic_shutdown() {
    use std::sync::Arc;

    let ctx = Arc::new(
        AppContext::builder()
            .with_config(())
            .with_shutdown(ShutdownBuilder::new())
            .with_http(HttpBuilder::new().host("127.0.0.1").port(0))
            .build()
            .await
            .unwrap(),
    );

    let addr = ctx.http().unwrap().local_addr();

    let ctx_clone = ctx.clone();
    let serve_handle = tokio::spawn(async move {
        let router = axum::Router::new()
            .route("/health", axum::routing::get(|| async { "ok" }));
        ctx_clone.http().unwrap().serve(router).await
    });

    // Give the server a moment to start accepting connections
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Make a request to verify it's serving
    let client = tokio::net::TcpStream::connect(addr).await;
    assert!(client.is_ok(), "should be able to connect to the server");

    // Trigger shutdown
    ctx.shutdown().unwrap().trigger();

    let result = serve_handle.await.unwrap();
    assert!(result.is_ok());
}

#[tokio::test]
async fn local_addr_stable_after_serve() {
    let ctx = AppContext::builder()
        .with_config(())
        .with_shutdown(ShutdownBuilder::new())
        .with_http(HttpBuilder::new().host("127.0.0.1").port(0))
        .build()
        .await
        .unwrap();

    let http = ctx.http().unwrap();
    let addr_before = http.local_addr();

    ctx.shutdown().unwrap().trigger();
    http.serve(axum::Router::new()).await.unwrap();

    let addr_after = http.local_addr();
    assert_eq!(addr_before, addr_after);
}
