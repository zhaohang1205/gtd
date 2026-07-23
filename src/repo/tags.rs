use rusqlite::Connection;

use anyhow::Result;
use crate::error::Error;
use crate::model::tag::Tag;
use crate::repo::log_event;
use crate::time;

pub fn list_tags(conn: &Connection) -> Result<Vec<Tag>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, category, is_system, color, icon, description, created_at \
         FROM tags ORDER BY category, name",
    )?;
    let rows = stmt.query_map([], |r| Ok(row_to_tag(r)))?;
    let mut out = Vec::new();
    for t in rows {
        out.push(t?);
    }
    Ok(out)
}

pub fn get_tag_by_name(conn: &Connection, name: &str) -> Result<Option<Tag>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, category, is_system, color, icon, description, created_at \
         FROM tags WHERE name = ?1",
    )?;
    let mut rows = stmt.query_map([name], |r| Ok(row_to_tag(r)))?;
    Ok(rows.next().transpose()?.map(|t| t))
}

/// Return the tag id for `name`, creating a custom tag if it doesn't exist.
pub fn find_or_create_tag(conn: &Connection, name: &str) -> Result<i64> {
    if let Some(t) = get_tag_by_name(conn, name)? {
        return Ok(t.id);
    }
    let category = if name == "p1" || name == "p2" || name == "p3" {
        "priority"
    } else {
        "custom"
    };
    let now = time::now_ms();
    conn.execute(
        "INSERT INTO tags (name, category, is_system, created_at) VALUES (?1, ?2, 0, ?3)",
        rusqlite::params![name, category, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn add_tag_to_task(conn: &Connection, task_id: &str, tag_name: &str) -> Result<()> {
    let tag_id = find_or_create_tag(conn, tag_name)?;
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM task_tags WHERE task_id = ?1 AND tag_id = ?2",
        rusqlite::params![task_id, tag_id],
        |r| r.get(0),
    )?;
    if exists > 0 {
        return Ok(());
    }
    let now = time::now_ms();
    conn.execute(
        "INSERT INTO task_tags (task_id, tag_id, added_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![task_id, tag_id, now],
    )?;
    let meta = format!("{{\"name\":\"{}\"}}", tag_name);
    log_event(
        conn,
        task_id,
        crate::model::event::EV_TAG_ADDED,
        None,
        None,
        Some(&meta),
    )?;
    Ok(())
}

pub fn remove_tag_from_task(conn: &Connection, task_id: &str, tag_name: &str) -> Result<()> {
    let tag = get_tag_by_name(conn, tag_name)?
        .ok_or_else(|| Error::TagNotFound(tag_name.to_string()))?;
    let deleted = conn.execute(
        "DELETE FROM task_tags WHERE task_id = ?1 AND tag_id = ?2",
        rusqlite::params![task_id, tag.id],
    )?;
    if deleted == 0 {
        return Err(Error::TagNotFound(tag_name.to_string()).into());
    }
    let meta = format!("{{\"name\":\"{}\"}}", tag_name);
    log_event(
        conn,
        task_id,
        crate::model::event::EV_TAG_REMOVED,
        None,
        None,
        Some(&meta),
    )?;
    Ok(())
}

pub fn get_task_tags(conn: &Connection, task_id: &str) -> Result<Vec<Tag>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.name, t.category, t.is_system, t.color, t.icon, t.description, t.created_at \
         FROM tags t JOIN task_tags tt ON tt.tag_id = t.id WHERE tt.task_id = ?1 \
         ORDER BY t.category, t.name",
    )?;
    let rows = stmt.query_map([task_id], |r| Ok(row_to_tag(r)))?;
    let mut out = Vec::new();
    for t in rows {
        out.push(t?);
    }
    Ok(out)
}

fn row_to_tag(r: &rusqlite::Row) -> Tag {
    Tag {
        id: r.get(0).unwrap(),
        name: r.get(1).unwrap(),
        category: r.get(2).unwrap(),
        is_system: r.get::<usize, i64>(3).unwrap() != 0,
        color: r.get(4).unwrap(),
        icon: r.get(5).unwrap(),
        description: r.get(6).unwrap(),
        created_at: r.get(7).unwrap(),
    }
}
