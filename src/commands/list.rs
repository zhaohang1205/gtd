use rusqlite::Connection;

use crate::commands::effective_due;

use crate::repo::{tags, tasks};
use crate::time;
use anyhow::Result;

pub fn run(
    conn: &Connection,
    status: Option<&str>,
    tags_filter: &[String],
    project: Option<&str>,
    due_before: Option<&str>,
    json: bool,
) -> Result<()> {
    let f = tasks::ListFilter {
        status: status.map(|s| s.parse().unwrap()),
        project: project.map(|s| s.to_string()),
        tags: tags_filter.to_vec(),
    };
    let mut tasks_vec = tasks::list(conn, &f)?;

    if let Some(db) = due_before {
        let before = time::parse_time(db)?;
        tasks_vec.retain(|t| effective_due(t).map(|d| d <= before).unwrap_or(false));
        tasks_vec.sort_by_key(|t| effective_due(t).unwrap_or(i64::MAX));
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&tasks_vec)?);
        return Ok(());
    }

    if tasks_vec.is_empty() {
        println!("(no tasks)");
        return Ok(());
    }
    println!(
        "{:<8} {:<9} {:<17} {:<22} {}",
        "ID", "STATUS", "DUE", "TAGS", "TITLE"
    );
    for t in &tasks_vec {
        let tags_s = tags::get_task_tags(conn, &t.id)?
            .iter()
            .map(|x| x.name.as_str())
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{:<8} {:<9} {:<17} {:<22} {}",
            &t.id[..8],
            t.status,
            time::format_local(effective_due(t)),
            tags_s,
            t.title
        );
    }
    Ok(())
}
