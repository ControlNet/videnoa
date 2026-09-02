use sqlx::{QueryBuilder, Row, Sqlite};

use crate::domain::{SortDirection, TaskListQuery, TaskSortField, TaskStatus};

use super::codec::{failure_stage, task_source, task_status};
use super::models::{PageResult, TaskRecord};
use super::task_row::{map_task, TASK_COLUMNS};
use super::{PersistenceError, Store};

impl Store {
    /// # Errors
    /// Returns an error when `SQLite` access or persisted decoding fails.
    pub async fn task_page(
        &self,
        query: &TaskListQuery,
    ) -> Result<PageResult<TaskRecord>, PersistenceError> {
        let total = self.task_count(query).await?;
        let mut builder = page_builder(query, false)?;
        let rows = builder.build().fetch_all(self.database.pool()).await?;
        let items = rows.iter().map(map_task).collect::<Result<Vec<_>, _>>()?;
        Ok(PageResult { items, total })
    }

    /// # Errors
    /// Returns an error when `SQLite` access or count conversion fails.
    pub async fn task_count(&self, query: &TaskListQuery) -> Result<u64, PersistenceError> {
        let mut builder = QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM tasks WHERE 1 = 1");
        push_filters(&mut builder, query);
        let count: i64 = builder
            .build()
            .fetch_one(self.database.pool())
            .await?
            .try_get(0)?;
        super::codec::rust_u64("task_count", count)
    }

    /// # Errors
    /// Returns an error when `SQLite` cannot explain the bounded query.
    pub async fn explain_task_page(
        &self,
        query: &TaskListQuery,
    ) -> Result<Vec<String>, PersistenceError> {
        let mut builder = page_builder(query, true)?;
        let rows = builder.build().fetch_all(self.database.pool()).await?;
        rows.iter()
            .map(|row| row.try_get("detail").map_err(PersistenceError::from))
            .collect()
    }

    /// # Errors
    /// Returns an error when `SQLite` cannot explain the queue query.
    pub async fn explain_queue_plan(&self) -> Result<Vec<String>, PersistenceError> {
        plan(
            self,
            "EXPLAIN QUERY PLAN SELECT id FROM tasks
             WHERE status = 'queued'
             ORDER BY priority DESC, created_at_ms ASC, id ASC LIMIT 100",
        )
        .await
    }

    /// # Errors
    /// Returns an error when `SQLite` cannot explain the recovery query.
    pub async fn explain_recovery_plan(&self) -> Result<Vec<String>, PersistenceError> {
        plan(
            self,
            "EXPLAIN QUERY PLAN SELECT id FROM tasks
             WHERE status NOT IN ('completed', 'failed', 'cancelled')
             ORDER BY updated_at_ms ASC, id ASC LIMIT 100",
        )
        .await
    }

    /// # Errors
    /// Returns an error when `SQLite` access or count conversion fails.
    pub async fn task_status_count(&self, status: TaskStatus) -> Result<u64, PersistenceError> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE status = ?")
            .bind(task_status(status))
            .fetch_one(self.database.pool())
            .await?;
        super::codec::rust_u64("task_status_count", count)
    }
}

async fn plan(store: &Store, sql: &str) -> Result<Vec<String>, PersistenceError> {
    sqlx::query(sql)
        .fetch_all(store.database.pool())
        .await?
        .iter()
        .map(|row| row.try_get("detail").map_err(PersistenceError::from))
        .collect()
}

fn page_builder(
    query: &TaskListQuery,
    explain: bool,
) -> Result<QueryBuilder<'static, Sqlite>, PersistenceError> {
    let prefix = if explain {
        format!("EXPLAIN QUERY PLAN SELECT {TASK_COLUMNS} FROM tasks WHERE 1 = 1")
    } else {
        format!("SELECT {TASK_COLUMNS} FROM tasks WHERE 1 = 1")
    };
    let mut builder = QueryBuilder::new(prefix);
    push_filters(&mut builder, query);
    push_order(&mut builder, query.sort, query.direction);
    builder
        .push(" LIMIT ")
        .push_bind(i64::from(query.page.limit().get()))
        .push(" OFFSET ")
        .push_bind(super::codec::sqlite_u64(
            "page_offset",
            query.page.offset().get(),
        )?);
    Ok(builder)
}

fn push_filters(builder: &mut QueryBuilder<'static, Sqlite>, query: &TaskListQuery) {
    if let Some(status) = query.status {
        builder
            .push(" AND status = ")
            .push_bind(task_status(status));
    }
    if let Some(worker_id) = query.worker_id {
        builder
            .push(" AND worker_id = ")
            .push_bind(worker_id.to_string());
    }
    if let Some(workflow) = &query.workflow {
        builder
            .push(" AND workflow = ")
            .push_bind(workflow.as_str().to_owned());
    }
    if let Some(source) = query.source {
        builder
            .push(" AND source = ")
            .push_bind(task_source(source));
    }
    if let Some(stage) = query.failure_stage {
        builder
            .push(" AND failure_stage = ")
            .push_bind(failure_stage(stage));
    }
    if let Some(search) = query.search.as_deref() {
        let pattern = format!("%{}%", escape_like(search));
        builder
            .push(" AND (input_path LIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\' OR output_path LIKE ")
            .push_bind(pattern)
            .push(" ESCAPE '\\')");
    }
}

fn push_order(
    builder: &mut QueryBuilder<'static, Sqlite>,
    sort: TaskSortField,
    direction: SortDirection,
) {
    let direction = match direction {
        SortDirection::Asc => " ASC",
        SortDirection::Desc => " DESC",
    };
    match sort {
        TaskSortField::Priority => {
            builder.push(" ORDER BY priority").push(direction);
            builder.push(", created_at_ms ASC, id ASC");
        }
        TaskSortField::CreatedAt => order(builder, "created_at_ms", direction),
        TaskSortField::CompletedAt => order(builder, "completed_at_ms", direction),
        TaskSortField::Status => order(builder, "status", direction),
        TaskSortField::Worker => order(builder, "worker_id", direction),
        TaskSortField::Duration => order(
            builder,
            "(COALESCE(completed_at_ms, updated_at_ms) - created_at_ms)",
            direction,
        ),
    }
}

fn order(builder: &mut QueryBuilder<'static, Sqlite>, field: &'static str, direction: &str) {
    builder
        .push(" ORDER BY ")
        .push(field)
        .push(direction)
        .push(", id ASC");
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}
