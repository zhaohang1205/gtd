use rusqlite::Connection;
use uuid::Uuid;

use anyhow::Result;
use crate::error::Error;
use crate::model::event;
use crate::model::task::{self, Task};
use crate::repo::log_event;
use crate::time;

/// Input for creating a task (capture / project).
pub struct CaptureInput {
    pub title: String,
    pub kind: task::TaskKind, // 'action' | 'project'
    pub parent_id: Option<String>,
    pub status: task::Status,
    pub due_at: Option<i64>,
    pub tag_names: Vec<String>,
    pub delegated_to: Option<String>,
    pub project_type: Option<task::ProjectType>,
    pub checklist: Vec<task::ChecklistItem>,
}

impl Default for CaptureInput {
    fn default() -> Self {
        Self {
            title: String::new(),
            kind: task::TaskKind::Action,
            parent_id: None,
            status: task::Status::Inbox,
            due_at: None,
            tag_names: Vec::new(),
            delegated_to: None,
            project_type: None,
            checklist: Vec::new(),
        }
    }
}

pub fn get(conn: &Connection, id: &str) -> Result<Task> {
    let mut stmt = conn.prepare(
        "SELECT id,title,notes,kind,parent_id,status,rrule,created_at,clarified_at,organized_at,\
                due_at,scheduled_start_at,scheduled_end_at,started_at,completed_at,archived_at,updated_at,\
                delegated_to,project_type,checklist \
         FROM tasks WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map([id], |r| row_to_task(r))?;
    rows.next()
        .transpose()?
        .ok_or_else(|| Error::TaskNotFound(id.to_string()).into())
}

pub fn create_capture(conn: &Connection, input: &CaptureInput) -> Result<Task> {
    let id = Uuid::new_v4().to_string();
    let now = time::now_ms();
    let status = input.status;
    let clarified = if status != task::Status::Inbox {
        Some(now)
    } else {
        None
    };
    let organized = if input.parent_id.is_some() {
        Some(now)
    } else {
        None
    };

    let pt_str = input.project_type.unwrap_or(task::ProjectType::Parallel).to_string();
    let cl_str = serde_json::to_string(&input.checklist).unwrap_or_else(|_| "[]".to_string());

    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO tasks \
         (id,title,notes,kind,parent_id,status,rrule,created_at,clarified_at,organized_at,due_at,updated_at,delegated_to,project_type,checklist) \
         VALUES (?1,?2,'',?3,?4,?5,NULL,?6,?7,?8,?9,?10,?11,?12,?13)",
        rusqlite::params![
            id,
            input.title,
            input.kind.to_string(),
            input.parent_id,
            status.to_string(),
            now,
            clarified,
            organized,
            input.due_at,
            now,
            input.delegated_to,
            pt_str,
            cl_str
        ],
    )?;
    let status_str = status.to_string();
    log_event(&tx, &id, event::EV_CAPTURED, None, Some(&status_str), None)?;
    if clarified.is_some() {
        log_event(&tx, &id, event::EV_CLARIFIED, None, Some(&status_str), None)?;
    }
    if organized.is_some() {
        log_event(&tx, &id, event::EV_ORGANIZED, None, Some(&status_str), None)?;
    }

    for tag in &input.tag_names {
        crate::repo::tags::add_tag_to_task_inner(&tx, &id, tag)?;
    }
    tx.commit()?;
    get(conn, &id)
}

/// Resolve a task reference to its full id: exact match, else a unique id
/// prefix (like git), else `TaskNotFound`.
pub fn resolve_id(conn: &Connection, key: &str) -> Result<String> {
    if get(conn, key).is_ok() {
        return Ok(key.to_string());
    }
    let mut stmt = conn.prepare("SELECT id FROM tasks WHERE id LIKE ?1 || '%' LIMIT 2")?;
    let mut rows = stmt.query_map([key], |r| r.get::<usize, String>(0))?;
    let first = rows.next().transpose()?;
    let second = rows.next().transpose()?;
    match (first, second) {
        (Some(id), None) => Ok(id),
        (Some(_), Some(_)) => anyhow::bail!("ambiguous id prefix: {}", key),
        (None, _) => Err(Error::TaskNotFound(key.to_string()).into()),
    }
}

