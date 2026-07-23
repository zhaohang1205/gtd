use serde::Serialize;

/// The seven GTD statuses. `inbox` is the entry point; everything else is a
/// "clarified" state. Transitions are free among these seven by design
/// (you can always re-clarify), except we never allow invalid strings.
pub const STATUSES: &[&str] = &[
    "inbox", "next", "waiting", "scheduled", "someday", "reference", "done",
];

#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub notes: String,
    pub kind: String, // 'action' | 'project'
    pub parent_id: Option<String>,
    pub status: String,
    pub rrule: Option<String>,
    pub created_at: i64,
    pub clarified_at: Option<i64>,
    pub organized_at: Option<i64>,
    pub due_at: Option<i64>,
    pub scheduled_start_at: Option<i64>,
    pub scheduled_end_at: Option<i64>,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub archived_at: Option<i64>,
    pub updated_at: i64,
}

pub fn is_valid_status(s: &str) -> bool {
    STATUSES.contains(&s)
}

/// Free transitions among the seven GTD states. Anything else is rejected.
pub fn can_transition(from: &str, to: &str) -> bool {
    is_valid_status(from) && is_valid_status(to) && from != to
}
