use anyhow::Result;
use rusqlite::Connection;
use std::process::Command as StdCommand;
use std::thread;
use std::time::Duration;
use crate::model::{event, pomodoro::Phase};
use crate::repo::{pomodoro, tasks};
use crate::time;

fn kill_daemon() {
    let _ = StdCommand::new("pkill").args(["-f", "gtp pomo daemon"]).status();
}

pub fn start(conn: &Connection, task_id: &str) -> Result<()> {
    let task = tasks::get(conn, task_id)?;
    kill_daemon();
    let mut state = pomodoro::get_state()?;
    let now = time::now_ms();
    let duration_ms = 25 * 60 * 1000;
    
    state.phase = Phase::Work;
    state.task_id = Some(task.id.clone());
    state.task_title = Some(task.title.clone());
    state.start_ts = Some(now);
    state.end_ts = Some(now + duration_ms);
    pomodoro::save_state(&state)?;

    StdCommand::new("gtp")
        .args(["pomo", "daemon"])
        .spawn()?;
        
    println!("Started pomodoro for: {}", task.title);
    Ok(())
}

pub fn stop() -> Result<()> {
    kill_daemon();
    let mut state = pomodoro::get_state()?;
    state.phase = Phase::Idle;
    state.task_id = None;
    state.task_title = None;
    state.start_ts = None;
    state.end_ts = None;
    pomodoro::save_state(&state)?;
    println!("Pomodoro stopped.");
    Ok(())
}

pub fn waybar() -> Result<()> {
    let state = pomodoro::get_state()?;
    if state.phase == Phase::Idle {
        println!("{}", serde_json::json!({
            "text": "",
            "class": "idle"
        }));
        return Ok(());
    }
    
    let now = time::now_ms();
    let end_ts = state.end_ts.unwrap_or(now);
    let mut diff = (end_ts - now) / 1000;
    if diff < 0 {
        diff = 0;
    }
    let m = diff / 60;
    let s = diff % 60;
    let text = format!("🍅 {:02}:{:02}", m, s);
    let class = match state.phase {
        Phase::Work => "work",
        Phase::ShortBreak => "short_break",
        Phase::LongBreak => "long_break",
        Phase::Idle => "idle",
    };
    let tooltip = format!("{} - {:?}", state.task_title.as_deref().unwrap_or(""), state.phase);
    println!("{}", serde_json::json!({
        "text": text,
        "class": class,
        "tooltip": tooltip
    }));
    Ok(())
}

pub fn daemon() -> Result<()> {
    let conn = crate::db::conn::open()?;
    loop {
        let mut state = pomodoro::get_state().unwrap_or_default();
        if state.phase == Phase::Idle {
            thread::sleep(Duration::from_secs(1));
            continue;
        }
        
        let now = time::now_ms();
        let end_ts = state.end_ts.unwrap_or(now);
        
        if now >= end_ts {
            match state.phase {
                Phase::Work => {
                    if let Some(ref tid) = state.task_id {
                        let duration = 25 * 60;
                        let _ = crate::repo::log_event(
                            &conn,
                            tid,
                            event::EV_POMODORO,
                            None,
                            None,
                            Some(&duration.to_string()),
                        );
                    }
                    state.cycle += 1;
                    state.total_count += 1;
                    if state.cycle.is_multiple_of(4) {
                        state.phase = Phase::LongBreak;
                        state.end_ts = Some(now + 15 * 60 * 1000);
                        notify("Pomodoro Work Complete", "Time for a long break! (15m)");
                    } else {
                        state.phase = Phase::ShortBreak;
                        state.end_ts = Some(now + 5 * 60 * 1000);
                        notify("Pomodoro Work Complete", "Time for a short break! (5m)");
                    }
                }
                Phase::ShortBreak | Phase::LongBreak => {
                    state.phase = Phase::Idle;
                    state.task_id = None;
                    state.task_title = None;
                    state.start_ts = None;
                    state.end_ts = None;
                    notify("Break Finished", "Ready to start again!");
                }
                Phase::Idle => {}
            }
            let _ = pomodoro::save_state(&state);
        }
        
        thread::sleep(Duration::from_secs(1));
    }
}

fn notify(summary: &str, body: &str) {
    let _ = StdCommand::new("notify-send")
        .args([summary, body])
        .status();
    let _ = StdCommand::new("paplay")
        .arg("/usr/share/sounds/freedesktop/stereo/complete.oga")
        .status();
}
