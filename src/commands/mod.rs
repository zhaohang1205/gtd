use rusqlite::Connection;

use crate::cli::Command;
use crate::model::task::Task;
use anyhow::Result;

mod alarm;
mod capture;
mod list;
pub mod pomo;
mod review;
mod show;
mod status;
mod tagging;

pub fn run(cmd: Command, conn: &Connection) -> Result<()> {
    match cmd {
        Command::Capture {
            title,
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
            due_before,
            json,
        } => list::run(conn, status.as_deref(), &tag, due_before.as_deref(), json),
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
        Command::Purge { id } => status::purge(conn, &id),
        Command::Tag { id, name } => tagging::add(conn, &id, &name),
        Command::Untag { id, name } => tagging::remove(conn, &id, &name),
        Command::Review => review::run(conn),
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
        Command::Alarm { action, slot } => match action.as_str() {
            "waybar" => alarm::waybar(slot),
            "next" => alarm::next(slot),
            _ => anyhow::bail!("unknown alarm action"),
        },
        Command::Tui => crate::tui::run(conn),
        Command::Completions { .. } => {
            anyhow::bail!("`gtp completions` is handled before the database is opened")
        }
    }
}

/// The "effective due" of a task: for recurring tasks the next occurrence on or
/// after now; otherwise due_at or scheduled_start_at. Used for sorting/filtering.
pub(crate) fn effective_due(task: &Task) -> Option<i64> {
    if let Some(rr) = &task.rrule {
        let anchor = task.scheduled_start_at.or(task.due_at);
        if let Some(start) = anchor {
            let now = crate::time::now_ms();
            if let Ok(occ) = crate::time::rrule_occurrences(rr, start, 366) {
                if let Some(next) = occ.into_iter().find(|m| *m >= now) {
                    return Some(next);
                }
            }
            return Some(start);
        }
    }
    task.due_at.or(task.scheduled_start_at)
}
