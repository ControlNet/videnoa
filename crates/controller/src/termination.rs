use videnoa_controller::StartupError;

pub(super) enum RuntimeExit {
    Server(Result<(), StartupError>),
    Runtime(Result<(), RuntimeError>),
    Signal(Result<(), std::io::Error>),
}

#[derive(Debug, thiserror::Error)]
pub(super) enum RuntimeError {
    #[error(transparent)]
    Orchestration(#[from] videnoa_controller::orchestration::OrchestrationError),
    #[error(transparent)]
    WorkerHealth(#[from] videnoa_controller::workers::WorkerHealthError),
}

#[cfg(unix)]
pub(super) async fn shutdown_signal() -> Result<(), std::io::Error> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result,
        _ = terminate.recv() => Ok(()),
    }
}

#[cfg(not(unix))]
pub(super) async fn shutdown_signal() -> Result<(), std::io::Error> {
    tokio::signal::ctrl_c().await
}
