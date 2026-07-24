use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TaskEvent {
    pub id: i64,
    pub task_id: String,
    pub event_type: String,
    pub from_status: Option<String>,
    pub to_status: Option<String>,
    pub at: i64,
    pub meta: Option<String>,
}

// Event types (kept as consts so they can't drift from the schema).
pub const EV_CAPTURED: &str = "captured";
pub const EV_CLARIFIED: &str = "clarified";
pub const EV_ORGANIZED: &str = "organized";
pub const EV_STATUS_CHANGED: &str = "status_changed";
pub const EV_SCHEDULED: &str = "scheduled";
pub const EV_DUE: &str = "due";
#[allow(dead_code)]
/// Reserved for the Pomodoro phase (task `started` status), not used in v1.
pub const EV_STARTED: &str = "started";
pub const EV_COMPLETED: &str = "completed";
pub const EV_ARCHIVED: &str = "archived";
pub const EV_TAG_ADDED: &str = "tag_added";
pub const EV_TAG_REMOVED: &str = "tag_removed";
