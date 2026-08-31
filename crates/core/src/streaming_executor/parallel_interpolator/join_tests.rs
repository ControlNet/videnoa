use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};

use super::join_workers;

#[test]
fn join_workers_waits_for_every_handle_after_panic() -> Result<()> {
    // Given
    let (panic_started_tx, panic_started_rx) = mpsc::channel();
    let panicking = thread::spawn(move || {
        let _ = panic_started_tx.send(());
        std::panic::resume_unwind(Box::new("injected worker panic"));
    });
    let (waiting_tx, waiting_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let second_finished = Arc::new(AtomicBool::new(false));
    let second_finished_worker = Arc::clone(&second_finished);
    let second = thread::spawn(move || {
        let _ = waiting_tx.send(());
        let _ = release_rx.recv();
        second_finished_worker.store(true, Ordering::SeqCst);
    });
    panic_started_rx
        .recv_timeout(Duration::from_secs(1))
        .map_err(|error| anyhow!("panicking lane did not start: {error}"))?;
    waiting_rx
        .recv_timeout(Duration::from_secs(1))
        .map_err(|error| anyhow!("second lane did not start: {error}"))?;
    let (result_tx, result_rx) = mpsc::channel();
    let join_task = thread::spawn(move || {
        let result = join_workers(vec![panicking, second], Ok(()));
        let _ = result_tx.send(result.is_err());
    });

    // When
    let returned_before_release = result_rx.recv_timeout(Duration::from_millis(100)).ok();
    release_tx
        .send(())
        .map_err(|_| anyhow!("failed to release second lane"))?;
    let returned_error = match returned_before_release {
        Some(result) => result,
        None => result_rx
            .recv_timeout(Duration::from_secs(1))
            .map_err(|error| anyhow!("join task did not finish: {error}"))?,
    };
    join_task
        .join()
        .map_err(|_| anyhow!("join verification task panicked"))?;

    // Then
    assert!(
        returned_before_release.is_none(),
        "stage returned before every worker handle was joined"
    );
    assert!(returned_error, "worker panic must remain an error");
    assert!(second_finished.load(Ordering::SeqCst));
    Ok(())
}
