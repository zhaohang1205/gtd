use serde::{Deserialize, Serialize};

use crate::model::pomodoro::PomoState;
use crate::model::task::ChecklistItem;

/// Serialized identity of the backup file. `export` always writes `version: 1`;
/// `import` refuses anything it does not understand, so future format changes
/// can bump this instead of silently mis-parsing.
pub const BACKUP_FORMAT: &str = "gtp-backup";
pub const BACKUP_VERSION: u32 = 1;

/// Full-database backup: every `tasks` column (including the ones the active
/// `Task` model no longer maps, like `kind`/`parent_id`), the append-only
/// `task_events` timeline, the tag catalog + associations, settings, and the
/// pomodoro state file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupData {
    pub format: String,
    pub version: u32,
    pub exported_at: i64,
    #[serde(default)]
    pub tasks: Vec<BackupTask>,
    #[serde(default)]
    pub events: Vec<BackupEvent>,
    #[serde(default)]
    pub tags: Vec<BackupTag>,
    #[serde(default)]
    pub task_tags: Vec<BackupTaskTag>,
    #[serde(default)]
    pub settings: Vec<BackupSetting>,
    /// Absent in backups from before the pomo state was included.
    #[serde(default)]
    pub pomodoro: Option<PomoState>,
}

/// A task row with every column of the `tasks` table, so the export is lossless
/// even for fields the current model dropped (kind, parent_id, project_type).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupTask {
    pub id: String,
    pub title: String,
    pub notes: String,
    pub kind: String,
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
    pub delegated_to: Option<String>,
    pub project_type: String,
    pub checklist: Vec<ChecklistItem>,
    pub archive_reason: Option<String>,
}

/// A `task_events` row without its autoincrement `id` (re-assigned on import).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupEvent {
    pub task_id: String,
    pub event_type: String,
    pub from_status: Option<String>,
    pub to_status: Option<String>,
    pub at: i64,
    pub meta: Option<String>,
}

/// A tag row without its integer `id`; tags are reconciled by name on import so
/// ids never conflict across databases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupTag {
    pub name: String,
    pub category: String,
    pub is_system: bool,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub description: Option<String>,
    pub created_at: i64,
}

/// A `task_tags` association, keyed by task id + tag name (not tag id).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupTaskTag {
    pub task_id: String,
    pub tag_name: String,
    pub added_at: i64,
}

/// A `settings` row (key → value).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupSetting {
    pub key: String,
    pub value: String,
}