pub fn rename(conn: &Connection, id: &str, new_title: &str) -> Result<Task> {
    let now = time::now_ms();
    conn.execute(
        "UPDATE tasks SET title=?1, updated_at=?2 WHERE id=?3",
        rusqlite::params![new_title, now, id],
    )?;
    get(conn, id)
}

pub fn update_checklist(conn: &Connection, id: &str, checklist: &Vec<task::ChecklistItem>) -> Result<Task> {
    let now = time::now_ms();
    let cl_str = serde_json::to_string(checklist).unwrap_or_else(|_| "[]".to_string());
    conn.execute(
        "UPDATE tasks SET checklist=?1, updated_at=?2 WHERE id=?3",
        rusqlite::params![cl_str, now, id],
    )?;
    get(conn, id)
}

/// Transition a task from its current status to `to_status`,
/// updating the relevant timestamp fields (time datafication).
pub fn transition(conn: &Connection, id: &str, to_status: task::Status) -> Result<Task> {
    let mut t = get(conn, id)?;
    let from = t.status;
    if from == to_status {
        return Err(Error::InvalidTransition {
            from: from.to_string(),
            to: to_status.to_string(),
        }
        .into());
    }
    let now = time::now_ms();
    if from == task::Status::Inbox && t.clarified_at.is_none() {
        t.clarified_at = Some(now);
    }

    if t.kind == task::TaskKind::Project && t.started_at.is_none() && to_status != task::Status::Inbox && to_status != task::Status::Someday {
        t.started_at = Some(now);
    }

    let tx = conn.unchecked_transaction()?;

    if to_status == task::Status::Done && t.rrule.is_some() {
        if let Some(start) = t.scheduled_start_at {
            if let Ok(occ) = time::rrule_occurrences(t.rrule.as_ref().unwrap(), start, 366) {
                if let Some(next) = occ.into_iter().find(|m| *m > start) {
                    let duration = t.scheduled_end_at.unwrap_or(start) - start;
                    
                    log_event(&tx, id, "habit_completed", Some(&from.to_string()), Some(&task::Status::Done.to_string()), None)?;

                    t.scheduled_start_at = Some(next);
                    t.scheduled_end_at = Some(next + duration);
                    t.status = task::Status::Scheduled;
                    t.updated_at = now;
                    
                    tx.execute(
                        "UPDATE tasks SET status=?1, clarified_at=?2, completed_at=?3, updated_at=?4, started_at=?5, scheduled_start_at=?6, scheduled_end_at=?7 WHERE id=?8",
                        rusqlite::params![t.status.to_string(), t.clarified_at, t.completed_at, t.updated_at, t.started_at, t.scheduled_start_at, t.scheduled_end_at, id],
                    )?;
                    tx.commit()?;
                    return Ok(t);
                }
            }
        }
    }

    if to_status == task::Status::Done && t.completed_at.is_none() {
        t.completed_at = Some(now);
    }
    t.status = to_status;
    t.updated_at = now;

    tx.execute(
        "UPDATE tasks SET status=?1, clarified_at=?2, completed_at=?3, updated_at=?4, started_at=?5, scheduled_start_at=?6, scheduled_end_at=?7 WHERE id=?8",
        rusqlite::params![t.status.to_string(), t.clarified_at, t.completed_at, t.updated_at, t.started_at, t.scheduled_start_at, t.scheduled_end_at, id],
    )?;
    let ev = if to_status == task::Status::Done {
        event::EV_COMPLETED
    } else {
        event::EV_STATUS_CHANGED
    };
    let from_str = from.to_string();
    let to_str = to_status.to_string();
    log_event(&tx, id, ev, Some(&from_str), Some(&to_str), None)?;
    tx.commit()?;
    Ok(t)
}

/// Attach a task to a project (set `parent_id`) and record an `organized` event.
pub fn assign_project(conn: &Connection, id: &str, project_id: &str) -> Result<Task> {
    let _ = get(conn, id)?;
    let now = time::now_ms();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE tasks SET parent_id=?1, organized_at=?2, updated_at=?3 WHERE id=?4",
        rusqlite::params![project_id, now, now, id],
    )?;
    log_event(&tx, id, event::EV_ORGANIZED, None, None, None)?;
    tx.commit()?;
    get(conn, id)
}

