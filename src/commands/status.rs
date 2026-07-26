use rusqlite::Connection;

use crate::repo::tasks;
use crate::time;
use anyhow::Result;

pub fn to_status(conn: &Connection, id: &str, to: &str) -> Result<()> {
    let id = tasks::resolve_id(conn, id)?;
    let parsed_status: crate::model::task::Status = to.parse().map_err(|e| anyhow::anyhow!("{}", e))?;
    let t = tasks::transition(conn, &id, parsed_status)?;
    println!("{} -> {}", &t.id[..8], t.status);
    if to == "next" {
        let missing_project = t.parent_id.is_none();
        let missing_time = t.due_at.is_none() && t.scheduled_start_at.is_none();
        if missing_project || missing_time {
            let mut parts = Vec::new();
            if missing_project {
                parts.push("项目");
            }
            if missing_time {
                parts.push("时间");
            }
            println!("  tip: 建议补充{} — `gtp tag <id> <项目>` / `gtp schedule <id> --start <时间>`", parts.join("/"));
        }
    }
    Ok(())
}

pub fn schedule(
    conn: &Connection,
    id: &str,
    start: Option<&str>,
    end: Option<&str>,
    rrule: Option<&str>,
) -> Result<()> {
    let id = tasks::resolve_id(conn, id)?;
    let start_ms = match start {
        Some(s) => time::parse_time(s)?,
        None => anyhow::bail!("schedule requires --start <when>"),
    };
    let end_ms = match end {
        Some(e) => Some(time::parse_time(e)?),
        None => None,
    };
    let t = tasks::schedule(conn, &id, start_ms, end_ms, rrule.map(|s| s.to_string()))?;
    println!("scheduled {} at {}", &t.id[..8], time::format_local(Some(start_ms)));
    if let Some(rr) = &t.rrule {
        println!("  rrule: {}", rr);
    }
    Ok(())
}

pub fn archive(conn: &Connection, id: &str) -> Result<()> {
    let id = tasks::resolve_id(conn, id)?;
    tasks::archive(conn, &id)?;
    println!("archived {}", &id[..8]);
    Ok(())
}

pub fn restore(conn: &Connection, id: &str) -> Result<()> {
    let id = tasks::resolve_id(conn, id)?;
    tasks::unarchive(conn, &id)?;
    println!("restored {}", &id[..8]);
    Ok(())
}
