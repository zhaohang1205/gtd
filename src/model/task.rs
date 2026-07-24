use serde::{Serialize, Deserialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Inbox,
    Next,
    Waiting,
    Scheduled,
    Someday,
    Reference,
    Done,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Status::Inbox => "inbox",
            Status::Next => "next",
            Status::Waiting => "waiting",
            Status::Scheduled => "scheduled",
            Status::Someday => "someday",
            Status::Reference => "reference",
            Status::Done => "done",
        };
        write!(f, "{}", s)
    }
}

impl FromStr for Status {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "inbox" => Ok(Status::Inbox),
            "next" => Ok(Status::Next),
            "waiting" => Ok(Status::Waiting),
            "scheduled" => Ok(Status::Scheduled),
            "someday" => Ok(Status::Someday),
            "reference" => Ok(Status::Reference),
            "done" => Ok(Status::Done),
            _ => Err(format!("Invalid status: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskKind {
    Action,
    Project,
}

impl fmt::Display for TaskKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            TaskKind::Action => "action",
            TaskKind::Project => "project",
        };
        write!(f, "{}", s)
    }
}

impl FromStr for TaskKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "action" => Ok(TaskKind::Action),
            "project" => Ok(TaskKind::Project),
            _ => Err(format!("Invalid kind: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectType {
    Parallel,
    Sequential,
}

impl Default for ProjectType {
    fn default() -> Self {
        ProjectType::Parallel
    }
}

impl fmt::Display for ProjectType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ProjectType::Parallel => "parallel",
            ProjectType::Sequential => "sequential",
        };
        write!(f, "{}", s)
    }
}

impl FromStr for ProjectType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "parallel" => Ok(ProjectType::Parallel),
            "sequential" => Ok(ProjectType::Sequential),
            _ => Err(format!("Invalid project type: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecklistItem {
    pub id: String,
    pub title: String,
    pub done: bool,
}



#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub notes: String,
    pub kind: TaskKind,
    pub parent_id: Option<String>,
    pub status: Status,
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
    pub project_type: ProjectType,
    pub checklist: Vec<ChecklistItem>,
}
