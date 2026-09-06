use axum::http::StatusCode;
use sha2::{Digest, Sha256};
use sqlx::Acquire;

use crate::domain::{IdempotencyKey, SseEvent, SseEventId};
use crate::persistence::{BatchAdmission, BatchReceipt};

use super::batch::{BatchCreateResponse, BatchPreviewRequest};
use super::error::TaskApiError;
use super::TaskService;

impl TaskService {
    pub(super) async fn create_keyed_batch(
        &self,
        key: IdempotencyKey,
        request: BatchPreviewRequest,
    ) -> Result<(StatusCode, BatchCreateResponse), TaskApiError> {
        let fingerprint: [u8; 32] =
            Sha256::digest(serde_json::to_vec(&request).map_err(|_| TaskApiError::Internal)?)
                .into();
        if let Some(receipt) = self.batch_replay(&key).await? {
            return replay(&receipt, &fingerprint);
        }
        let preview = self.prepare_batch_response(request).await;
        // A concurrent request may have completed while this request scanned files.
        if let Some(receipt) = self.batch_replay(&key).await? {
            return replay(&receipt, &fingerprint);
        }
        let mut response = preview?;
        if response.failed > 0 {
            return Ok((StatusCode::BAD_REQUEST, response));
        }
        let service = self.clone();
        let requests: Vec<_> = response
            .items
            .iter()
            .map(|row| row.request.clone())
            .collect();
        let prepared = tokio::task::spawn_blocking(move || {
            requests
                .into_iter()
                .map(|request| service.prepare_task(request))
                .collect::<Vec<_>>()
        })
        .await
        .map_err(|_| TaskApiError::Internal)?;
        let mut transaction = match self
            .store()
            .begin_batch(&key, &fingerprint)
            .await
            .map_err(|_| TaskApiError::Internal)?
        {
            BatchAdmission::Existing(receipt) => return replay(&receipt, &fingerprint),
            BatchAdmission::Fresh(transaction) => transaction,
        };
        for (row, task) in response.items.iter_mut().zip(&prepared) {
            let result = match task {
                Ok(task) => {
                    let mut savepoint = transaction
                        .begin()
                        .await
                        .map_err(|_| TaskApiError::Internal)?;
                    if let Ok(record) =
                        crate::persistence::insert_batch_task(&mut savepoint, task).await
                    {
                        savepoint
                            .commit()
                            .await
                            .map_err(|_| TaskApiError::Internal)?;
                        Ok(super::mapping::task(record))
                    } else {
                        savepoint
                            .rollback()
                            .await
                            .map_err(|_| TaskApiError::Internal)?;
                        Err(TaskApiError::Internal.into_parts().1)
                    }
                }
                Err(error) => Err(error.clone().into_parts().1),
            };
            match result {
                Ok(task) => {
                    row.task = Some(task);
                    response.created += 1;
                }
                Err(error) => {
                    row.error = Some(error);
                    response.failed += 1;
                }
            }
        }
        let status = if response.failed == 0 {
            StatusCode::CREATED
        } else {
            StatusCode::MULTI_STATUS
        };
        let body = serde_json::to_string(&response).map_err(|_| TaskApiError::Internal)?;
        crate::persistence::finish_batch(transaction, &key, status.as_u16(), &body)
            .await
            .map_err(|_| TaskApiError::Internal)?;
        for (row, prepared) in response.items.iter().zip(&prepared) {
            if let (Some(task), Ok(prepared)) = (&row.task, prepared) {
                crate::logging::task_created(prepared);
                self.events.publish(SseEvent::TaskUpdated {
                    event_id: SseEventId::random(),
                    task: task.clone(),
                });
            }
        }
        Ok((status, response))
    }

    async fn batch_replay(
        &self,
        key: &IdempotencyKey,
    ) -> Result<Option<BatchReceipt>, TaskApiError> {
        self.store()
            .batch_receipt(key)
            .await
            .map_err(|_| TaskApiError::Internal)
    }
}

fn replay(
    receipt: &BatchReceipt,
    fingerprint: &[u8; 32],
) -> Result<(StatusCode, BatchCreateResponse), TaskApiError> {
    if receipt.fingerprint.as_slice() != fingerprint {
        return Err(TaskApiError::Conflict);
    }
    let status = match receipt.status {
        201 => StatusCode::OK,
        207 => StatusCode::MULTI_STATUS,
        _ => return Err(TaskApiError::Internal),
    };
    let response: BatchCreateResponse =
        serde_json::from_str(&receipt.body).map_err(|_| TaskApiError::Internal)?;
    tracing::info!(
        created = response.created,
        failed = response.failed,
        "Batch request replayed"
    );
    Ok((status, response))
}
