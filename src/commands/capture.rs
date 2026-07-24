use rusqlite::Connection;

use crate::model::task;
use crate::repo::tasks;
use crate::time;
use anyhow::Result;

pub fn run(
    conn: &Connection,
    title: &str,
    project: Option<&str>,
    tags: &[String],
    p1: bool,
    p2: bool,
    p3: bool,
    due: Option<&str>,
    status: Option<&str>,
    json: bool,
) -> Result<()> {
    let quick_add = crate::parser::parse_quick_add(title);
    
    let mut tag_names: Vec<String> = tags.to_vec();
    tag_names.extend(quick_add.tags);
    if p1 {
        tag_names.push("p1".into());
    }
    if p2 {
        tag_names.push("p2".into());
    }
    if p3 {
        tag_names.push("p3".into());
    }

    let parent_id = match project {
        Some(p) => Some(tasks::resolve_project(conn, p)?),
        None => None,
    };
    let due_at = match due {
        Some(d) => Some(time::parse_time(d)?),
        None => {
            if let Some(ref d) = quick_add.time_str {
                Some(time::parse_time(d)?)
            } else {
                None
            }
        }
    };
    let status_str = status.unwrap_or("inbox");
    let parsed_status: task::Status = status_str.parse().map_err(|e| anyhow::anyhow!("{}", e))?;

    let input = tasks::CaptureInput {
        title: quick_add.title,
        kind: task::TaskKind::Action,
        parent_id,
        status: if parsed_status == task::Status::Inbox && due_at.is_some() { task::Status::Scheduled } else { parsed_status },
        due_at,
        tag_names,
        ..Default::default()
    };
    let t = tasks::create_capture(conn, &input)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&t)?);
    } else {
        println!(
            "captured [{}] {}  (status: {})",
            &t.id[..8],
            t.title,
            t.status
        );
    }
    Ok(())
}
