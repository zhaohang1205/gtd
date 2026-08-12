use rusqlite::Connection;
use uuid::Uuid;

use crate::error::Error;
use crate::model::event;
use crate::model::task::{self, Task};
use crate::repo::log_event;
use crate::time;
use anyhow::Result;

/// Input for creating a task (capture).
pub struct CaptureInput {
    pub title: String,
    pub status: task::Status,
    pub due_at: Option<i64>,
    pub tag_names: Vec<String>,
    pub rrule: Option<String>,
    pub delegated_to: Option<String>,
    pub checklist: Vec<task::ChecklistItem>,
}

impl Default for CaptureInput {
    fn default() -> Self {
        Self {
            title: String::new(),
            status: task::Status::Inbox,
            due_at: None,
            tag_names: Vec::new(),
            rrule: None,
            delegated_to: None,
            checklist: Vec::new(),
        }
    }
}

pub fn get(conn: &Connection, id: &str) -> Result<Task> {
    let mut stmt = conn.prepare(
        "SELECT id,title,notes,status,rrule,created_at,clarified_at,\
                due_at,scheduled_start_at,scheduled_end_at,started_at,completed_at,archived_at,updated_at,\
                delegated_to,checklist,archive_reason \
         FROM tasks WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map([id], row_to_task)?;
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

    let cl_str = serde_json::to_string(&input.checklist).unwrap_or_else(|_| "[]".to_string());

    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO tasks \
         (id,title,notes,status,rrule,created_at,clarified_at,due_at,updated_at,delegated_to,checklist) \
         VALUES (?1,?2,'',?3,?4,?5,?6,?7,?8,?9,?10)",
        rusqlite::params![
            id,
            input.title,
            status.to_string(),
            input.rrule,
            now,
            clarified,
            input.due_at,
            now,
            input.delegated_to,
            cl_str
        ],
    )?;
    let status_str = status.to_string();
    log_event(&tx, &id, event::EV_CAPTURED, None, Some(&status_str), None)?;
    if clarified.is_some() {
        log_event(&tx, &id, event::EV_CLARIFIED, None, Some(&status_str), None)?;
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

pub fn update_notes(conn: &Connection, id: &str, new_notes: &str) -> Result<Task> {
    let now = time::now_ms();
    conn.execute(
        "UPDATE tasks SET notes=?1, updated_at=?2 WHERE id=?3",
        rusqlite::params![new_notes, now, id],
    )?;
    get(conn, id)
}

pub fn update_checklist(
    conn: &Connection,
    id: &str,
    checklist: &Vec<task::ChecklistItem>,
) -> Result<Task> {
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

    let tx = conn.unchecked_transaction()?;

    if to_status == task::Status::Done {
        // 循环任务：把起点（排程开始时间或截止时间）推进到下一次发生，继续排程。
        // 兼容仅有 due_at + rrule（如快速录入 `~time rrule=...`）的任务。
        if let Some(rrule) = &t.rrule {
            let anchor = t.scheduled_start_at.or(t.due_at);
            if let Some(start) = anchor {
                if let Ok(occ) = time::rrule_occurrences(rrule, start, 366) {
                    if let Some(next) = occ.into_iter().find(|m| *m > start) {
                        let duration = t.scheduled_end_at.unwrap_or(start) - start;

                        log_event(
                            &tx,
                            id,
                            event::EV_HABIT_COMPLETED,
                            Some(&from.to_string()),
                            Some(&task::Status::Done.to_string()),
                            None,
                        )?;

                        if t.scheduled_start_at.is_some() {
                            t.scheduled_start_at = Some(next);
                            t.scheduled_end_at = Some(next + duration);
                        }
                        if t.scheduled_start_at.is_none() && t.due_at.is_some() {
                            t.due_at = Some(next);
                        }
                        t.status = task::Status::Scheduled;
                        t.updated_at = now;

                        tx.execute(
                            "UPDATE tasks SET status=?1, clarified_at=?2, completed_at=?3, updated_at=?4, started_at=?5, scheduled_start_at=?6, scheduled_end_at=?7, due_at=?8 WHERE id=?9",
                            rusqlite::params![t.status.to_string(), t.clarified_at, t.completed_at, t.updated_at, t.started_at, t.scheduled_start_at, t.scheduled_end_at, t.due_at, id],
                        )?;
                        tx.commit()?;
                        return Ok(t);
                    }
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

/// Set a soft deadline (`due_at`) without changing the task status. Used by the
/// inbox→next planning hook so a next action keeps its status while gaining a due.
pub fn set_due(conn: &Connection, id: &str, due_ms: Option<i64>) -> Result<Task> {
    let mut t = get(conn, id)?;
    t.due_at = due_ms;
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

/// Replace a task's recurrence rule, keeping its scheduled window. Used by the
/// TUI "edit rrule" action. Logs a `scheduled` event (rule change).
pub fn set_rrule(conn: &Connection, id: &str, rrule: Option<String>) -> Result<Task> {
    let mut t = get(conn, id)?;
    let from = t.status;
    t.rrule = rrule.clone();
    t.updated_at = time::now_ms();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE tasks SET rrule=?1, updated_at=?2 WHERE id=?3",
        rusqlite::params![t.rrule, t.updated_at, id],
    )?;
    let meta = t
        .rrule
        .as_deref()
        .map(|r| format!("{{\"rrule\":\"{}\"}}", r));
    let to_s = t.status.to_string();
    log_event(
        &tx,
        id,
        event::EV_SCHEDULED,
        Some(&from.to_string()),
        Some(&to_s),
        meta.as_deref(),
    )?;
    tx.commit()?;
    Ok(t)
}

/// Set/clear the delegated-to field (the "waiting for" person/thing).
pub fn set_delegated(conn: &Connection, id: &str, who: Option<String>) -> Result<Task> {
    let mut t = get(conn, id)?;
    t.delegated_to = who;
    t.updated_at = time::now_ms();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE tasks SET delegated_to=?1, updated_at=?2 WHERE id=?3",
        rusqlite::params![t.delegated_to, t.updated_at, id],
    )?;
    tx.commit()?;
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
    let t = get(conn, id)?;
    let now = time::now_ms();
    let reason = if t.status == task::Status::Done {
        "completed"
    } else {
        "deleted"
    };
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE tasks SET archived_at=?1, archive_reason=?2, updated_at=?3 WHERE id=?4",
        rusqlite::params![now, reason, now, id],
    )?;
    log_event(&tx, id, event::EV_ARCHIVED, None, None, Some(reason))?;
    tx.commit()?;
    get(conn, id)
}

/// Undo a soft-delete: clear `archived_at` and record a `restored` event.
pub fn unarchive(conn: &Connection, id: &str) -> Result<Task> {
    let _ = get(conn, id)?;
    let now = time::now_ms();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE tasks SET archived_at=NULL, archive_reason=NULL, updated_at=?1 WHERE id=?2",
        rusqlite::params![now, id],
    )?;
    log_event(&tx, id, event::EV_RESTORED, None, None, None)?;
    tx.commit()?;
    get(conn, id)
}

/// Count of archived (soft-deleted) tasks, for the guide sidebar badge.
pub fn count_archived(conn: &Connection) -> Result<usize> {
    let c: usize = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE archived_at IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    Ok(c)
}

/// Tasks whose `due_at` falls in the inclusive `[start_ms, end_ms]` window.
/// Lightweight query returning only the columns the due-notification check needs,
/// instead of scanning every task row on each tick.
pub fn due_in_range(
    conn: &Connection,
    start_ms: i64,
    end_ms: i64,
) -> Result<Vec<(String, String, Option<i64>)>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, due_at FROM tasks \
         WHERE archived_at IS NULL AND due_at BETWEEN ?1 AND ?2",
    )?;
    let rows = stmt.query_map(rusqlite::params![start_ms, end_ms], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// List only archived (soft-deleted) tasks, for the restore UI.
pub fn list_archived(conn: &Connection) -> Result<Vec<Task>> {
    let mut stmt = conn.prepare(
        "SELECT id,title,notes,status,rrule,created_at,clarified_at,\
                due_at,scheduled_start_at,scheduled_end_at,started_at,completed_at,archived_at,updated_at,\
                delegated_to,checklist,archive_reason \
         FROM tasks WHERE archived_at IS NOT NULL \
         ORDER BY archived_at DESC",
    )?;
    let rows = stmt.query_map([], row_to_task)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub struct ListFilter {
    pub status: Option<task::Status>,
    pub tags: Vec<String>,
    pub query: Option<String>,
    pub review_stale: bool,
}

pub fn list(conn: &Connection, f: &ListFilter) -> Result<Vec<Task>> {
    let mut sql = String::from(
        "SELECT id,title,notes,status,rrule,created_at,clarified_at,\
                due_at,scheduled_start_at,scheduled_end_at,started_at,completed_at,archived_at,updated_at,\
                delegated_to,checklist,archive_reason \
         FROM tasks WHERE archived_at IS NULL",
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(s) = &f.status {
        sql.push_str(" AND status = ?");
        params.push(Box::new(s.to_string()));
    }
    if f.review_stale {
        let seven_days_ago = crate::time::now_ms() - 7 * 24 * 3600 * 1000;
        sql.push_str(" AND (updated_at < ? OR updated_at IS NULL)");
        params.push(Box::new(seven_days_ago));
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
    let rows = stmt.query_map(param_refs.as_slice(), row_to_task)?;
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
    let status_str: String = r.get(3)?;
    let delegated_to: Option<String> = r.get(14)?;
    let cl_str: String = r.get(15)?;

    Ok(Task {
        id: r.get(0)?,
        title: r.get(1)?,
        notes: r.get(2)?,
        status: status_str
            .parse()
            .unwrap_or(crate::model::task::Status::Inbox),
        rrule: r.get(4)?,
        created_at: r.get(5)?,
        clarified_at: r.get(6)?,
        due_at: r.get(7)?,
        scheduled_start_at: r.get(8)?,
        scheduled_end_at: r.get(9)?,
        started_at: r.get(10)?,
        completed_at: r.get(11)?,
        archived_at: r.get(12)?,
        updated_at: r.get(13)?,
        delegated_to,
        checklist: serde_json::from_str(&cl_str).unwrap_or_default(),
        archive_reason: r.get(16)?,
    })
}
