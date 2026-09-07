use std::sync::Arc;

use axum::body::to_bytes;
use axum::http::StatusCode;
use serde_json::json;
use tokio::sync::Barrier;
use tokio::task::JoinSet;
use tower::ServiceExt;
use videnoa_controller::domain::{Task, TaskCreateRequest};

use super::support::{fixture, json_request, TestResult};

const RACE_REPETITIONS: u32 = 10;
const REQUESTS_PER_BODY: u32 = 8;

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn mixed_duplicate_intake_races_preserve_one_request_body() -> TestResult {
    // Given: same-key retries for two different canonical request bodies share a start barrier.
    let fixture = fixture().await?;
    for repetition in 0..RACE_REPETITIONS {
        let input = fixture.input_root.join(format!("race-{repetition}.mkv"));
        let output = fixture.output_root.join(format!("race-{repetition}.mp4"));
        std::fs::write(&input, b"task-21-race-input")?;
        let bodies = [
            json!({
                "input_path": input,
                "output_path": output,
                "workflow": "anime-upscale",
                "priority": 7,
                "source": "api",
                "source_reference": format!("task-21-api-{repetition}")
            }),
            json!({
                "input_path": input,
                "output_path": output,
                "workflow": "anime-upscale",
                "priority": 8,
                "source": "manual",
                "source_reference": null
            }),
        ];
        let requests = bodies
            .iter()
            .cloned()
            .map(serde_json::from_value::<TaskCreateRequest>)
            .collect::<Result<Vec<_>, _>>()?;
        let participants = usize::try_from(REQUESTS_PER_BODY * 2 + 1)?;
        let barrier = Arc::new(Barrier::new(participants));
        let mut submissions = JoinSet::new();
        for (body_index, body) in bodies.iter().enumerate() {
            for _ in 0..REQUESTS_PER_BODY {
                let barrier = Arc::clone(&barrier);
                let router = fixture.router.clone();
                let key = format!("task-21-race-{repetition}");
                let body = body.clone();
                submissions.spawn(async move {
                    barrier.wait().await;
                    let mut request = json_request("POST", "/api/tasks", &body)?;
                    request
                        .headers_mut()
                        .insert("idempotency-key", key.parse()?);
                    let response = router.oneshot(request).await?;
                    let status = response.status();
                    let body = to_bytes(response.into_body(), 1024 * 1024).await?;
                    Ok::<_, Box<dyn std::error::Error + Send + Sync>>((body_index, body, status))
                });
            }
        }

        // When: every contender is released together and all durable outcomes are collected.
        barrier.wait().await;
        let mut created = 0;
        let mut replayed = 0;
        let mut conflicts = 0;
        let mut canonical = None;
        let mut replay_bodies = Vec::new();
        while let Some(result) = submissions.join_next().await {
            let (body_index, body, status) = result??;
            match status {
                StatusCode::CREATED => {
                    created += 1;
                    canonical = Some((body_index, serde_json::from_slice::<Task>(&body)?));
                }
                StatusCode::OK => {
                    replayed += 1;
                    replay_bodies.push((body_index, serde_json::from_slice::<Task>(&body)?));
                }
                StatusCode::CONFLICT => conflicts += 1,
                status => return Err(std::io::Error::other(format!("unexpected {status}")).into()),
            }
        }

        // Then: one body wins durably, its duplicates replay, and the other body always conflicts.
        assert_eq!((created, replayed, conflicts), (1, 7, 8));
        let (winner_index, canonical) = canonical.ok_or("created response missing")?;
        let winning_request = &requests[winner_index];
        assert_eq!(canonical.input_path, winning_request.input_path);
        assert_eq!(canonical.output_path, winning_request.output_path);
        assert_eq!(canonical.workflow, winning_request.workflow);
        assert_eq!(canonical.priority, winning_request.priority);
        assert_eq!(canonical.source, winning_request.source);
        assert_eq!(canonical.source_reference, winning_request.source_reference);
        for (body_index, replay) in replay_bodies {
            assert_eq!(body_index, winner_index);
            assert_eq!(replay, canonical);
        }
        let durable = fixture
            .store
            .task(canonical.id)
            .await?
            .ok_or("durable canonical task missing")?;
        assert_eq!(durable.request, *winning_request);
    }
    let durable_tasks: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks")
        .fetch_one(fixture.store.database().pool())
        .await?;
    assert_eq!(durable_tasks, i64::from(RACE_REPETITIONS));
    eprintln!(
        "task21_intake repetitions={RACE_REPETITIONS} contenders_per_repetition={} durable_tasks={durable_tasks}",
        REQUESTS_PER_BODY * 2
    );
    Ok(())
}
