#[path = "persistence_atomic.rs"]
mod persistence_atomic;
#[path = "task_api.rs"]
mod task_api;

mod scheduler {
    pub mod support {
        include!("task11/support.rs");
    }
    mod atomic {
        include!("task11/atomic.rs");
    }
    mod scheduling {
        include!("task11/scheduling.rs");
    }
    mod limits {
        include!("task11/scheduler.rs");
    }
}
