use std::convert::Infallible;
use std::future::{pending, ready};
use std::sync::Arc;

use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::{self, Stream, StreamExt};
use tokio::sync::broadcast;
use tokio::time::{interval, Duration, MissedTickBehavior};

use crate::auth::authenticate_passive;
use crate::domain::{SseEvent, SseEventKind};
use crate::persistence::DurableChange;

use super::OperationsState;

const EVENT_CAPACITY: usize = 64;
const AUTH_RECHECK_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Clone)]
enum LiveEvent {
    Delta(Arc<SseEvent>),
    DurableChange(DurableChange),
}

#[derive(Clone)]
pub struct EventHub {
    sender: broadcast::Sender<LiveEvent>,
    wakeups: broadcast::Sender<()>,
}

impl EventHub {
    #[must_use]
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(EVENT_CAPACITY);
        let (wakeups, _) = broadcast::channel(EVENT_CAPACITY);
        Self { sender, wakeups }
    }

    pub(crate) fn publish(&self, event: SseEvent) {
        let _ = self.sender.send(LiveEvent::Delta(Arc::new(event)));
        let _ = self.wakeups.send(());
    }

    pub(crate) fn publish_change(&self, change: DurableChange) {
        let _ = self.sender.send(LiveEvent::DurableChange(change));
        let _ = self.wakeups.send(());
    }

    #[must_use]
    pub fn subscribe_wakeups(&self) -> broadcast::Receiver<()> {
        self.wakeups.subscribe()
    }
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new()
    }
}

pub(super) async fn stream(
    State(state): State<OperationsState>,
    headers: HeaderMap,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let initial = stream::once(ready(Ok(refetch_event())));
    let mut auth_recheck = interval(AUTH_RECHECK_INTERVAL);
    auth_recheck.set_missed_tick_behavior(MissedTickBehavior::Delay);
    auth_recheck.tick().await;
    let shutdown = state.shutdown.clone();
    let updates = stream::unfold(
        (
            state.events.sender.subscribe(),
            state,
            headers,
            auth_recheck,
            shutdown,
        ),
        |(mut receiver, state, headers, mut auth_recheck, shutdown)| async move {
            loop {
                tokio::select! {
                    biased;
                    () = async {
                        match &shutdown {
                            Some(shutdown) => shutdown.cancelled().await,
                            None => pending::<()>().await,
                        }
                    } => return None,
                    received = receiver.recv() => {
                        let event = match received {
                            Ok(LiveEvent::Delta(event)) => delta_event(&event),
                            Ok(LiveEvent::DurableChange(change)) => {
                                durable_change_event(&state, change).await
                            }
                            Err(broadcast::error::RecvError::Lagged(_)) => refetch_event(),
                            Err(broadcast::error::RecvError::Closed) => return None,
                        };
                        return Some((Ok(event), (receiver, state, headers, auth_recheck, shutdown)));
                    }
                    _ = auth_recheck.tick() => {
                        if authenticate_passive(&state.auth, &headers, chrono::Utc::now()).await.is_err() {
                            return None;
                        }
                    }
                }
            }
        },
    );
    Sse::new(initial.chain(updates)).keep_alive(KeepAlive::default())
}

async fn durable_change_event(state: &OperationsState, change: DurableChange) -> Event {
    match change {
        DurableChange::Task(id) => match state.store.task(id).await {
            Ok(Some(task)) => delta_event(&SseEvent::TaskUpdated {
                event_id: crate::domain::SseEventId::random(),
                task: crate::tasks::mapping::task(task),
            }),
            Ok(None) | Err(_) => refetch_event(),
        },
        DurableChange::Worker(id) => match state.workers.worker(id).await {
            Ok(Some(record)) => match super::workers::summary(state, record).await {
                Ok(worker) => delta_event(&SseEvent::WorkerUpdated {
                    event_id: crate::domain::SseEventId::random(),
                    worker,
                }),
                Err(_) => refetch_event(),
            },
            Ok(None) | Err(_) => refetch_event(),
        },
        DurableChange::WorkerDeleted => refetch_event(),
        DurableChange::Settings => match state.store.settings().await {
            Ok(settings) => delta_event(&SseEvent::SchedulerUpdated {
                event_id: crate::domain::SseEventId::random(),
                scheduler: settings.scheduler,
            }),
            Err(_) => refetch_event(),
        },
    }
}

fn delta_event(event: &SseEvent) -> Event {
    let kind = match event.kind() {
        SseEventKind::TaskUpdated => "task_updated",
        SseEventKind::WorkerUpdated => "worker_updated",
        SseEventKind::SchedulerUpdated => "scheduler_updated",
    };
    match Event::default().event(kind).json_data(event) {
        Ok(event) => event,
        Err(_) => refetch_event(),
    }
}

fn refetch_event() -> Event {
    Event::default()
        .event("refetch")
        .data("{\"reason\":\"snapshot_required\"}")
}
