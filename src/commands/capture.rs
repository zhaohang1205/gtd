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
    let mut tag_names: Vec<String> = tags.to_vec();
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
        None => None,
    };
    let status_str = status.unwrap_or("inbox");
    let parsed_status: task::Status = status_str.parse().map_err(|e| anyhow::anyhow!("{}", e))?;
    if !task::is_valid_status(&parsed_status) {
        anyhow::bail!("invalid status: {}", status_str);
    }

    let input = tasks::CaptureInput {
        title: title.to_string(),
        kind: task::TaskKind::Action,
        parent_id,
        status: parsed_status,
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
