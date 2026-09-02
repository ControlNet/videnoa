use chrono::{DateTime, Utc};

use crate::domain::{
    WorkerCapacity, WorkerCreateRequest, WorkerId, WorkerName, WorkerUpdateRequest,
};
use crate::persistence::{
    CasOutcome, NewWorker, PersistenceError, Store, WorkerDeleteOutcome, WorkerHealthUpdate,
    WorkerIdentityConflict, WorkerRecord, WorkerUpdate, WorkerUpdateOutcome,
};

use super::WorkerRegistryError;

#[derive(Clone, Debug)]
pub struct WorkerRegistry {
    store: Store,
}

impl WorkerRegistry {
    #[must_use]
    pub const fn new(store: Store) -> Self {
        Self { store }
    }

    /// Creates an offline worker whose health must be refreshed before scheduling.
    ///
    /// # Errors
    /// Returns a typed duplicate, validation, or persistence error.
    pub async fn create(
        &self,
        request: WorkerCreateRequest,
        now: DateTime<Utc>,
    ) -> Result<WorkerRecord, WorkerRegistryError> {
        let name = normalized_name(&request.name)?;
        let worker = NewWorker {
            id: WorkerId::random(),
            name,
            api_url: request.api_url,
            enabled: request.enabled,
            online: false,
            compute_slots: request.compute_slots,
            created_at: now,
        };
        match self.store.insert_worker(&worker).await {
            Ok(()) => self.load(worker.id).await,
            Err(error) if unique_violation(&error) => Err(self
                .identity_error(None, &worker.name, &worker.api_url)
                .await?),
            Err(error) => Err(error.into()),
        }
    }

    /// Loads one durable worker record.
    ///
    /// # Errors
    /// Returns an error when `SQLite` cannot load the worker.
    pub async fn worker(&self, id: WorkerId) -> Result<Option<WorkerRecord>, WorkerRegistryError> {
        self.store.worker(id).await.map_err(Into::into)
    }

    /// Updates worker policy using optimistic concurrency.
    ///
    /// # Errors
    /// Returns typed stale, duplicate, capacity, not-found, or persistence errors.
    pub async fn update(
        &self,
        id: WorkerId,
        request: WorkerUpdateRequest,
        now: DateTime<Utc>,
    ) -> Result<WorkerRecord, WorkerRegistryError> {
        let current = self
            .worker(id)
            .await?
            .ok_or(WorkerRegistryError::NotFound)?;
        if request.version != current.version {
            return Err(WorkerRegistryError::Conflict);
        }
        let update = WorkerUpdate {
            id,
            expected_version: request.version,
            name: normalized_name(&request.name)?,
            api_url: request.api_url,
            enabled: request.enabled,
            compute_slots: request.compute_slots,
            updated_at: now,
        };
        match self.store.update_worker(&update).await {
            Ok(WorkerUpdateOutcome::Applied { .. }) => self.load(id).await,
            Ok(WorkerUpdateOutcome::Conflict) => Err(WorkerRegistryError::Conflict),
            Ok(WorkerUpdateOutcome::CapacityBelowUsage) => {
                Err(WorkerRegistryError::CapacityBelowUsage)
            }
            Err(error) if unique_violation(&error) => Err(self
                .identity_error(Some(id), &update.name, &update.api_url)
                .await?),
            Err(error) => Err(error.into()),
        }
    }

    /// Changes only the worker enabled policy while preserving assignments.
    ///
    /// # Errors
    /// Returns typed stale, not-found, or persistence errors.
    pub async fn set_enabled(
        &self,
        id: WorkerId,
        expected_version: u64,
        enabled: bool,
        now: DateTime<Utc>,
    ) -> Result<WorkerRecord, WorkerRegistryError> {
        let current = self
            .worker(id)
            .await?
            .ok_or(WorkerRegistryError::NotFound)?;
        self.update(
            id,
            WorkerUpdateRequest {
                version: expected_version,
                name: current.name,
                api_url: current.api_url,
                enabled,
                compute_slots: current.compute_slots,
            },
            now,
        )
        .await
    }

    /// Atomically refreshes worker health and compatible capabilities.
    ///
    /// # Errors
    /// Returns typed stale, not-found, or persistence errors.
    pub async fn refresh_health(
        &self,
        update: WorkerHealthUpdate,
    ) -> Result<WorkerRecord, WorkerRegistryError> {
        if self.worker(update.id).await?.is_none() {
            return Err(WorkerRegistryError::NotFound);
        }
        match self.store.update_worker_health(&update).await? {
            CasOutcome::Applied { .. } => self.load(update.id).await,
            CasOutcome::Conflict => Err(WorkerRegistryError::Conflict),
        }
    }

    /// Deletes a worker only when its version is current and no task references it.
    ///
    /// # Errors
    /// Returns typed stale, referenced, not-found, or persistence errors.
    pub async fn delete(
        &self,
        id: WorkerId,
        expected_version: u64,
    ) -> Result<(), WorkerRegistryError> {
        match self.store.delete_worker(id, expected_version).await? {
            WorkerDeleteOutcome::Deleted => Ok(()),
            WorkerDeleteOutcome::NotFound => Err(WorkerRegistryError::NotFound),
            WorkerDeleteOutcome::Conflict => Err(WorkerRegistryError::Conflict),
            WorkerDeleteOutcome::Referenced => Err(WorkerRegistryError::Referenced),
        }
    }

    /// Computes current capacity from durable task assignments.
    ///
    /// # Errors
    /// Returns an error when the worker is absent or capacity cannot be loaded.
    pub async fn capacity(&self, id: WorkerId) -> Result<WorkerCapacity, WorkerRegistryError> {
        if self.worker(id).await?.is_none() {
            return Err(WorkerRegistryError::NotFound);
        }
        self.store.worker_capacity(id).await.map_err(Into::into)
    }

    async fn load(&self, id: WorkerId) -> Result<WorkerRecord, WorkerRegistryError> {
        self.worker(id).await?.ok_or(WorkerRegistryError::NotFound)
    }

    async fn identity_error(
        &self,
        id: Option<WorkerId>,
        name: &WorkerName,
        api_url: &crate::domain::WorkerApiUrl,
    ) -> Result<WorkerRegistryError, WorkerRegistryError> {
        match self
            .store
            .worker_identity_conflict(id, name, api_url)
            .await?
        {
            Some(WorkerIdentityConflict::Name) => Ok(WorkerRegistryError::DuplicateName),
            Some(WorkerIdentityConflict::ApiUrl) => Ok(WorkerRegistryError::DuplicateApiUrl),
            None => Ok(WorkerRegistryError::Conflict),
        }
    }
}

fn normalized_name(name: &WorkerName) -> Result<WorkerName, WorkerRegistryError> {
    let value = name.as_str().trim();
    if value.is_empty() {
        return Err(WorkerRegistryError::InvalidName);
    }
    Ok(WorkerName::new(value))
}

fn unique_violation(error: &PersistenceError) -> bool {
    matches!(error, PersistenceError::Database(sqlx::Error::Database(error)) if error.is_unique_violation())
}
