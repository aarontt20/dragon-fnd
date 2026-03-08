use std::io;
use thiserror::Error;

/// Errors from the HTTP subsystem.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum HttpError {
    /// TCP listener failed to bind to the configured address.
    #[error("failed to bind to {addr}")]
    BindFailed {
        addr: String,
        #[source]
        source: io::Error,
    },

    /// `serve()` has already been called — it may only be called once.
    #[error("serve() has already been called — it may only be called once")]
    AlreadyServing,

    /// HTTP was registered but shutdown was not.
    #[error("HTTP subsystem requires shutdown — call with_shutdown() on the builder")]
    ShutdownRequired,

    /// The axum server returned an error.
    #[error("server error")]
    ServeFailed {
        #[source]
        source: io::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn bind_failed_display() {
        let err = HttpError::BindFailed {
            addr: "0.0.0.0:8080".into(),
            source: io::Error::new(io::ErrorKind::AddrInUse, "address in use"),
        };
        assert_eq!(err.to_string(), "failed to bind to 0.0.0.0:8080");
        assert!(err.source().is_some());
    }

    #[test]
    fn bind_failed_source_chain() {
        let io_err = io::Error::new(io::ErrorKind::AddrInUse, "port taken");
        let err = HttpError::BindFailed {
            addr: "127.0.0.1:3000".into(),
            source: io_err,
        };
        let source = err.source().unwrap();
        assert_eq!(source.to_string(), "port taken");
    }

    #[test]
    fn already_serving_display() {
        let err = HttpError::AlreadyServing;
        assert_eq!(
            err.to_string(),
            "serve() has already been called \u{2014} it may only be called once"
        );
        assert!(err.source().is_none());
    }

    #[test]
    fn shutdown_required_display() {
        let err = HttpError::ShutdownRequired;
        assert_eq!(
            err.to_string(),
            "HTTP subsystem requires shutdown \u{2014} call with_shutdown() on the builder"
        );
        assert!(err.source().is_none());
    }

    #[test]
    fn serve_failed_display() {
        let err = HttpError::ServeFailed {
            source: io::Error::new(io::ErrorKind::Other, "connection reset"),
        };
        assert_eq!(err.to_string(), "server error");
        assert!(err.source().is_some());
    }

    #[test]
    fn debug() {
        let err = HttpError::AlreadyServing;
        let debug = format!("{:?}", err);
        assert!(debug.contains("AlreadyServing"));
    }

    #[test]
    fn top_level_error_from_http() {
        let http_err = HttpError::ShutdownRequired;
        let err: crate::Error = http_err.into();
        assert_eq!(
            err.to_string(),
            "http error: HTTP subsystem requires shutdown \u{2014} call with_shutdown() on the builder"
        );
    }
}
