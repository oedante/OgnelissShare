//! Запуск службы связи: `LISTEN` (по умолчанию `0.0.0.0:8789`).

use std::net::SocketAddr;
use std::process::ExitCode;
use std::time::Duration;

use ogneliss_signal::{AppState, Config, router};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> ExitCode {
    let listen = std::env::var("LISTEN").unwrap_or_else(|_| "0.0.0.0:8789".into());
    let listener = match TcpListener::bind(&listen).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("ошибка: не удалось занять {listen}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let state = AppState::new(Config::default());

    // Уборщик: раз в 30 секунд убирает истёкшие комнаты.
    let sweeper = tokio::spawn({
        let state = state.clone();
        async move {
            let mut tick = tokio::time::interval(Duration::from_secs(30));
            loop {
                tick.tick().await;
                state.sweep();
            }
        }
    });

    eprintln!("ogneliss-signal {} слушает {listen}", env!("CARGO_PKG_VERSION"));
    let app = router(state.clone()).into_make_service_with_connect_info::<SocketAddr>();
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            eprintln!("останавливаемся");
            // Ждущие опросы отвечают сразу, иначе выход затянулся бы на 30 с.
            state.close();
        })
        .await;
    sweeper.abort();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ошибка: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Ждём Ctrl-C или SIGTERM (его шлёт `docker stop`).
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = term => {}
    }
}
