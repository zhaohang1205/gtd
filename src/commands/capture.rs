use rusqlite::Connection;

use crate::model::task;
use crate::repo::tasks;
use crate::time;
use anyhow::Result;

/// CLI-derived arguments for `capture`. Keeps `run`'s signature small and
/// separates command parsing from the repo-layer `tasks::CaptureInput`.
pub struct CaptureArgs {
    pub title: String,
    pub project: Option<String>,
    pub tags: Vec<String>,
    pub p1: bool,
    pub p2: bool,
    pub p3: bool,
    pub due: Option<String>,
    pub status: Option<String>,
    pub json: bool,
}

pub fn run(conn: &Connection, args: CaptureArgs) -> Result<()> {
    let quick_add = crate::parser::parse_quick_add(&args.title);

    let mut tag_names: Vec<String> = args.tags.clone();
    tag_names.extend(quick_add.tags);
    if args.p1 {
        tag_names.push("p1".into());
    }
    if args.p2 {
        tag_names.push("p2".into());
    }
    if args.p3 {
        tag_names.push("p3".into());
    }

    let parent_id = match args.project {
        Some(p) => Some(tasks::resolve_project(conn, &p)?),
        None => None,
    };
    let due_at = match args.due {
        Some(d) => Some(time::parse_time(&d)?),
        None => match quick_add.time_str {
            Some(d) => Some(time::parse_time(&d)?),
            None => None,
        },
    };
    let status_str = args.status.as_deref().unwrap_or("inbox");
    let parsed_status: task::Status = status_str.parse().map_err(|e| anyhow::anyhow!("{}", e))?;

    let input = tasks::CaptureInput {
        title: quick_add.title,
        kind: task::TaskKind::Action,
        parent_id,
        status: if parsed_status == task::Status::Inbox && due_at.is_some() {
            task::Status::Scheduled
        } else {
            parsed_status
        },
        due_at,
        tag_names,
        ..Default::default()
    };
    let t = tasks::create_capture(conn, &input)?;

    if args.json {
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
