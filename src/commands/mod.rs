use rusqlite::Connection;

use crate::cli::Command;
use crate::model::task::Task;
use anyhow::Result;

mod capture;
mod list;
pub mod pomo;
mod project;
mod show;
mod status;
mod tagging;

pub fn run(cmd: Command, conn: &Connection) -> Result<()> {
    match cmd {
        Command::Capture {
            title,
            project,
            tag,
            p1,
            p2,
            p3,
            due,
            status,
            json,
        } => capture::run(
            conn,
            capture::CaptureArgs {
                title,
                project,
                tags: tag,
                p1,
                p2,
                p3,
                due,
                status,
                json,
            },
        ),
        Command::List {
            status,
            tag,
            project,
            due_before,
            json,
        } => list::run(
            conn,
            status.as_deref(),
            &tag,
            project.as_deref(),
            due_before.as_deref(),
            json,
        ),
        Command::Show { id, json } => show::run(conn, &id, json),
        Command::Next { id } => status::to_status(conn, &id, "next"),
        Command::Wait { id } => status::to_status(conn, &id, "waiting"),
        Command::Someday { id } => status::to_status(conn, &id, "someday"),
        Command::Done { id } => status::to_status(conn, &id, "done"),
        Command::Schedule {
            id,
            start,
            end,
            rrule,
        } => status::schedule(
            conn,
            &id,
            start.as_deref(),
            end.as_deref(),
            rrule.as_deref(),
        ),
        Command::Archive { id } => status::archive(conn, &id),
        Command::Restore { id } => status::restore(conn, &id),
        Command::Tag { id, name } => tagging::add(conn, &id, &name),
        Command::Untag { id, name } => tagging::remove(conn, &id, &name),
        Command::Project { name, tag } => project::create(conn, &name, &tag),
        Command::Tree => project::tree(conn),
        Command::Review => project::review(conn),
        Command::Tags => tagging::list(conn),
        Command::Pomo { action, task_id } => match action.as_str() {
            "start" => {
                if let Some(id) = task_id {
                    pomo::start(conn, &id)
                } else {
                    anyhow::bail!("task_id required for start")
                }
            }
            "stop" => pomo::stop(),
            "daemon" => pomo::daemon(),
            "waybar" => pomo::waybar(),
            _ => anyhow::bail!("unknown pomo action"),
        },
        Command::Tui => crate::tui::run(conn),
    }
}

/// The "effective due" of a task: for recurring tasks the next occurrence on or
/// after now; otherwise due_at or scheduled_start_at. Used for sorting/filtering.
pub(crate) fn effective_due(task: &Task) -> Option<i64> {
    if let (Some(rr), Some(start)) = (&task.rrule, task.scheduled_start_at) {
        let now = crate::time::now_ms();
        if let Ok(occ) = crate::time::rrule_occurrences(rr, start, 366) {
            if let Some(next) = occ.into_iter().find(|m| *m >= now) {
                return Some(next);
            }
        }
        return Some(start);
    }
    task.due_at.or(task.scheduled_start_at)
}
