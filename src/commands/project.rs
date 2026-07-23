use rusqlite::Connection;

use crate::commands::effective_due;
use crate::repo::tasks;
use crate::time;
use anyhow::Result;

pub fn create(conn: &Connection, name: &str, tags: &[String]) -> Result<()> {
    let input = tasks::CaptureInput {
        title: name.to_string(),
        kind: "project".to_string(),
        parent_id: None,
        status: "next".to_string(),
        due_at: None,
        tag_names: tags.to_vec(),
    };
    let p = tasks::create_capture(conn, &input)?;
    println!("project created [{}] {}", &p.id[..8], p.title);
    Ok(())
}

pub fn tree(conn: &Connection) -> Result<()> {
    let projects = tasks::list(
        conn,
        &tasks::ListFilter {
            status: None,
            project: None,
            tags: vec![],
        },
    )?
    .into_iter()
    .filter(|t| t.kind == "project")
    .collect::<Vec<_>>();

    for p in &projects {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM tasks WHERE parent_id = ?1 AND archived_at IS NULL",
            rusqlite::params![p.id],
            |r| r.get(0),
        )?;
        println!("● {}  [{}]  ({} actions)", p.title, &p.id[..8], count);
        let actions = tasks::list(
            conn,
            &tasks::ListFilter {
                status: None,
                project: Some(p.id.clone()),
                tags: vec![],
            },
        )?;
        for a in actions {
            println!("    - [{:<8}] {:<9} {}", &a.id[..8], a.status, a.title);
        }
    }
    if projects.is_empty() {
        println!("(no projects)");
    }
    Ok(())
}

pub fn review(conn: &Connection) -> Result<()> {
    let all = tasks::list(
        conn,
        &tasks::ListFilter {
            status: None,
            project: None,
            tags: vec![],
        },
    )?;
    let inbox = all.iter().filter(|t| t.status == "inbox").count();
    let next = all.iter().filter(|t| t.status == "next").count();
    let waiting = all.iter().filter(|t| t.status == "waiting").count();
    let someday = all.iter().filter(|t| t.status == "someday").count();
    let scheduled = all.iter().filter(|t| t.status == "scheduled").count();

    let now = time::now_ms();
    let horizon = 3 * 24 * 3600 * 1000i64;
    let due_soon = all
        .iter()
        .filter(|t| {
            effective_due(t)
                .map(|x| x <= now + horizon)
                .unwrap_or(false)
        })
        .count();

    println!("Weekly Review");
    println!("  inbox     : {}", inbox);
    println!("  next      : {}", next);
    println!("  waiting   : {}", waiting);
    println!("  scheduled : {} ({} due within 3d)", scheduled, due_soon);
    println!("  someday   : {}", someday);
    Ok(())
}
