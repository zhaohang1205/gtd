use super::app::{App, Mode, Pane, View};
use super::calendar;
use crate::model::task::{self};
use crate::repo::tasks::CaptureInput;
use crate::repo::{tags, tasks};
use crate::time;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}
pub(crate) trait AppHandlers {
    fn handle_key(&mut self, key: KeyEvent) -> Result<()>;
    fn handle_normal(&mut self, key: KeyEvent) -> Result<()>;
    fn handle_input(&mut self, key: KeyEvent) -> Result<()>;
    fn confirm_input(&mut self, mode: Mode, input: &str) -> Result<()>;
    fn restore_selected(&mut self) -> Result<()>;
}

impl<'a> AppHandlers for App<'a> {
    fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return Ok(());
        }

        if let Some(popup) = self.popup.take() {
            match key.code {
                KeyCode::Enter => {
                    if let crate::tui::app::Popup::TaskDueNow(id, _) = popup {
                        let _ = crate::commands::pomo::start(self.conn, &id);
                        self.needs_clear = true;
                    }
                }
                _ => {
                    // Default to close on any key
                    if let crate::tui::app::Popup::TaskDueNow(_, _) = popup {
                        if key.code != KeyCode::Esc {
                            // If they typed something else, maybe restore the popup? Let's just close on anything for now.
                        }
                    }
                }
            }
            return Ok(());
        }

        match self.mode {
            Mode::Normal | Mode::Visual => self.handle_normal(key),
            _ => self.handle_input(key),
        }
    }

    fn handle_normal(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                if self.is_reviewing {
                    self.is_reviewing = false;
                    self.review_step = 0;
                    self.status_message = "Weekly Review cancelled".into();
                    let _ = self.refresh();
                    self.load_detail();
                } else if self.mode == Mode::Visual {
                    self.set_mode(Mode::Normal);
                    self.visual_start_idx = None;
                    self.selected_ids.clear();
                    self.status_message = "Exited visual mode".into();
                    let _ = self.refresh();
                    self.load_detail();
                } else if self.tag_filter.is_some() || !self.search_query.is_empty() {
                    self.tag_filter = None;
                    self.search_query.clear();
                    self.status_message = "Cleared filters".into();
                    let _ = self.refresh();
                    self.load_detail();
                } else {
                    self.hide_pomo_banner = true;
                    self.status_message.clear();
                }
            }
            KeyCode::Char('v') | KeyCode::Char('V') => {
                if self.mode == Mode::Visual {
                    self.set_mode(Mode::Normal);
                    self.visual_start_idx = None;
                    self.selected_ids.clear();
                    self.status_message = "Exited visual mode".into();
                } else {
                    self.set_mode(Mode::Visual);
                    self.visual_start_idx = Some(self.selected);
                    self.update_visual_selection();
                    self.status_message = "-- VISUAL --".into();
                }
                let _ = self.refresh();
            }
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('?') | KeyCode::F(1) => self.show_help = !self.show_help,
            KeyCode::F(5) => {
                self.theme = self.theme.toggle();
                self.status_message = if self.theme.is_dark {
                    "Theme set to Catppuccin Mocha (Dark)".to_string()
                } else {
                    "Theme set to Catppuccin Latte (Light)".to_string()
                };
            },
            KeyCode::Char('h') => {
                self.pane = match self.pane {
                    Pane::Right => Pane::Center,
                    Pane::Center => Pane::Left,
                    Pane::Left => Pane::Left,
                };
            }
            KeyCode::Char('l') => {
                self.pane = match self.pane {
                    Pane::Left => Pane::Center,
                    Pane::Center => Pane::Right,
                    Pane::Right => Pane::Right,
                };
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.pane == Pane::Left {
                    self.next_view(1);
                } else {
                    self.move_sel(1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.pane == Pane::Left {
                    self.next_view(-1);
                } else {
                    self.move_sel(-1);
                }
            }
            KeyCode::Char(d) if d.is_ascii_digit() => {
                if let Some(v) = View::from_digit(d) {
                    self.set_view(v);
                }
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.show_syntax = !self.show_syntax;
            }
            KeyCode::Char('p') => self.set_view(View::Projects),
            KeyCode::Char('r') => {
                self.is_reviewing = true;
                self.review_step = 1;
                self.set_view(View::Inbox);
                self.status_message = "Weekly Review started".into();
            }
            KeyCode::Char('R') if self.is_reviewing => {
                self.review_step += 1;
                match self.review_step {
                    2 => self.set_view(View::Projects),
                    3 => self.set_view(View::Waiting),
                    4 => self.set_view(View::Someday),
                    _ => {
                        self.is_reviewing = false;
                        self.review_step = 0;
                        self.set_view(View::Next);
                        self.status_message = "Weekly Review Complete! 🎉".into();
                    }
                }
            }
            KeyCode::Char('a') => {
                if self.view == View::Tags {
                    self.set_mode(Mode::CreatingTag);
                    self.input.clear();
                } else {
                    self.set_mode(Mode::Capturing);
                    self.input.clear();
                }
            }
            KeyCode::Char('e') => {
                if let Some(row) = self.items.get(self.selected).cloned() {
                    self.set_mode(Mode::EditingTitle);
                    self.input = row.title.clone();
                }
            }
            KeyCode::Char('n') => {
                if let Some(row) = self.items.get(self.selected).cloned() {
                    if let Ok(task) = tasks::get(self.conn, &row.id) {
                        crossterm::terminal::disable_raw_mode()?;
                        crossterm::execute!(
                            std::io::stdout(),
                            crossterm::terminal::LeaveAlternateScreen
                        )?;

                        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
                        let mut temp_file = tempfile::NamedTempFile::new()?;
                        use std::io::Write;
                        temp_file.write_all(task.notes.as_bytes())?;

                        let _ = std::process::Command::new(editor)
                            .arg(temp_file.path())
                            .status();

                        if let Ok(new_notes) = std::fs::read_to_string(temp_file.path()) {
                            if new_notes != task.notes {
                                let _ = tasks::update_notes(self.conn, &task.id, &new_notes);
                            }
                        }

                        crossterm::terminal::enable_raw_mode()?;
                        crossterm::execute!(
                            std::io::stdout(),
                            crossterm::terminal::EnterAlternateScreen
                        )?;
                        self.needs_clear = true;
                        self.load_detail();
                    }
                }
            }
            KeyCode::Char('x') => {
                self.act_on_selected(task::Status::Done)?;
                self.move_sel(1);
            }
            KeyCode::Char('w') => {
                self.set_mode(Mode::WaitingWho);
                self.input.clear();
            }
            KeyCode::Char('s') => self.act_on_selected(task::Status::Someday)?,
            KeyCode::Char('c') => {
                self.set_mode(Mode::SchedulingCalendar);
                self.calendar = calendar::CalendarState::new();
                self.input.clear();
            }
            KeyCode::Char('t') => {
                self.set_mode(Mode::Tagging);
                self.input.clear();
            }
            KeyCode::Char('d') => {
                if let Some(row) = self.items.get(self.selected).cloned() {
                    if let Ok(t) = tasks::get(self.conn, &row.id) {
                        self.set_mode(Mode::EditingDue);
                        self.input = time::format_local(t.due_at);
                        if self.input == "-" {
                            self.input.clear();
                        }
                    }
                }
            }
            KeyCode::Char('L') => {
                if let Some(row) = self.items.get(self.selected).cloned() {
                    if let Ok(t) = tasks::get(self.conn, &row.id) {
                        self.set_mode(Mode::EditingRrule);
                        self.input = t.rrule.clone().unwrap_or_default();
                    }
                }
            }
            KeyCode::Char('b') => {
                if let Some(row) = self.items.get(self.selected).cloned() {
                    // 复用规划钩子中的项目归属流程 (空/Esc 跳过)
                    self.set_mode(Mode::PlanningProject);
                    self.input.clear();
                    self.status_message =
                        format!("{} 归到哪个项目? (空/Esc 跳过)", short_id(&row.id));
                }
            }
            KeyCode::Char('W') => {
                if let Some(row) = self.items.get(self.selected).cloned() {
                    if let Ok(t) = tasks::get(self.conn, &row.id) {
                        self.set_mode(Mode::EditingDelegated);
                        self.input = t.delegated_to.clone().unwrap_or_default();
                    }
                }
            }
            KeyCode::Char('T') => {
                if let Some(row) = self.items.get(self.selected).cloned() {
                    if let Ok(t) = tasks::get(self.conn, &row.id) {
                        if t.kind == task::TaskKind::Project {
                            self.set_mode(Mode::EditingProjectType);
                            self.input.clear();
                            self.status_message = format!(
                                "{} 项目类型? (parallel/sequential, 空/Esc 跳过)",
                                short_id(&row.id)
                            );
                        } else {
                            self.status_message = "仅项目可设置项目类型".into();
                        }
                    }
                }
            }
            KeyCode::Char('C') => {
                let pomo = crate::repo::pomodoro::get_state().unwrap_or_default();
                self.set_mode(Mode::ConfiguringPomo);
                self.input = format!(
                    "{};{};{}",
                    pomo.config.work_mins,
                    pomo.config.short_break_mins,
                    pomo.config.long_break_mins
                );
            }
            KeyCode::Char('g') => self.move_sel(-10000),
            KeyCode::Char('G') => self.move_sel(10000),
            KeyCode::Char('A') | KeyCode::Char('D') | KeyCode::Delete => {
                if self.view == View::Tags {
                    if let Some(row) = self.items.get(self.selected).cloned() {
                        let tag_name = row.title.trim_start_matches('@');
                        match tags::delete_tag(self.conn, tag_name) {
                            Ok(_) => {
                                self.status_message = format!("Tag @{} deleted", tag_name);
                                self.refresh()?;
                            }
                            Err(e) => {
                                self.status_message = format!("Delete failed: {}", e);
                            }
                        }
                    }
                    return Ok(());
                }
                let mut ids = vec![];
                if self.mode == Mode::Visual && !self.selected_ids.is_empty() {
                    ids.extend(self.selected_ids.iter().cloned());
                } else if let Some(row) = self.items.get(self.selected).cloned() {
                    ids.push(row.id);
                }
                if ids.is_empty() {
                    return Ok(());
                }
                self.pending_archive_ids = ids;
                self.set_mode(Mode::ConfirmArchive);
                self.status_message = format!(
                    "确认归档 {} 项? (y/Enter 确认, n/Esc 取消)",
                    self.pending_archive_ids.len()
                );
            }
            KeyCode::Enter => self.act_on_selected(task::Status::Next)?,
            KeyCode::Char('K') if self.items.get(self.selected).is_some() => {
                self.set_mode(Mode::ChecklistAdding);
                self.input.clear();
            }
            KeyCode::Char(' ') => {
                if let Ok(pomo) = crate::repo::pomodoro::get_state() {
                    let is_in_break = matches!(
                        pomo.phase,
                        crate::model::pomodoro::Phase::ShortBreak
                            | crate::model::pomodoro::Phase::LongBreak
                    );
                    // 跨天检测：Idle 且 last_date 是今天，才触发续杯逻辑
                    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
                    let today_active = pomo.last_date.as_deref() == Some(today.as_str());
                    let is_post_break_idle = pomo.phase == crate::model::pomodoro::Phase::Idle
                        && today_active
                        && (pomo.today_count > 0 || pomo.task_id.is_some());

                    if is_in_break || is_post_break_idle {
                        let target_id = self
                            .items
                            .get(self.selected)
                            .map(|r| r.id.clone())
                            .or(pomo.task_id);
                        if let Some(tid) = target_id {
                            if let Ok(t) = tasks::get(self.conn, &tid) {
                                if t.status != task::Status::Next && t.status != task::Status::Done
                                {
                                    let _ = tasks::transition(self.conn, &tid, task::Status::Next);
                                    let _ = self.refresh();
                                }
                            }
                            let _ = crate::commands::pomo::start(self.conn, &tid);
                            self.status_message =
                                format!("🚀 零摩擦开启新一轮专注！ ({})", short_id(&tid));
                            self.load_detail();
                            return Ok(());
                        }
                    }
                }

                if let Some(row) = self.items.get(self.selected).cloned() {
                    if let Ok(mut task) = tasks::get(self.conn, &row.id) {
                        if !task.checklist.is_empty() {
                            let mut toggled_title = String::new();
                            if let Some(item) = task.checklist.iter_mut().find(|i| !i.done) {
                                item.done = true;
                                toggled_title = item.title.clone();
                            }
                            if !toggled_title.is_empty() {
                                let _ =
                                    tasks::update_checklist(self.conn, &task.id, &task.checklist);
                                self.status_message = format!("打卡: {}", toggled_title);
                                self.load_detail();
                            } else {
                                // 全部已完成时，按空格则全部重置为未完成
                                for item in task.checklist.iter_mut() {
                                    item.done = false;
                                }
                                let _ =
                                    tasks::update_checklist(self.conn, &task.id, &task.checklist);
                                self.status_message = "重置检查单".to_string();
                                self.load_detail();
                            }
                        }
                    }
                }
            }
            KeyCode::Char('P') => {
                let target_id = self
                    .items
                    .get(self.selected)
                    .map(|r| r.id.clone())
                    .or_else(|| {
                        crate::repo::pomodoro::get_state()
                            .ok()
                            .and_then(|s| s.task_id)
                    });
                if let Some(tid) = target_id {
                    if let Ok(t) = tasks::get(self.conn, &tid) {
                        if t.status != task::Status::Next && t.status != task::Status::Done {
                            let _ = tasks::transition(self.conn, &tid, task::Status::Next);
                            let _ = self.refresh();
                        }
                    }
                    let _ = crate::commands::pomo::start(self.conn, &tid);
                    self.status_message =
                        format!("🎯 Focus & Pomodoro started for {}", short_id(&tid));
                    self.load_detail();
                }
            }
            KeyCode::Char('S') => {
                let _ = crate::commands::pomo::stop();
                self.status_message.clear();
            }
            KeyCode::Char('/') => {
                self.set_mode(Mode::Search);
                self.input = self.search_query.clone();
            }
            KeyCode::Char('u') => {
                let _ = self.restore_selected();
            }
            KeyCode::Char('f') => {
                self.set_mode(Mode::FilteringTag);
                self.input = self.tag_filter.clone().unwrap_or_default();
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_input(&mut self, key: KeyEvent) -> Result<()> {
        if self.mode == Mode::SchedulingCalendar {
            if let Some(res) = self.calendar.handle_key(key.code) {
                match res {
                    Some((start, end)) => {
                        self.sched_dates = Some((start, end));
                        self.set_mode(Mode::SchedulingTimeRRule);
                        self.input.clear();
                    }
                    None => {
                        self.set_mode(Mode::Normal);
                    }
                }
            }
            return Ok(());
        }

        if self.mode == Mode::ConfirmArchive {
            match key.code {
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    let ids = std::mem::take(&mut self.pending_archive_ids);
                    let mut count = 0;
                    for id in &ids {
                        if let Ok(task) = tasks::get(self.conn, id) {
                            if matches!(
                                task.status,
                                task::Status::Done | task::Status::Waiting | task::Status::Someday
                            ) {
                                if let Ok(pomo) = crate::repo::pomodoro::get_state() {
                                    if pomo.task_id.as_deref() == Some(id) {
                                        let _ = crate::commands::pomo::stop();
                                    }
                                }
                            }
                            if tasks::archive(self.conn, id).is_ok() {
                                count += 1;
                            }
                        }
                    }
                    self.set_mode(Mode::Normal);
                    self.status_message = format!("archived {} items", count);
                    if self.mode == Mode::Visual {
                        self.selected_ids.clear();
                        self.visual_start_idx = None;
                    }
                    self.refresh()?;
                    self.load_detail();
                }
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.pending_archive_ids.clear();
                    self.set_mode(Mode::Normal);
                    self.status_message = "归档已取消".into();
                    self.refresh()?;
                    self.load_detail();
                }
                _ => {}
            }
            return Ok(());
        }

        match key.code {
            KeyCode::Esc => {
                self.set_mode(Mode::Normal);
                self.input.clear();
                self.refresh()?;
                self.load_detail();
            }
            KeyCode::Enter => {
                let input = self.input.clone();
                let mode = self.mode;
                self.set_mode(Mode::Normal);
                self.input.clear();
                self.confirm_input(mode, &input)?;
            }
            KeyCode::Tab => {
                if matches!(
                    self.mode,
                    Mode::Tagging | Mode::FilteringTag | Mode::Capturing
                ) {
                    let last_word = self
                        .input
                        .split_whitespace()
                        .last()
                        .unwrap_or("")
                        .to_string();
                    let tag_token = if let Some(stripped) = last_word.strip_prefix('@') {
                        stripped.to_string()
                    } else {
                        last_word.to_string()
                    };
                    if !tag_token.is_empty() {
                        // 优先硬编码的核心 5 个快捷标签，再动态查询 DB 数据库中所有已知标签（包括自定义标签）
                        let default_tags = ["home", "work", "errands", "quick", "focus"];
                        let mut all_tag_names: Vec<String> =
                            default_tags.iter().map(|s| s.to_string()).collect();

                        if let Ok(db_tags) = tags::list_tags(self.conn) {
                            for t in db_tags {
                                if !all_tag_names.contains(&t.name) {
                                    all_tag_names.push(t.name);
                                }
                            }
                        }

                        if let Some(matched) =
                            all_tag_names.iter().find(|p| p.starts_with(&tag_token))
                        {
                            let prefix_len = tag_token.len();
                            if !last_word.starts_with('@') {
                                let last_word_len = last_word.len();
                                let new_len = self.input.len() - last_word_len;
                                self.input.truncate(new_len);
                                self.input.push('@');
                                self.input.push_str(&last_word);
                            }
                            self.input.push_str(&matched[prefix_len..]);
                            self.input.push(' ');
                        }
                    }
                }
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(c) => self.input.push(c),
            _ => {}
        }
        Ok(())
    }

    fn confirm_input(&mut self, mode: Mode, input: &str) -> Result<()> {
        match mode {
            Mode::Search => {
                self.search_query = input.trim().to_string();
                if self.search_query.is_empty() {
                    self.status_message = "Search cleared".into();
                } else {
                    self.status_message = format!("Search: {}", self.search_query);
                }
                self.refresh()?;
                self.load_detail();
            }
            Mode::FilteringTag => {
                let t = input.trim().to_string();
                if t.is_empty() {
                    self.tag_filter = None;
                    self.status_message = "Tag filter cleared".into();
                } else {
                    self.tag_filter = Some(t.clone());
                    self.status_message = format!("Filter tag: @{}", t);
                }
                self.refresh()?;
                self.load_detail();
            }
            Mode::EditingTitle => {
                let title = input.trim();
                if !title.is_empty() {
                    if let Some(row) = self.items.get(self.selected).cloned() {
                        tasks::rename(self.conn, &row.id, title)?;
                        self.status_message = format!("renamed {}", short_id(&row.id));
                        self.refresh()?;
                        self.load_detail();
                    }
                }
            }
            Mode::EditingDue => {
                if let Some(row) = self.items.get(self.selected).cloned() {
                    let inp = input.trim();
                    if inp.is_empty() {
                        tasks::set_due(self.conn, &row.id, None)?;
                        self.status_message = format!("due cleared {}", short_id(&row.id));
                    } else {
                        match time::parse_time(inp) {
                            Ok(ms) => {
                                tasks::set_due(self.conn, &row.id, Some(ms))?;
                                self.status_message = format!("due set {}", short_id(&row.id));
                            }
                            Err(e) => self.status_message = format!("bad time: {}", e),
                        }
                    }
                    self.refresh()?;
                    self.load_detail();
                }
            }
            Mode::EditingRrule => {
                if let Some(row) = self.items.get(self.selected).cloned() {
                    let inp = input.trim();
                    let rrule = if inp.is_empty() {
                        None
                    } else {
                        Some(inp.to_string())
                    };
                    let set = rrule.is_some();
                    tasks::set_rrule(self.conn, &row.id, rrule)?;
                    self.status_message = if set {
                        format!("rrule set {}", short_id(&row.id))
                    } else {
                        format!("rrule cleared {}", short_id(&row.id))
                    };
                    self.refresh()?;
                    self.load_detail();
                }
            }
            Mode::EditingDelegated => {
                if let Some(row) = self.items.get(self.selected).cloned() {
                    let inp = input.trim();
                    let who = if inp.is_empty() {
                        None
                    } else {
                        Some(inp.to_string())
                    };
                    let set = who.is_some();
                    tasks::set_delegated(self.conn, &row.id, who)?;
                    self.status_message = if set {
                        format!("delegated {}", short_id(&row.id))
                    } else {
                        format!("delegated cleared {}", short_id(&row.id))
                    };
                    self.refresh()?;
                    self.load_detail();
                }
            }
            Mode::EditingProjectType => {
                if let Some(row) = self.items.get(self.selected).cloned() {
                    let inp = input.trim();
                    if !inp.is_empty() {
                        match inp.parse::<task::ProjectType>() {
                            Ok(pt) => {
                                tasks::set_project_type(self.conn, &row.id, pt)?;
                                self.status_message = format!("project type {}", short_id(&row.id));
                            }
                            Err(e) => self.status_message = format!("bad type: {}", e),
                        }
                    }
                    self.refresh()?;
                    self.load_detail();
                }
            }
            Mode::Capturing => {
                let raw_input = input.trim();
                if !raw_input.is_empty() {
                    let quick_add = crate::parser::parse_quick_add(raw_input);
                    let due_at = if let Some(ref t) = quick_add.time_str {
                        time::parse_time(t).ok()
                    } else {
                        None
                    };
                    let t = tasks::create_capture(
                        self.conn,
                        &CaptureInput {
                            title: quick_add.title,
                            kind: task::TaskKind::Action,
                            parent_id: None,
                            status: if due_at.is_some() {
                                task::Status::Scheduled
                            } else {
                                task::Status::Inbox
                            },
                            due_at,
                            tag_names: quick_add.tags,
                            ..Default::default()
                        },
                    )?;
                    self.set_view(View::Inbox);
                    self.status_message = format!("captured {}", short_id(&t.id));
                }
            }
            Mode::Tagging => {
                let name = input.trim();
                if !name.is_empty() {
                    let mut ids = vec![];
                    if !self.selected_ids.is_empty() {
                        ids.extend(self.selected_ids.iter().cloned());
                    } else if let Some(row) = self.items.get(self.selected).cloned() {
                        ids.push(row.id);
                    }

                    let mut count = 0;
                    for id in ids {
                        if tags::add_tag_to_task(self.conn, &id, name).is_ok() {
                            count += 1;
                        }
                    }
                    self.status_message = format!("tagged {} items with +{}", count, name);
                    self.selected_ids.clear();
                    self.visual_start_idx = None;
                    self.refresh()?;
                    self.load_detail();
                }
            }
            Mode::SchedulingCalendar => {}
            Mode::SchedulingTimeRRule => {
                if let Some((start_d, end_d)) = self.sched_dates.take() {
                    let parts: Vec<&str> = input.splitn(2, ';').collect();
                    let time_part = parts[0].trim();
                    let rrule_part = parts
                        .get(1)
                        .map(|s| s.trim_start_matches("rrule=").trim().to_string());
                    let final_rrule = rrule_part.filter(|r| !r.is_empty());

                    let (start_t_str, end_t_str) = if time_part.contains('-') {
                        let mut s = time_part.splitn(2, '-');
                        (
                            s.next().unwrap_or("00:00").trim(),
                            s.next().unwrap_or("23:59").trim(),
                        )
                    } else if !time_part.is_empty() {
                        (time_part, "23:59")
                    } else {
                        ("00:00", "23:59")
                    };

                    let start_time = chrono::NaiveTime::parse_from_str(start_t_str, "%H:%M")
                        .unwrap_or_else(|_| chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap());
                    let end_time = chrono::NaiveTime::parse_from_str(end_t_str, "%H:%M")
                        .unwrap_or_else(|_| chrono::NaiveTime::from_hms_opt(23, 59, 59).unwrap());

                    let start_ms = start_d
                        .and_time(start_time)
                        .and_local_timezone(chrono::Local)
                        .single()
                        .map(|t| t.timestamp_millis())
                        .unwrap_or_else(|| {
                            start_d.and_time(start_time).and_utc().timestamp_millis()
                        });
                    let end_ms = end_d
                        .and_time(end_time)
                        .and_local_timezone(chrono::Local)
                        .single()
                        .map(|t| t.timestamp_millis())
                        .unwrap_or_else(|| end_d.and_time(end_time).and_utc().timestamp_millis());

                    if let Some(row) = self.items.get(self.selected).cloned() {
                        let _ = tasks::schedule(
                            self.conn,
                            &row.id,
                            start_ms,
                            Some(end_ms),
                            final_rrule,
                        );
                        self.status_message = format!("scheduled {}", short_id(&row.id));
                        self.refresh().unwrap_or(());
                        self.load_detail();
                    }
                }
            }
            Mode::WaitingWho => {
                if let Some(row) = self.items.get(self.selected).cloned() {
                    let who = input.trim();
                    if !who.is_empty() {
                        let new_title = format!("{} [Wait: {}]", row.title, who);
                        tasks::rename(self.conn, &row.id, &new_title)?;
                    }
                    self.set_mode(Mode::WaitingWhen);
                    self.input.clear();
                    return Ok(());
                }
            }
            Mode::WaitingWhen => {
                let mut start_s = input.trim();
                if start_s.is_empty() {
                    start_s = "+1d";
                }
                if let Some(row) = self.items.get(self.selected).cloned() {
                    match time::parse_time(start_s) {
                        Ok(start_ms) => {
                            tasks::schedule(self.conn, &row.id, start_ms, None, None)?;
                            let t = tasks::transition(self.conn, &row.id, task::Status::Waiting)?;
                            self.status_message = format!("{} -> waiting", short_id(&t.id));
                            self.refresh()?;
                            self.load_detail();
                        }
                        Err(e) => self.status_message = format!("bad time: {}", e),
                    }
                }
            }
            Mode::PlanningProject => {
                let name = input.trim();
                if let Some(row) = self.items.get(self.selected).cloned() {
                    if !name.is_empty() {
                        // 接受项目 id、id 前缀或标题。
                        if let Ok(pid) = tasks::resolve_project(self.conn, name) {
                            tasks::assign_project(self.conn, &row.id, &pid)?;
                            self.status_message = format!("{} -> project", short_id(&row.id));
                        } else {
                            self.status_message = format!("project not found: {}", name);
                        }
                        self.refresh()?;
                        self.load_detail();
                    }
                    // 无论是否填了项目，都进入时间步骤（为空则跳过）。
                    if let Some(row) = self.items.get(self.selected).cloned() {
                        if let Ok(t) = tasks::get(self.conn, &row.id) {
                            if Self::needs_time(&t) {
                                self.set_mode(Mode::PlanningTime);
                                self.input.clear();
                                self.status_message =
                                    format!("{} 预计开始/截止? (空/Esc 跳过)", short_id(&row.id));
                                return Ok(());
                            }
                        }
                    }
                    self.set_mode(Mode::Normal);
                    self.refresh()?;
                    self.load_detail();
                }
            }
            Mode::PlanningTime => {
                let start_s = input.trim();
                if let Some(row) = self.items.get(self.selected).cloned() {
                    if !start_s.is_empty() {
                        match time::parse_time(start_s) {
                            Ok(start_ms) => {
                                tasks::set_due(self.conn, &row.id, Some(start_ms))?;
                                self.status_message = format!("due set {}", short_id(&row.id));
                            }
                            Err(e) => self.status_message = format!("bad time: {}", e),
                        }
                    }
                    self.set_mode(Mode::Normal);
                    self.refresh()?;
                    self.load_detail();
                }
            }
            Mode::ChecklistAdding => {
                if !input.is_empty() {
                    if let Some(row) = self.items.get(self.selected).cloned() {
                        if let Ok(mut task) = tasks::get(self.conn, &row.id) {
                            task.checklist.push(task::ChecklistItem {
                                id: uuid::Uuid::new_v4().to_string(),
                                title: input.to_string(),
                                done: false,
                            });
                            let _ = tasks::update_checklist(self.conn, &task.id, &task.checklist);
                            self.status_message = "Checklist +1".to_string();
                            self.load_detail();
                        }
                    }
                }
                self.set_mode(Mode::Normal);
                self.input.clear();
            }
            Mode::CreatingTag => {
                let name = input.trim().trim_start_matches('@');
                if !name.is_empty() && tags::find_or_create_tag(self.conn, name).is_ok() {
                    self.status_message = format!("created tag: {}", name);
                    self.refresh()?;
                }
                self.set_mode(Mode::Normal);
                self.input.clear();
            }
            Mode::ConfiguringPomo => {
                let parts: Vec<&str> = input.split(';').map(|s| s.trim()).collect();
                if parts.len() == 3 {
                    if let (Ok(w), Ok(s), Ok(l)) = (
                        parts[0].parse::<u32>(),
                        parts[1].parse::<u32>(),
                        parts[2].parse::<u32>(),
                    ) {
                        if w > 0 && s > 0 && l > 0 {
                            let mut pomo = crate::repo::pomodoro::get_state().unwrap_or_default();
                            pomo.config.work_mins = w;
                            pomo.config.short_break_mins = s;
                            pomo.config.long_break_mins = l;
                            if crate::repo::pomodoro::save_state(&pomo).is_ok() {
                                self.status_message = format!(
                                    "🍅 番茄钟配置已更新: 工作 {}m / 短休 {}m / 长休 {}m",
                                    w, s, l
                                );
                            }
                        } else {
                            self.status_message = "时长必须大于0".into();
                        }
                    } else {
                        self.status_message = "配置格式错误 (示例: 25;5;15)".into();
                    }
                } else {
                    self.status_message = "格式必须包含3项 (工作;短休;长休)".into();
                }
                self.set_mode(Mode::Normal);
                self.input.clear();
            }
            Mode::Normal | Mode::Visual | Mode::ConfirmArchive => {}
        }
        Ok(())
    }

    /// Restore the currently selected archived task (only meaningful in the
    /// Archived view). No-op outside that view or if nothing is selected.
    fn restore_selected(&mut self) -> Result<()> {
        if self.view != View::Archived {
            return Ok(());
        }
        if let Some(row) = self.items.get(self.selected).cloned() {
            if tasks::unarchive(self.conn, &row.id).is_ok() {
                self.status_message = format!("restored {}", short_id(&row.id));
                self.refresh()?;
                self.load_detail();
            }
        }
        Ok(())
    }
}