/// Set a soft deadline (`due_at`) without changing the task status. Used by the
/// inbox→next planning hook so a next action keeps its status while gaining a due.
pub fn set_due(conn: &Connection, id: &str, due_ms: i64) -> Result<Task> {
    let mut t = get(conn, id)?;
    t.due_at = Some(due_ms);
    t.updated_at = time::now_ms();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE tasks SET due_at=?1, updated_at=?2 WHERE id=?3",
        rusqlite::params![t.due_at, t.updated_at, id],
    )?;
    log_event(&tx, id, event::EV_DUE, None, None, None)?;
    tx.commit()?;
    get(conn, id)
}

/// Schedule a task: set planned start/end + optional recurrence, move to
/// `scheduled`, and record a `scheduled` event.
pub fn schedule(
    conn: &Connection,
    id: &str,
    start_ms: i64,
    end_ms: Option<i64>,
    rrule: Option<String>,
) -> Result<Task> {
    let mut t = get(conn, id)?;
    let from = t.status;
    let now = time::now_ms();
    if from == task::Status::Inbox && t.clarified_at.is_none() {
        t.clarified_at = Some(now);
    }
    t.scheduled_start_at = Some(start_ms);
    t.scheduled_end_at = end_ms;
    t.rrule = rrule.clone();
    t.status = task::Status::Scheduled;
    t.updated_at = now;
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE tasks SET status=?1, clarified_at=?2, scheduled_start_at=?3, scheduled_end_at=?4, rrule=?5, updated_at=?6 WHERE id=?7",
        rusqlite::params![
            t.status.to_string(),
            t.clarified_at,
            t.scheduled_start_at,
            t.scheduled_end_at,
            t.rrule,
            t.updated_at,
            id
        ],
    )?;
    let meta = rrule.as_deref().map(|r| format!("{{\"rrule\":\"{}\"}}", r));
    let from_str = from.to_string();
    log_event(
        &tx,
        id,
        event::EV_SCHEDULED,
        Some(&from_str),
        Some("scheduled"),
        meta.as_deref(),
    )?;
    tx.commit()?;
    Ok(t)
}

pub fn archive(conn: &Connection, id: &str) -> Result<Task> {
    let _ = get(conn, id)?;
    let now = time::now_ms();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE tasks SET archived_at=?1, updated_at=?2 WHERE id=?3",
        rusqlite::params![now, now, id],
    )?;
    log_event(&tx, id, event::EV_ARCHIVED, None, None, None)?;
    tx.commit()?;
    get(conn, id)
}

pub struct ListFilter {
    pub status: Option<task::Status>,
    pub project: Option<String>, // project id or name
    pub tags: Vec<String>,
    pub query: Option<String>,
}

