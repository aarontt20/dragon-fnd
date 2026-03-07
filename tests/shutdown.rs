#![cfg(feature = "shutdown")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dragon_fnd::shutdown::{ShutdownBuilder, ShutdownError};
use dragon_fnd::AppContext;

#[tokio::test]
async fn appcontext_with_shutdown() {
    let ctx = AppContext::builder()
        .with_config(())
        .with_shutdown(ShutdownBuilder::new())
        .build()
        .await
        .unwrap();

    assert!(ctx.shutdown().is_some());
}

#[tokio::test]
async fn appcontext_without_shutdown() {
    // shutdown feature is enabled but not registered — accessor returns None.
    // Need another async subsystem to get AsyncBuild, so use shutdown itself.
    // Without any async subsystem, we'd use build_sync().
    let ctx = AppContext::builder().with_config(()).build_sync().unwrap();

    assert!(ctx.shutdown().is_none());
}

#[tokio::test]
async fn manual_trigger_and_wait() {
    let ctx = AppContext::builder()
        .with_config(())
        .with_shutdown(ShutdownBuilder::new().grace_period(Duration::from_secs(5)))
        .build()
        .await
        .unwrap();

    let shutdown = ctx.shutdown().unwrap();
    shutdown.trigger();
    shutdown.wait().await.unwrap();
}

#[tokio::test]
async fn cleanup_reverse_order() {
    let ctx = AppContext::builder()
        .with_config(())
        .with_shutdown(ShutdownBuilder::new())
        .build()
        .await
        .unwrap();

    let shutdown = ctx.shutdown().unwrap();
    let order = Arc::new(std::sync::Mutex::new(Vec::new()));

    let o1 = order.clone();
    shutdown
        .register_cleanup("first", move || async move {
            o1.lock().unwrap().push(1);
        })
        .unwrap();

    let o2 = order.clone();
    shutdown
        .register_cleanup("second", move || async move {
            o2.lock().unwrap().push(2);
        })
        .unwrap();

    let o3 = order.clone();
    shutdown
        .register_cleanup("third", move || async move {
            o3.lock().unwrap().push(3);
        })
        .unwrap();

    shutdown.trigger();
    shutdown.wait().await.unwrap();

    let executed = order.lock().unwrap();
    assert_eq!(*executed, vec![3, 2, 1]);
}

#[tokio::test]
async fn grace_period_exceeded() {
    let ctx = AppContext::builder()
        .with_config(())
        .with_shutdown(ShutdownBuilder::new().grace_period(Duration::from_millis(50)))
        .build()
        .await
        .unwrap();

    let shutdown = ctx.shutdown().unwrap();
    shutdown
        .register_cleanup("slow", || async {
            tokio::time::sleep(Duration::from_secs(10)).await;
        })
        .unwrap();

    shutdown.trigger();
    let err = shutdown.wait().await.unwrap_err();

    match err {
        ShutdownError::GracePeriodExceeded {
            remaining,
            panicked,
            ..
        } => {
            // "slow" was still running when time ran out
            assert_eq!(remaining, vec!["slow"]);
            assert!(panicked.is_empty());
        }
        other => panic!("expected GracePeriodExceeded, got: {other:?}"),
    }
}

#[tokio::test]
async fn wait_result_shared_across_callers() {
    let ctx = AppContext::builder()
        .with_config(())
        .with_shutdown(ShutdownBuilder::new())
        .build()
        .await
        .unwrap();

    let shutdown = ctx.shutdown().unwrap();
    shutdown.trigger();

    let r1 = shutdown.wait().await;
    let r2 = shutdown.wait().await;
    assert!(r1.is_ok());
    assert!(r2.is_ok());
}

#[tokio::test]
async fn late_registration_after_trigger() {
    let ctx = AppContext::builder()
        .with_config(())
        .with_shutdown(ShutdownBuilder::new())
        .build()
        .await
        .unwrap();

    let shutdown = ctx.shutdown().unwrap();
    shutdown.trigger();

    let result = shutdown.register_cleanup("too-late", || async {});
    assert!(matches!(result, Err(ShutdownError::AlreadyTriggered)));
}

#[tokio::test]
async fn token_cross_task_cancellation() {
    let ctx = AppContext::builder()
        .with_config(())
        .with_shutdown(ShutdownBuilder::new())
        .build()
        .await
        .unwrap();

    let shutdown = ctx.shutdown().unwrap();
    let token = shutdown.token();

    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();

    let handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = token.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_millis(1)) => {
                    c.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
    });

    // Let the worker do some iterations
    tokio::time::sleep(Duration::from_millis(20)).await;
    shutdown.trigger();
    handle.await.unwrap();

    assert!(counter.load(Ordering::SeqCst) > 0);
}

#[tokio::test]
async fn signal_handler_install_succeeds() {
    // Verifies init_shutdown() succeeds in a tokio runtime — signal
    // handler installation requires an active runtime.
    let ctx = AppContext::builder()
        .with_config(())
        .with_shutdown(ShutdownBuilder::new())
        .build()
        .await;

    assert!(ctx.is_ok());
}

#[tokio::test]
async fn hook_panic_safety() {
    let ctx = AppContext::builder()
        .with_config(())
        .with_shutdown(ShutdownBuilder::new())
        .build()
        .await
        .unwrap();

    let shutdown = ctx.shutdown().unwrap();
    let ran = Arc::new(AtomicUsize::new(0));

    let r = ran.clone();
    shutdown
        .register_cleanup("good-before", move || async move {
            r.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();

    shutdown
        .register_cleanup("panicker", || async {
            panic!("boom");
        })
        .unwrap();

    let r = ran.clone();
    shutdown
        .register_cleanup("good-after", move || async move {
            r.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();

    shutdown.trigger();
    let result = shutdown.wait().await;
    // wait() should succeed despite the panic
    assert!(result.is_ok());
    // Both non-panicking hooks ran (reverse order: good-after, panicker, good-before)
    assert_eq!(ran.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn context_debug_with_shutdown() {
    let ctx = AppContext::builder()
        .with_config(42u32)
        .with_shutdown(ShutdownBuilder::new())
        .build()
        .await
        .unwrap();

    let debug = format!("{:?}", ctx);
    assert!(debug.contains("AppContext"));
    assert!(debug.contains("shutdown: true"));
}

#[tokio::test]
async fn builder_debug_with_shutdown() {
    let builder = AppContext::builder()
        .with_shutdown(ShutdownBuilder::new());

    let debug = format!("{:?}", builder);
    assert!(debug.contains("AppContextBuilder"));
    assert!(debug.contains("shutdown: true"));
}
