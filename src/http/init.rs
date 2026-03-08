use std::net::SocketAddr;
use std::sync::Mutex;

use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use super::config::HttpConfig;
use super::error::HttpError;

/// Handle to the HTTP server, providing serve and address inspection.
///
/// Created by [`AppContextBuilder::build()`](crate::context::AppContextBuilder) when
/// `with_http()` is registered. Use [`serve()`](Self::serve) to start accepting
/// connections and [`local_addr()`](Self::local_addr) to inspect the bound address.
///
/// # Example
///
/// ```no_run
/// # use dragon_fnd::AppContext;
/// # use dragon_fnd::http::HttpBuilder;
/// # use dragon_fnd::shutdown::ShutdownBuilder;
/// # use std::sync::Arc;
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let ctx = Arc::new(
///     AppContext::builder()
///         .with_config(())
///         .with_shutdown(ShutdownBuilder::new())
///         .with_http(HttpBuilder::new().port(0))
///         .build()
///         .await?
/// );
///
/// let router = axum::Router::new()
///     .route("/health", axum::routing::get(|| async { "ok" }));
///
/// if let Some(http) = ctx.http() {
///     http.serve(router).await?;
/// }
/// if let Some(shutdown) = ctx.shutdown() {
///     shutdown.wait().await?;
/// }
/// # Ok(())
/// # }
/// ```
pub struct Http {
    listener: Mutex<Option<TcpListener>>,
    local_addr: SocketAddr,
    token: CancellationToken,
}

impl std::fmt::Debug for Http {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Http")
            .field("local_addr", &self.local_addr)
            .field(
                "listener",
                &self
                    .listener
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .is_some(),
            )
            .finish()
    }
}

impl Http {
    /// Returns the local address the server is bound to.
    ///
    /// This returns the address that was bound at build time, regardless
    /// of whether `serve()` has been called or has already returned.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Serves the given router until shutdown is triggered.
    ///
    /// Consumes the bound listener internally. This is a long-running
    /// future that blocks until the shutdown subsystem's cancellation
    /// token is triggered. The router must have state already applied
    /// via `.with_state()`.
    ///
    /// After `serve()` returns, call `ctx.shutdown().unwrap().wait().await`
    /// to run registered cleanup hooks (e.g., database pool close).
    ///
    /// Returns `HttpError::AlreadyServing` if called more than once.
    pub async fn serve(&self, router: axum::Router) -> Result<(), HttpError> {
        let listener = self
            .listener
            .lock()
            .map_err(|_| HttpError::AlreadyServing)?
            .take()
            .ok_or(HttpError::AlreadyServing)?;

        axum::serve(listener, router)
            .with_graceful_shutdown(self.token.clone().cancelled_owned())
            .await
            .map_err(|source| HttpError::ServeFailed { source })
    }
}

/// Bind a TCP listener and create the HTTP handle.
///
/// Called from `AppContextBuilder::build()`.
pub(crate) async fn init_http(
    config: &HttpConfig,
    token: CancellationToken,
) -> Result<Http, HttpError> {
    let addr = config.addr_string();

    let listener = TcpListener::bind(&addr)
        .await
        .map_err(|source| HttpError::BindFailed {
            addr: addr.clone(),
            source,
        })?;

    let local_addr =
        listener
            .local_addr()
            .map_err(|source| HttpError::BindFailed {
                addr,
                source,
            })?;

    Ok(Http {
        listener: Mutex::new(Some(listener)),
        local_addr,
        token,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(port: u16) -> HttpConfig {
        HttpConfig {
            host: "127.0.0.1".into(),
            port,
        }
    }

    #[tokio::test]
    async fn init_binds_to_port_0() {
        let token = CancellationToken::new();
        let http = init_http(&test_config(0), token).await.unwrap();
        assert_ne!(http.local_addr().port(), 0);
    }

    #[tokio::test]
    async fn local_addr_available_before_serve() {
        let token = CancellationToken::new();
        let http = init_http(&test_config(0), token).await.unwrap();
        let addr = http.local_addr();
        assert_eq!(addr.ip(), std::net::Ipv4Addr::new(127, 0, 0, 1));
        assert_ne!(addr.port(), 0);
    }

    #[tokio::test]
    async fn serve_twice_returns_already_serving() {
        let token = CancellationToken::new();
        let http = init_http(&test_config(0), token.clone()).await.unwrap();
        let router = axum::Router::new();

        // Cancel immediately so serve() returns quickly
        token.cancel();
        http.serve(router).await.unwrap();

        // Second call should fail
        let result = http.serve(axum::Router::new()).await;
        assert!(matches!(result, Err(HttpError::AlreadyServing)));
    }

    #[tokio::test]
    async fn debug_output() {
        let token = CancellationToken::new();
        let http = init_http(&test_config(0), token).await.unwrap();
        let debug = format!("{:?}", http);
        assert!(debug.contains("Http"));
        assert!(debug.contains("local_addr"));
        assert!(debug.contains("listener: true"));
    }

    #[tokio::test]
    async fn debug_after_serve() {
        let token = CancellationToken::new();
        let http = init_http(&test_config(0), token.clone()).await.unwrap();
        token.cancel();
        http.serve(axum::Router::new()).await.unwrap();

        let debug = format!("{:?}", http);
        assert!(debug.contains("listener: false"));
    }
}