pub fn list(conn: &Connection, f: &ListFilter) -> Result<Vec<Task>> {
    let mut sql = String::from(
        "SELECT id,title,notes,kind,parent_id,status,rrule,created_at,clarified_at,organized_at,\
                due_at,scheduled_start_at,scheduled_end_at,started_at,completed_at,archived_at,updated_at,\
                delegated_to,project_type,checklist \
         FROM tasks WHERE archived_at IS NULL",
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(s) = &f.status {
        sql.push_str(" AND status = ?");
        params.push(Box::new(s.to_string()));
    }
    if let Some(p) = &f.project {
        let pid = resolve_project(conn, p)?;
        sql.push_str(" AND parent_id = ?");
        params.push(Box::new(pid));
    }
    for tag in &f.tags {
        sql.push_str(
            " AND id IN (SELECT task_id FROM task_tags tt JOIN tags g ON g.id=tt.tag_id WHERE g.name = ?)",
        );
        params.push(Box::new(tag.clone()));
    }
    if let Some(q) = &f.query {
        sql.push_str(" AND (title LIKE ? OR notes LIKE ?)");
        let like_q = format!("%{}%", q);
        params.push(Box::new(like_q.clone()));
        params.push(Box::new(like_q));
    }
    sql.push_str(
        " ORDER BY (scheduled_start_at IS NOT NULL) DESC, scheduled_start_at ASC, due_at ASC, created_at ASC",
    );

    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(param_refs.as_slice(), |r| row_to_task(r))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn count(conn: &Connection, f: &ListFilter) -> Result<usize> {
    let mut sql = String::from("SELECT COUNT(*) FROM tasks WHERE archived_at IS NULL");
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(s) = &f.status {
        sql.push_str(" AND status = ?");
        params.push(Box::new(s.to_string()));
    }
    if let Some(p) = &f.project {
        let pid = resolve_project(conn, p)?;
        sql.push_str(" AND parent_id = ?");
        params.push(Box::new(pid));
    }
    for tag in &f.tags {
        sql.push_str(
            " AND id IN (SELECT task_id FROM task_tags tt JOIN tags g ON g.id=tt.tag_id WHERE g.name = ?)",
        );
        params.push(Box::new(tag.clone()));
    }
    if let Some(q) = &f.query {
        sql.push_str(" AND (title LIKE ? OR notes LIKE ?)");
        let like_q = format!("%{}%", q);
        params.push(Box::new(like_q.clone()));
        params.push(Box::new(like_q));
    }
    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let c: usize = stmt.query_row(param_refs.as_slice(), |r| r.get(0))?;
    Ok(c)
}

/// Resolve a project reference (id, id-prefix, or title) to its project id.
pub fn resolve_project(conn: &Connection, key: &str) -> Result<String> {
    // try as task id (exact or unique prefix)
    if let Ok(id) = resolve_id(conn, key) {
        let t = get(conn, &id)?;
        if t.kind == task::TaskKind::Project {
            return Ok(t.id);
        }
    }
    // try by title
    let mut stmt = conn.prepare("SELECT id FROM tasks WHERE kind='project' AND title = ?1 LIMIT 1")?;
    let mut rows = stmt.query_map([key], |r| r.get::<usize, String>(0))?;
    if let Some(id) = rows.next().transpose()? {
        return Ok(id);
    }
    Err(Error::ProjectNotFound(key.to_string()).into())
}

pub fn events(conn: &Connection, task_id: &str) -> Result<Vec<event::TaskEvent>> {
    let mut stmt = conn.prepare(
        "SELECT id,task_id,event_type,from_status,to_status,at,meta \
         FROM task_events WHERE task_id = ?1 ORDER BY at ASC",
    )?;
    let rows = stmt.query_map([task_id], |r| {
        Ok(event::TaskEvent {
            id: r.get(0)?,
            task_id: r.get(1)?,
            event_type: r.get(2)?,
            from_status: r.get(3)?,
            to_status: r.get(4)?,
            at: r.get(5)?,
            meta: r.get(6)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

fn row_to_task(r: &rusqlite::Row) -> rusqlite::Result<Task> {
    let kind_str: String = r.get(3)?;
    let status_str: String = r.get(5)?;
    let delegated_to: Option<String> = r.get(17)?;
    let pt_str: String = r.get(18)?;
    let cl_str: String = r.get(19)?;

    Ok(Task {
        id: r.get(0)?,
        title: r.get(1)?,
        notes: r.get(2)?,
        kind: kind_str.parse().unwrap_or(crate::model::task::TaskKind::Action),
        parent_id: r.get(4)?,
        status: status_str.parse().unwrap_or(crate::model::task::Status::Inbox),
        rrule: r.get(6)?,
        created_at: r.get(7)?,
        clarified_at: r.get(8)?,
        organized_at: r.get(9)?,
        due_at: r.get(10)?,
        scheduled_start_at: r.get(11)?,
        scheduled_end_at: r.get(12)?,
        started_at: r.get(13)?,
        completed_at: r.get(14)?,
        archived_at: r.get(15)?,
        updated_at: r.get(16)?,
        delegated_to,
        project_type: pt_str.parse().unwrap_or_default(),
        checklist: serde_json::from_str(&cl_str).unwrap_or_default(),
    })
}
