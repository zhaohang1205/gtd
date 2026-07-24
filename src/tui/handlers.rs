use super::app::{App, Mode, Pane, View};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crate::model::task::{self};
use crate::repo::{tasks, tags};
use crate::repo::tasks::CaptureInput;
use crate::time;
use super::calendar;

pub(crate) trait AppHandlers {
    fn handle_key(&mut self, key: KeyEvent) -> Result<()>;
    fn handle_normal(&mut self, key: KeyEvent) -> Result<()>;
    fn handle_input(&mut self, key: KeyEvent) -> Result<()>;
    fn confirm_input(&mut self, mode: Mode, input: &str) -> Result<()>;
}

impl<'a> AppHandlers for App<'a> {
    fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return Ok(());
        }
        match self.mode {
            Mode::Normal => self.handle_normal(key),
            _ => self.handle_input(key),
        }
    }

    fn handle_normal(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('?') | KeyCode::F(1) => self.show_help = !self.show_help,
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
                let _ = crate::commands::pomo::stop();
                self.status_message = "pomodoro stopped".into();
            }
            KeyCode::Char('p') => self.set_view(View::Projects),
            KeyCode::Char('r') => {
                self.is_reviewing = true;
                self.review_step = 1;
                self.set_view(View::Inbox);
                self.status_message = "Weekly Review started".into();
            }
            KeyCode::Char('R') => {
                if self.is_reviewing {
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
            }
            KeyCode::Char('a') => {
                self.mode = Mode::Capturing;
                self.input.clear();
            }
            KeyCode::Char('e') => {
                if let Some(row) = self.items.get(self.selected).cloned() {
                    self.mode = Mode::EditingTitle;
                    self.input = row.title.clone();
                }
            }
            KeyCode::Char('n') => {
                if let Some(row) = self.items.get(self.selected).cloned() {
                    if let Ok(task) = tasks::get(self.conn, &row.id) {
                        crossterm::terminal::disable_raw_mode()?;
                        crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
                        
                        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
                        let mut temp_file = tempfile::NamedTempFile::new()?;
                        use std::io::Write;
                        temp_file.write_all(task.notes.as_bytes())?;
                        
                        let _ = std::process::Command::new(editor).arg(temp_file.path()).status();
                        
                        if let Ok(new_notes) = std::fs::read_to_string(temp_file.path()) {
                            if new_notes != task.notes {
                                let _ = tasks::update_notes(self.conn, &task.id, &new_notes);
                            }
                        }
                        
                        crossterm::terminal::enable_raw_mode()?;
                        crossterm::execute!(std::io::stdout(), crossterm::terminal::EnterAlternateScreen)?;
                        self.load_detail();
                    }
                }
            }
            KeyCode::Char('x') => self.act_on_selected(task::Status::Done)?,
            KeyCode::Char('w') => {
                self.mode = Mode::WaitingWho;
                self.input.clear();
            }
            KeyCode::Char('s') => self.act_on_selected(task::Status::Someday)?,
            KeyCode::Char('c') => {
                self.mode = Mode::SchedulingCalendar;
                self.calendar = calendar::CalendarState::new();
                self.input.clear();
            }
            KeyCode::Char('t') => {
                self.mode = Mode::Tagging;
                self.input.clear();
            }
            KeyCode::Char('g') => self.move_sel(-10000),
            KeyCode::Char('G') => self.move_sel(10000),
            KeyCode::Char('A') | KeyCode::Char('D') | KeyCode::Delete => {
                let mut ids = vec![];
                if self.mode == Mode::Visual && !self.selected_ids.is_empty() {
                    ids.extend(self.selected_ids.iter().cloned());
                } else if let Some(row) = self.items.get(self.selected).cloned() {
                    ids.push(row.id);
                }
                for id in &ids {
                    let _ = tasks::archive(self.conn, id);
                }
                self.status_message = format!("archived {} items", ids.len());
                if self.mode == Mode::Visual {
                    self.mode = Mode::Normal;
                    self.selected_ids.clear();
                    self.visual_start_idx = None;
                }
                self.refresh()?;
                self.load_detail();
            }
            KeyCode::Enter => self.act_on_selected(task::Status::Next)?,
            KeyCode::Char('C') => {
                if self.items.get(self.selected).is_some() {
                    self.mode = Mode::ChecklistAdding;
                    self.input.clear();
                }
            }
            KeyCode::Char(' ') => {
                if let Some(row) = self.items.get(self.selected).cloned() {
                    if let Ok(mut task) = tasks::get(self.conn, &row.id) {
                        if !task.checklist.is_empty() {
                            let mut toggled_title = String::new();
                            if let Some(item) = task.checklist.iter_mut().find(|i| !i.done) {
                                item.done = true;
                                toggled_title = item.title.clone();
                            }
                            if !toggled_title.is_empty() {
                                let _ = tasks::update_checklist(self.conn, &task.id, &task.checklist);
                                self.status_message = format!("打卡: {}", toggled_title);
                                self.load_detail();
                            } else {
                                // 全部已完成时，按空格则全部重置为未完成
                                for item in task.checklist.iter_mut() {
                                    item.done = false;
                                }
                                let _ = tasks::update_checklist(self.conn, &task.id, &task.checklist);
                                self.status_message = "重置检查单".to_string();
                                self.load_detail();
                            }
                        }
                    }
                }
            }
            KeyCode::Char('P') => {
                if let Some(row) = self.items.get(self.selected).cloned() {
                    let _ = crate::commands::pomo::start(self.conn, &row.id);
                    self.status_message = format!("pomodoro started for {}", &row.id[..8]);
                }
            }
            KeyCode::Char('S') => {
                let _ = crate::commands::pomo::stop();
                self.status_message = "pomodoro stopped".into();
            }
            KeyCode::Char('/') => {
                self.mode = Mode::Search;
                self.input = self.search_query.clone();
            }
            KeyCode::Char('f') => {
                self.mode = Mode::FilteringTag;
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
                        self.mode = Mode::SchedulingTimeRRule;
                        self.input.clear();
                    }
                    None => {
                        self.mode = Mode::Normal;
                    }
                }
            }
            return Ok(());
        }

        match key.code {
            KeyCode::Esc => {
                if self.is_reviewing {
                    self.is_reviewing = false;
                    self.review_step = 0;
                    self.status_message = "Weekly Review cancelled".into();
                    self.refresh()?;
                    self.load_detail();
                } else if self.mode == Mode::Visual {
                    self.mode = Mode::Normal;
                    self.visual_start_idx = None;
                    self.selected_ids.clear();
                    self.status_message = "Exited visual mode".into();
                    self.refresh()?;
                    self.load_detail();
                } else if self.tag_filter.is_some() || !self.search_query.is_empty() {
                    self.mode = Mode::Normal;
                    self.tag_filter = None;
                    self.search_query.clear();
                    self.input.clear();
                    self.status_message = "Cleared filters".into();
                    self.refresh()?;
                    self.load_detail();
                } else {
                    self.mode = Mode::Normal;
                    self.input.clear();
                    self.refresh()?;
                    self.load_detail();
                }
            }
            KeyCode::Char('v') | KeyCode::Char('V') => {
                if self.mode == Mode::Visual {
                    self.mode = Mode::Normal;
                    self.visual_start_idx = None;
                    self.selected_ids.clear();
                    self.status_message = "Exited visual mode".into();
                } else {
                    self.mode = Mode::Visual;
                    self.visual_start_idx = Some(self.selected);
                    self.update_visual_selection();
                    self.status_message = "-- VISUAL --".into();
                }
                let _ = self.refresh();
            }
            KeyCode::Enter => {
                let input = self.input.clone();
                let mode = self.mode;
                self.mode = Mode::Normal;
                self.input.clear();
                self.confirm_input(mode, &input)?;
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
                        self.status_message = format!("renamed {}", &row.id[..8]);
                        self.refresh()?;
                        self.load_detail();
                    }
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
                            status: if due_at.is_some() { task::Status::Scheduled } else { task::Status::Inbox },
                            due_at,
                            tag_names: quick_add.tags,
                            ..Default::default()
                        },
                    )?;
                    self.set_view(View::Inbox);
                    self.status_message = format!("captured {}", &t.id[..8]);
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
                        if let Ok(_) = tags::add_tag_to_task(self.conn, &id, name) {
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
                    let rrule_part = parts.get(1).map(|s| s.trim_start_matches("rrule=").trim().to_string());
                    let final_rrule = if let Some(r) = rrule_part {
                        if r.is_empty() { None } else { Some(r) }
                    } else { None };

                    let (start_t_str, end_t_str) = if time_part.contains('-') {
                        let mut s = time_part.splitn(2, '-');
                        (s.next().unwrap_or("00:00").trim(), s.next().unwrap_or("23:59").trim())
                    } else if !time_part.is_empty() {
                        (time_part, "23:59")
                    } else {
                        ("00:00", "23:59")
                    };

                    let start_time = chrono::NaiveTime::parse_from_str(start_t_str, "%H:%M").unwrap_or_else(|_| chrono::NaiveTime::from_hms_opt(0,0,0).unwrap());
                    let end_time = chrono::NaiveTime::parse_from_str(end_t_str, "%H:%M").unwrap_or_else(|_| chrono::NaiveTime::from_hms_opt(23,59,59).unwrap());

                    let start_ms = start_d.and_time(start_time).and_local_timezone(chrono::Local).single().map(|t| t.timestamp_millis()).unwrap_or_else(|| start_d.and_time(start_time).and_utc().timestamp_millis());
                    let end_ms = end_d.and_time(end_time).and_local_timezone(chrono::Local).single().map(|t| t.timestamp_millis()).unwrap_or_else(|| end_d.and_time(end_time).and_utc().timestamp_millis());

                    if let Some(row) = self.items.get(self.selected).cloned() {
                        let _ = tasks::schedule(
                            self.conn,
                            &row.id,
                            start_ms,
                            Some(end_ms),
                            final_rrule,
                        );
                        self.status_message = format!("scheduled {}", &row.id[..8]);
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
                    self.mode = Mode::WaitingWhen;
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
                            self.status_message = format!("{} -> waiting", &t.id[..8]);
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
                            self.status_message = format!("{} -> project", &row.id[..8]);
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
                                self.mode = Mode::PlanningTime;
                                self.input.clear();
                                self.status_message =
                                    format!("{} 预计开始/截止? (空/Esc 跳过)", &row.id[..8]);
                                return Ok(());
                            }
                        }
                    }
                    self.mode = Mode::Normal;
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
                                tasks::set_due(self.conn, &row.id, start_ms)?;
                                self.status_message = format!("due set {}", &row.id[..8]);
                            }
                            Err(e) => self.status_message = format!("bad time: {}", e),
                        }
                    }
                    self.mode = Mode::Normal;
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
                            self.status_message = format!("Checklist +1");
                            self.load_detail();
                        }
                    }
                }
                self.mode = Mode::Normal;
                self.input.clear();
            }
            Mode::Normal | Mode::Visual => {}
        }
        Ok(())
    }

}
