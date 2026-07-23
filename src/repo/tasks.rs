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
    pub kind: String, // 'action' | 'project'
    pub parent_id: Option<String>,
    pub status: String,
    pub due_at: Option<i64>,
    pub tag_names: Vec<String>,
}

pub fn get(conn: &Connection, id: &str) -> Result<Task> {
    let mut stmt = conn.prepare(
        "SELECT id,title,notes,kind,parent_id,status,rrule,created_at,clarified_at,organized_at,\
                due_at,scheduled_start_at,scheduled_end_at,started_at,completed_at,archived_at,updated_at \
         FROM tasks WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map([id], |r| Ok(row_to_task(r)))?;
    rows.next()
        .transpose()?
        .ok_or_else(|| Error::TaskNotFound(id.to_string()).into())
}

pub fn create_capture(conn: &Connection, input: &CaptureInput) -> Result<Task> {
    let id = Uuid::new_v4().to_string();
    let now = time::now_ms();
    let status = input.status.clone();
    let clarified = if status != "inbox" {
        Some(now)
    } else {
        None
    };
    let organized = if input.parent_id.is_some() {
        Some(now)
    } else {
        None
    };

    conn.execute(
        "INSERT INTO tasks \
         (id,title,notes,kind,parent_id,status,rrule,created_at,clarified_at,organized_at,due_at,updated_at) \
         VALUES (?1,?2,'',?3,?4,?5,NULL,?6,?7,?8,?9,?10)",
        rusqlite::params![
            id,
            input.title,
            input.kind,
            input.parent_id,
            status,
            now,
            clarified,
            organized,
            input.due_at,
            now
        ],
    )?;
    log_event(conn, &id, event::EV_CAPTURED, None, Some(&status), None)?;
    if clarified.is_some() {
        log_event(conn, &id, event::EV_CLARIFIED, None, Some(&status), None)?;
    }
    if organized.is_some() {
        log_event(conn, &id, event::EV_ORGANIZED, None, Some(&status), None)?;
    }

    for tag in &input.tag_names {
        crate::repo::tags::add_tag_to_task(conn, &id, tag)?;
    }
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

/// Move a task to `to_status`, recording the transition in the event log and
/// updating the relevant timestamp fields (time datafication).
pub fn transition(conn: &Connection, id: &str, to_status: &str) -> Result<Task> {
    if !task::is_valid_status(to_status) {
        return Err(Error::InvalidStatus(to_status.to_string()).into());
    }
    let mut t = get(conn, id)?;
    let from = t.status.clone();
    if from == to_status {
        return Err(Error::InvalidTransition {
            from,
            to: to_status.to_string(),
        }
        .into());
    }
    if !task::can_transition(&from, to_status) {
        return Err(Error::InvalidTransition {
            from,
            to: to_status.to_string(),
        }
        .into());
    }
    let now = time::now_ms();
    if from == "inbox" && t.clarified_at.is_none() {
        t.clarified_at = Some(now);
    }
    if to_status == "done" && t.completed_at.is_none() {
        t.completed_at = Some(now);
    }
    t.status = to_status.to_string();
    t.updated_at = now;

    conn.execute(
        "UPDATE tasks SET status=?1, clarified_at=?2, completed_at=?3, updated_at=?4 WHERE id=?5",
        rusqlite::params![t.status, t.clarified_at, t.completed_at, t.updated_at, id],
    )?;
    let ev = if to_status == "done" {
        event::EV_COMPLETED
    } else {
        event::EV_STATUS_CHANGED
    };
    log_event(conn, id, ev, Some(&from), Some(to_status), None)?;
    Ok(t)
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
    let from = t.status.clone();
    let now = time::now_ms();
    if from == "inbox" && t.clarified_at.is_none() {
        t.clarified_at = Some(now);
    }
    t.scheduled_start_at = Some(start_ms);
    t.scheduled_end_at = end_ms;
    t.rrule = rrule.clone();
    t.status = "scheduled".to_string();
    t.updated_at = now;
    conn.execute(
        "UPDATE tasks SET status=?1, clarified_at=?2, scheduled_start_at=?3, scheduled_end_at=?4, rrule=?5, updated_at=?6 WHERE id=?7",
        rusqlite::params![
            t.status,
            t.clarified_at,
            t.scheduled_start_at,
            t.scheduled_end_at,
            t.rrule,
            t.updated_at,
            id
        ],
    )?;
    let meta = rrule.as_deref().map(|r| format!("{{\"rrule\":\"{}\"}}", r));
    log_event(
        conn,
        id,
        event::EV_SCHEDULED,
        Some(&from),
        Some("scheduled"),
        meta.as_deref(),
    )?;
    Ok(t)
}

pub fn archive(conn: &Connection, id: &str) -> Result<Task> {
    let _ = get(conn, id)?;
    let now = time::now_ms();
    conn.execute(
        "UPDATE tasks SET archived_at=?1, updated_at=?2 WHERE id=?3",
        rusqlite::params![now, now, id],
    )?;
    log_event(conn, id, event::EV_ARCHIVED, None, None, None)?;
    get(conn, id)
}

pub struct ListFilter {
    pub status: Option<String>,
    pub project: Option<String>, // project id or name
    pub tags: Vec<String>,
}

pub fn list(conn: &Connection, f: &ListFilter) -> Result<Vec<Task>> {
    let mut sql = String::from(
        "SELECT id,title,notes,kind,parent_id,status,rrule,created_at,clarified_at,organized_at,\
                due_at,scheduled_start_at,scheduled_end_at,started_at,completed_at,archived_at,updated_at \
         FROM tasks WHERE archived_at IS NULL",
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(s) = &f.status {
        sql.push_str(" AND status = ?");
        params.push(Box::new(s.clone()));
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
    sql.push_str(
        " ORDER BY (scheduled_start_at IS NOT NULL) DESC, scheduled_start_at ASC, due_at ASC, created_at ASC",
    );

    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(param_refs.as_slice(), |r| Ok(row_to_task(r)))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Resolve a project reference (id, id-prefix, or title) to its project id.
pub fn resolve_project(conn: &Connection, key: &str) -> Result<String> {
    // try as task id (exact or unique prefix)
    if let Ok(id) = resolve_id(conn, key) {
        let t = get(conn, &id)?;
        if t.kind == "project" {
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

fn row_to_task(r: &rusqlite::Row) -> Task {
    Task {
        id: r.get(0).unwrap(),
        title: r.get(1).unwrap(),
        notes: r.get(2).unwrap(),
        kind: r.get(3).unwrap(),
        parent_id: r.get(4).unwrap(),
        status: r.get(5).unwrap(),
        rrule: r.get(6).unwrap(),
        created_at: r.get(7).unwrap(),
        clarified_at: r.get(8).unwrap(),
        organized_at: r.get(9).unwrap(),
        due_at: r.get(10).unwrap(),
        scheduled_start_at: r.get(11).unwrap(),
        scheduled_end_at: r.get(12).unwrap(),
        started_at: r.get(13).unwrap(),
        completed_at: r.get(14).unwrap(),
        archived_at: r.get(15).unwrap(),
        updated_at: r.get(16).unwrap(),
    }
}
