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
    let duration_ms = (state.config.work_mins as i64) * 60 * 1000;
    
    state.phase = Phase::Work;
    state.task_id = Some(task.id.clone());
    state.task_title = Some(task.title.clone());
    state.start_ts = Some(now);
    state.end_ts = Some(now + duration_ms);
    pomodoro::save_state(&state)?;

    let exe_path = std::env::current_exe().unwrap_or_else(|_| "gtp".into());
    StdCommand::new(exe_path)
        .args(["pomo", "daemon"])
        .spawn()?;
        
    notify("🎯 专注模式开启", &format!("开始专注任务: {}\n预计时长: {} 分钟", task.title, state.config.work_mins));
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
    notify("⏹️ 专注已终止", "番茄钟与专注模式已停止");
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
                        let duration = state.config.work_mins * 60;
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
                    state.today_count += 1;
                    state.streak += 1;
                    state.last_completed_task_title = state.task_title.clone();

                    let current_title = state.task_title.as_deref().unwrap_or("无标题");
                    let total_mins = state.today_count * state.config.work_mins;

                    if state.cycle.is_multiple_of(4) {
                        state.phase = Phase::LongBreak;
                        let long_break_ms = (state.config.long_break_mins as i64) * 60 * 1000;
                        state.end_ts = Some(now + long_break_ms);
                        notify(
                            "🏆 专注成就战报达成！",
                            &format!(
                                "🎯 任务: {}\n⏱️ 专注: {}m | 🔥 今日第 {} 个番茄 ({:.1}h)\n💪 连击 Streak: {} 连击！建议长休 {} 分钟 ☕",
                                current_title,
                                state.config.work_mins,
                                state.today_count,
                                (total_mins as f64) / 60.0,
                                state.streak,
                                state.config.long_break_mins
                            ),
                        );
                    } else {
                        state.phase = Phase::ShortBreak;
                        let short_break_ms = (state.config.short_break_mins as i64) * 60 * 1000;
                        state.end_ts = Some(now + short_break_ms);
                        notify(
                            "🎉 专注成就战报达成！",
                            &format!(
                                "🎯 任务: {}\n⏱️ 专注: {}m | 🔥 今日第 {} 个番茄 ({:.1}h)\n💪 连击 Streak: {} 连击！开启小休 {} 分钟 ☕",
                                current_title,
                                state.config.work_mins,
                                state.today_count,
                                (total_mins as f64) / 60.0,
                                state.streak,
                                state.config.short_break_mins
                            ),
                        );
                    }
                }
                Phase::ShortBreak | Phase::LongBreak => {
                    state.phase = Phase::Idle;
                    notify("⏰ 休息结束！战报结清", "休息已完成！按 [Space / P] 再接再厉开启新一轮，或按 [S] 结束专注。💪");
                }
                Phase::Idle => {}
            }
            let _ = pomodoro::save_state(&state);
        }
        
        thread::sleep(Duration::from_secs(1));
    }
}

fn notify(summary: &str, body: &str) {
    // 桌面图形通知
    let _ = StdCommand::new("notify-send")
        .args(["-u", "normal", "-i", "appointment-soon", summary, body])
        .status();

    // 声音提醒多层降级策略：paplay -> pw-play -> aplay -> mpv -> 终端铃声 \x07
    let sound_paths = [
        "/usr/share/sounds/freedesktop/stereo/complete.oga",
        "/usr/share/sounds/freedesktop/stereo/alarm-clock-elapsed.oga",
        "/usr/share/sounds/freedesktop/stereo/service-login.oga",
        "/usr/share/sounds/alsa/Front_Center.wav",
    ];

    let mut played = false;
    for path in sound_paths.iter() {
        if std::path::Path::new(path).exists() {
            if StdCommand::new("paplay").arg(path).status().map(|s| s.success()).unwrap_or(false)
                || StdCommand::new("pw-play").arg(path).status().map(|s| s.success()).unwrap_or(false)
                || StdCommand::new("aplay").arg(path).status().map(|s| s.success()).unwrap_or(false)
                || StdCommand::new("mpv").args(["--no-terminal", path]).status().map(|s| s.success()).unwrap_or(false)
            {
                played = true;
                break;
            }
        }
    }

    if !played {
        // 音频播放失败时发出终端 Bell 响铃
        print!("\x07");
        let _ = std::io::Write::flush(&mut std::io::stdout());
    }
}
