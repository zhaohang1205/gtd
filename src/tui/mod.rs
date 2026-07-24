use std::io::{self, Stdout};
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListState, Paragraph},
    text::{Line, Span},
    Terminal,
};
use rusqlite::Connection;

use crate::model::event::TaskEvent;
use crate::model::tag::Tag;
use crate::model::task::{self, Task};
use crate::repo::tasks::{self, CaptureInput, ListFilter};
use crate::repo::tags;
use crate::time;

mod ui;
mod calendar;
use ui::build_list_items;



fn visual_len(s: &str) -> usize {
    s.chars().map(|c| {
        if c.is_ascii() || (c >= '\u{E000}' && c <= '\u{F8FF}') {
            1
        } else {
            2
        }
    }).sum()
}

fn pad_right(s: &str, width: usize) -> String {
    let len = visual_len(s);
    if len < width {
        format!("{}{}", s, " ".repeat(width - len))
    } else {
        s.to_string()
    }
}

/// GTD 的七个状态（数据层不变）。界面里只有 Inbox 和 Next 是“主视图”，
/// 其余状态作为可折叠的“上下文分组”放在左侧引导栏，既保持可达，
/// 又不会把前台铺得太满造成心理负担。
#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Inbox,
    Next,
    Waiting,
    Scheduled,
    Someday,
    Reference,
    Done,
    Projects,
    Review,
}

impl View {
    fn label(self) -> &'static str {
        match self {
            View::Inbox => "Inbox",
            View::Next => "Next",
            View::Waiting => "Waiting",
            View::Scheduled => "Scheduled",
            View::Someday => "Someday",
            View::Reference => "Reference",
            View::Done => "Done",
            View::Projects => "Projects",
            View::Review => "Review",
        }
    }

    /// 状态视图对应的状态字符串（用于查询与中文展示）。
    fn status(self) -> Option<&'static str> {
        match self {
            View::Inbox => Some("inbox"),
            View::Next => Some("next"),
            View::Waiting => Some("waiting"),
            View::Scheduled => Some("scheduled"),
            View::Someday => Some("someday"),
            View::Reference => Some("reference"),
            View::Done => Some("done"),
            View::Projects | View::Review => None,
        }
    }

    /// 数字键 1-7 映射到的视图。
    fn from_digit(d: char) -> Option<View> {
        match d {
            '1' => Some(View::Inbox),
            '2' => Some(View::Next),
            '3' => Some(View::Waiting),
            '4' => Some(View::Scheduled),
            '5' => Some(View::Someday),
            '6' => Some(View::Reference),
            '7' => Some(View::Done),
            _ => None,
        }
    }

}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Capturing,
    Tagging,
    SchedulingCalendar,
    SchedulingTimeRRule,
    WaitingWho,
    WaitingWhen,
    /// 计划钩子第 1 步：询问归属项目。
    PlanningProject,
    /// 计划钩子第 2 步：询问预计时间。
    PlanningTime,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pane {
    Left,
    Center,
    Right,
}

#[derive(Clone)]
struct Row {
    id: String,
    title: String,
    status: String,
    due: Option<i64>,
    tags: Vec<String>,
    indent: usize,
}

struct DetailData {
    task: Task,
    tags: Vec<Tag>,
    events: Vec<TaskEvent>,
}

pub struct App<'a> {
    conn: &'a Connection,
    view: View,
    items: Vec<Row>,
    selected: usize,
    list_state: ListState,
    detail: Option<DetailData>,
    mode: Mode,
    pane: Pane,
    input: String,
    status_message: String,
    show_help: bool,
    should_quit: bool,
    calendar: calendar::CalendarState,
    sched_dates: Option<(chrono::NaiveDate, chrono::NaiveDate)>,
}

impl<'a> App<'a> {
    fn new(conn: &'a Connection) -> Result<Self> {
        let mut app = App {
            conn,
            view: View::Inbox,
            items: Vec::new(),
            selected: 0,
            list_state: ListState::default(),
            detail: None,
            mode: Mode::Normal,
            pane: Pane::Center,
            input: String::new(),
            status_message: String::new(),
            show_help: false,
            should_quit: false,
            calendar: calendar::CalendarState::new(),
            sched_dates: None,
        };
        app.refresh()?;
        app.load_detail();
        Ok(app)
    }

    fn total_count(&self) -> usize {
        tasks::count(
            self.conn,
            &ListFilter {
                status: None,
                project: None,
                tags: vec![],
            },
        )
        .unwrap_or(0)
    }

    fn context_count(&self, v: View) -> usize {
        match v.status() {
            Some(s) => tasks::count(
                self.conn,
                &ListFilter {
                    status: Some(s.parse::<task::Status>().unwrap_or(task::Status::Inbox)),
                    project: None,
                    tags: vec![],
                },
            )
            .unwrap_or(0),
            None => 0,
        }
    }

    fn refresh(&mut self) -> Result<()> {
        self.items.clear();
        match self.view {
            View::Projects => {
                let projects = tasks::list(
                    self.conn,
                    &ListFilter {
                        status: None,
                        project: None,
                        tags: vec![],
                    },
                )?
                .into_iter()
                .filter(|t| t.kind == task::TaskKind::Project)
                .collect::<Vec<_>>();
                for p in projects {
                    self.items.push(row_from(&p, 0, self.conn)?);
                    let actions = tasks::list(
                        self.conn,
                        &ListFilter {
                            status: None,
                            project: Some(p.id.clone()),
                            tags: vec![],
                        },
                    )?;
                    for a in actions {
                        self.items.push(row_from(&a, 1, self.conn)?);
                    }
                }
            }
            _ => {
                if let Some(s) = self.view.status() {
                    let ts = tasks::list(
                        self.conn,
                        &ListFilter {
                            status: Some(s.parse::<task::Status>().unwrap_or(task::Status::Inbox)),
                            project: None,
                            tags: vec![],
                        },
                    )?;
                    for t in ts {
                        self.items.push(row_from(&t, 0, self.conn)?);
                    }
                }
            }
        }
        if self.selected >= self.items.len() {
            self.selected = self.items.len().saturating_sub(1);
        }
        Ok(())
    }

    fn load_detail(&mut self) {
        self.detail = None;
        if let Some(row) = self.items.get(self.selected) {
            if let Ok(task) = tasks::get(self.conn, &row.id) {
                let tg = tags::get_task_tags(self.conn, &row.id).unwrap_or_default();
                let ev = tasks::events(self.conn, &row.id).unwrap_or_default();
                self.detail = Some(DetailData {
                    task,
                    tags: tg,
                    events: ev,
                });
            }
        }
    }

    fn set_view(&mut self, v: View) {
        self.view = v;
        self.selected = 0;
        self.status_message.clear();
        if let Err(e) = self.refresh() {
            self.status_message = format!("err: {}", e);
        }
        self.load_detail();
    }

    fn move_sel(&mut self, delta: isize) {
        if self.items.is_empty() {
            self.selected = 0;
            self.load_detail();
            return;
        }
        let n = self.items.len() as isize;
        let mut s = self.selected as isize + delta;
        if s < 0 {
            s = 0;
        }
        if s >= n {
            s = n - 1;
        }
        self.selected = s as usize;
        self.load_detail();
    }



    fn next_view(&mut self, delta: isize) {
        let views = [
            View::Inbox,
            View::Next,
            View::Waiting,
            View::Scheduled,
            View::Someday,
            View::Reference,
            View::Done,
            View::Projects,
            View::Review,
        ];
        let idx = views.iter().position(|v| *v == self.view).unwrap_or(0) as isize;
        let mut next_idx = idx + delta;
        if next_idx < 0 {
            next_idx = views.len() as isize - 1;
        } else if next_idx >= views.len() as isize {
            next_idx = 0;
        }
        self.set_view(views[next_idx as usize]);
    }

    /// 一个 next 任务若仍缺计划信息（项目 和/或 时间），返回 true。
    fn needs_planning(t: &Task) -> bool {
        let missing_project = t.parent_id.is_none();
        let missing_time = t.due_at.is_none() && t.scheduled_start_at.is_none();
        missing_project || missing_time
    }

    /// 把任务置为 next；若缺计划信息，则启动可选的分步补全钩子（可跳过）。
    fn act_next(&mut self, row: Row) -> Result<()> {
        let t = tasks::transition(self.conn, &row.id, task::Status::Next)?;
        self.status_message = format!("{} -> next", &t.id[..8]);
        if Self::needs_planning(&t) {
            self.mode = Mode::PlanningProject;
            self.input.clear();
            let hint = Self::planning_hint(&t);
            self.status_message = format!("{} 归到哪个项目? (空/Esc 跳过) {}", &t.id[..8], hint);
        } else {
            self.refresh()?;
            self.load_detail();
        }
        Ok(())
    }

    fn act_on_selected(&mut self, to: task::Status) -> Result<()> {
        if let Some(row) = self.items.get(self.selected).cloned() {
            if row.status == to.to_string() {
                self.status_message = format!("already {}", to);
                return Ok(());
            }
            if to == task::Status::Next {
                return self.act_next(row);
            }
            let t = tasks::transition(self.conn, &row.id, to)?;
            self.status_message = format!("{} -> {}", &t.id[..8], t.status);
            self.refresh()?;
            self.load_detail();
        }
        Ok(())
    }

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
            KeyCode::Char('r') => self.set_view(View::Review),
            KeyCode::Char('a') => {
                self.mode = Mode::Capturing;
                self.input.clear();
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
                if let Some(row) = self.items.get(self.selected).cloned() {
                    tasks::archive(self.conn, &row.id)?;
                    self.status_message = format!("archived {}", &row.id[..8]);
                    self.refresh()?;
                    self.load_detail();
                }
            }
            KeyCode::Enter => self.act_on_selected(task::Status::Next)?,
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
                // 补全钩子里按 Esc 表示跳过当前（及后续）步骤。
                self.mode = Mode::Normal;
                self.input.clear();
                self.refresh()?;
                self.load_detail();
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
            Mode::Capturing => {
                let title = input.trim();
                if !title.is_empty() {
                    let t = tasks::create_capture(
                        self.conn,
                        &CaptureInput {
                            title: title.to_string(),
                            kind: task::TaskKind::Action,
                            parent_id: None,
                            status: task::Status::Inbox,
                            due_at: None,
                            tag_names: vec![],
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
                    if let Some(row) = self.items.get(self.selected).cloned() {
                        tags::add_tag_to_task(self.conn, &row.id, name)?;
                        self.status_message = format!("tagged {} +{}", &row.id[..8], name);
                        self.refresh()?;
                        self.load_detail();
                    }
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
            Mode::Normal => {}
        }
        Ok(())
    }

    fn needs_time(t: &Task) -> bool {
        t.due_at.is_none() && t.scheduled_start_at.is_none()
    }

    fn planning_hint(t: &Task) -> String {
        let mut missing = Vec::new();
        if t.parent_id.is_none() {
            missing.push("项目");
        }
        if t.due_at.is_none() && t.scheduled_start_at.is_none() {
            missing.push("时间");
        }
        if missing.is_empty() {
            String::new()
        } else {
            format!("建议补充{} (t 加项目, c 排期)", missing.join("/"))
        }
    }

    fn render(&mut self, f: &mut ratatui::Frame) {
        self.list_state.select(Some(self.selected));
        let size = f.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(size);

        // 三栏：引导栏 | 列表 | 详情
        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(22),
                Constraint::Percentage(46),
                Constraint::Percentage(32),
            ])
            .split(chunks[0]);

        let left_chunks = if self.show_help {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(10), Constraint::Length(14)])
                .split(body[0])
        } else {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(100)])
                .split(body[0])
        };

        self.render_guide(f, left_chunks[0]);
        if self.show_help {
            self.render_help_drawer(f, left_chunks[1]);
        }

        match self.view {
            View::Review => self.render_review(f, body[1]),
            _ => self.render_list(f, body[1]),
        }
        self.render_detail(f, body[2]);

        // 状态栏 (Statusline)
        let mode_str = if self.mode == Mode::Normal { " NORMAL " } else { " INSERT " };
        let mode_bg = if self.mode == Mode::Normal { Color::Blue } else { Color::Yellow };
        
        let pomo = crate::repo::pomodoro::get_state().unwrap_or_default();
        let pomo_str = if pomo.phase != crate::model::pomodoro::Phase::Idle {
            let now = crate::time::now_ms();
            let end_ts = pomo.end_ts.unwrap_or(now);
            let mut diff = (end_ts - now) / 1000;
            if diff < 0 { diff = 0; }
            let m = diff / 60;
            let s = diff % 60;
            format!("  {:02}:{:02} [{}] ", m, s, pomo.task_title.as_deref().unwrap_or(""))
        } else {
            String::new()
        };

        let status_left = Line::from(vec![
            Span::styled(mode_str, Style::default().fg(Color::Black).bg(mode_bg).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {} ", self.view.label()), Style::default().fg(Color::White).bg(Color::DarkGray).add_modifier(Modifier::BOLD)),
            if !pomo_str.is_empty() {
                Span::styled(pomo_str, Style::default().fg(Color::White).bg(Color::Red).add_modifier(Modifier::BOLD))
            } else {
                Span::raw("")
            },
        ]);

        let status_msg = if self.status_message.is_empty() {
            String::new()
        } else {
            format!(" {} ", self.status_message)
        };
        let status_right = Line::from(vec![
            Span::styled(status_msg, Style::default().fg(Color::Yellow).bg(Color::DarkGray)),
        ]);

        let status_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(chunks[1]);
            
        f.render_widget(Paragraph::new(status_left).style(Style::default().bg(Color::DarkGray)), status_layout[0]);
        f.render_widget(Paragraph::new(status_right).alignment(ratatui::layout::Alignment::Right).style(Style::default().bg(Color::DarkGray)), status_layout[1]);

        if self.mode != Mode::Normal && self.mode != Mode::SchedulingCalendar {
            let title = match self.mode {
                Mode::Capturing => " New task ",
                Mode::Tagging => " Add tag (Hints: home, work, errands...) ",
                Mode::SchedulingTimeRRule => " 设定时间与循环规则 (格式: 15:00-16:00 ;FREQ=DAILY) ",
                Mode::SchedulingCalendar => "", // Not reached
                Mode::WaitingWho => " Waiting for who/what? ",
                Mode::WaitingWhen => " Reminder time? (e.g. +1d, tomorrow 10:00) ",
                Mode::PlanningProject => " Project? ",
                Mode::PlanningTime => " Time? ",
                Mode::Normal => "",
            };
            let text = format!(" {}_", self.input);
            let area = Self::centered_rect(50, 3, size);
            f.render_widget(ratatui::widgets::Clear, area);
            let block = Block::default().title(title).borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow));
            f.render_widget(Paragraph::new(text).block(block), area);
        }
        if self.mode == Mode::SchedulingCalendar {
            let area = Self::centered_rect(60, 15, size);
            self.calendar.render(f, area);
        }
    }

    fn render_help_drawer(&self, f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" 快捷键 (F1/?) ");
            
        let mut rows = vec![];
        let keys = [
            ("h/l", "切换面板"),
            ("j/k", "上下移动"),
            ("P", "番茄钟"),
            ("a", "收集任务"),
            ("w", "标记等待"),
            ("s", "标记将来"),
            ("c", "排期时间"),
            ("t", "添加标签"),
            ("x", "标记完成"),
            ("D", "归档任务"),
            ("Ent", "下一步"),
            ("q", "退出"),
        ];

        for (k, desc) in keys.iter() {
            rows.push(ratatui::widgets::Row::new(vec![
                ratatui::text::Line::from(Span::styled(format!("{:>5} ", k), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
                ratatui::text::Line::from(Span::raw(*desc)),
            ]));
        }

        let widths = [Constraint::Length(6), Constraint::Min(0)];
        let table = ratatui::widgets::Table::new(rows, widths).block(block).column_spacing(1);
        f.render_widget(table, area);
    }

fn centered_rect(percent_x: u16, height: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(r.height.saturating_sub(height) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

    fn render_guide(&self, f: &mut ratatui::Frame, area: Rect) {
        let mut lines: Vec<Line> = Vec::new();

        if self.total_count() == 0 {
            lines.push(Line::from(Span::styled(
                " 欢迎使用 gtp",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
        }

        let cur = self.view;
        let is_left_pane = self.pane == Pane::Left;

        let mut add_group = |views: &[(char, View)], title: &'static str| {
            lines.push(Line::from(Span::styled(
                title,
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
            )));
            for (key, v) in views {
                let cnt = self.context_count(*v);
                let active = cur == *v;
                let (icon, label) = match v {
                    View::Inbox => ("", "收件箱"),
                    View::Next => ("", "下一步"),
                    View::Waiting => ("", "等待中"),
                    View::Scheduled => ("", "已排程"),
                    View::Someday => ("", "将来/也许"),
                    View::Reference => ("", "参考资料"),
                    View::Done => ("", "已完成"),
                    View::Projects => ("", "项目树"),
                    View::Review => ("", "周回顾"),
                };
                let padded_label = pad_right(label, 10);

                if active {
                    let mut style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
                    if is_left_pane {
                        style = style.add_modifier(Modifier::REVERSED);
                    }
                    lines.push(Line::from(Span::styled(
                        format!(" ▶ {} {} {} {:>3} ", key, icon, padded_label, cnt),
                        style,
                    )));
                } else {
                    lines.push(Line::from(vec![
                        Span::styled(format!("   {} ", key), Style::default().fg(Color::DarkGray)),
                        Span::raw(format!("{} {} {:>3} ", icon, padded_label, cnt)),
                    ]));
                }
            }
            lines.push(Line::from(""));
        };

        add_group(&[('1', View::Inbox), ('2', View::Next)], "  [Active]");
        add_group(&[('3', View::Waiting), ('4', View::Scheduled), ('5', View::Someday)], "  [Waiting]");
        add_group(&[('6', View::Reference), ('7', View::Done)], "  [Archive]");
        
        lines.push(Line::from(Span::styled(
            "  [Modules]",
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
        )));
        for (key, v) in &[('p', View::Projects), ('r', View::Review)] {
            let active = cur == *v;
            let (icon, label) = match v {
                View::Projects => ("", "项目树"),
                View::Review => ("", "周回顾"),
                _ => ("", ""),
            };
            let padded_label = pad_right(label, 10);
            if active {
                let mut style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
                if is_left_pane {
                    style = style.add_modifier(Modifier::REVERSED);
                }
                lines.push(Line::from(Span::styled(
                    format!(" ▶ {} {} {}     ", key, icon, padded_label),
                    style,
                )));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(format!("   {} ", key), Style::default().fg(Color::DarkGray)),
                    Span::raw(format!("{} {}     ", icon, padded_label)),
                ]));
            }
        }
        lines.push(Line::from(""));

        // 提示
        lines.push(Line::from(Span::styled("  [Hint]", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD))));
        lines.push(Line::from(Span::styled(format!("  {}", next_hint(self.view)), Style::default().fg(Color::Gray))));

        let border_color = if self.pane == Pane::Left { Color::Yellow } else { Color::DarkGray };
        f.render_widget(
            Paragraph::new(lines).block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(border_color)).title(" Guide ")),
            area,
        );
    }

    fn render_list(&mut self, f: &mut ratatui::Frame, area: Rect) {
        let border_color = if self.pane == Pane::Center { Color::Yellow } else { Color::DarkGray };
        let items = build_list_items(self);
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(border_color))
                    .title(format!("Tasks · {}", self.view.label())),
            )
            .highlight_style(
                if self.pane == Pane::Center {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                },
            );
        f.render_stateful_widget(list, area, &mut self.list_state);
    }

    fn render_detail(&self, f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        let border_color = if self.pane == Pane::Right { Color::Yellow } else { Color::DarkGray };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(" 任务详情 ");

        match &self.detail {
            None => {
                f.render_widget(Paragraph::new(" 未选中任务").block(block), area);
            }
            Some(d) => {
                let mut rows = vec![];
                rows.push(ratatui::widgets::Row::new(vec![
                    Line::from(Span::styled(" 标题", Style::default().add_modifier(Modifier::BOLD))),
                    Line::from(Span::styled(d.task.title.clone(), Style::default().add_modifier(Modifier::BOLD))),
                ]));
                rows.push(ratatui::widgets::Row::new(vec![
                    Line::from(" 状态"),
                    Line::from(status_cn(d.task.status.clone())),
                ]));
                if let Some(p) = &d.task.parent_id {
                    rows.push(ratatui::widgets::Row::new(vec![
                        Line::from(" 项目"),
                        Line::from(p[..8].to_string()),
                    ]));
                }
                rows.push(ratatui::widgets::Row::new(vec![
                    Line::from(" 截止时间"),
                    Line::from(time::format_local(d.task.due_at)),
                ]));
                if d.task.scheduled_start_at.is_some() || d.task.scheduled_end_at.is_some() {
                    rows.push(ratatui::widgets::Row::new(vec![
                        Line::from(" 计划时间"),
                        Line::from(format!("{} -> {}", time::format_local(d.task.scheduled_start_at), time::format_local(d.task.scheduled_end_at))),
                    ]));
                }
                if let Some(st) = d.task.started_at {
                    rows.push(ratatui::widgets::Row::new(vec![
                        Line::from(" 开始时间"),
                        Line::from(time::format_local(Some(st))),
                    ]));
                }
                if let Some(ct) = d.task.completed_at {
                    rows.push(ratatui::widgets::Row::new(vec![
                        Line::from(" 完成时间"),
                        Line::from(time::format_local(Some(ct))),
                    ]));
                }
                
                if let Some(rr) = &d.task.rrule {
                    let cn_rr = rr.replace("FREQ=DAILY", "每天")
                        .replace("FREQ=WEEKLY", "每周")
                        .replace("FREQ=MONTHLY", "每月")
                        .replace("INTERVAL=", "间隔=")
                        .replace("COUNT=", "次数=")
                        .replace("UNTIL=", "直到=");
                    
                    rows.push(ratatui::widgets::Row::new(vec![
                        Line::from(" 循环规则"),
                        Line::from(cn_rr),
                    ]));
                }
                
                let mut tag_line = vec![];
                for (i, t) in d.tags.iter().enumerate() {
                    tag_line.push(Span::styled(t.name.clone(), Style::default().fg(Color::Cyan)));
                    if i < d.tags.len() - 1 {
                        tag_line.push(Span::raw(", "));
                    }
                }
                rows.push(ratatui::widgets::Row::new(vec![
                    Line::from(" 标签"),
                    Line::from(tag_line),
                ]));

                if let Some(del) = &d.task.delegated_to {
                    rows.push(ratatui::widgets::Row::new(vec![
                        Line::from(" 委派给"),
                        Line::from(del.clone()),
                    ]));
                }
                
                if d.task.kind == task::TaskKind::Project {
                    rows.push(ratatui::widgets::Row::new(vec![
                        Line::from(" 项目类型"),
                        Line::from(d.task.project_type.to_string()),
                    ]));
                }
                
                if !d.task.checklist.is_empty() {
                    let mut cl_lines = vec![];
                    for item in &d.task.checklist {
                        let check = if item.done { "[x]" } else { "[ ]" };
                        cl_lines.push(format!("{} {}", check, item.title));
                    }
                    rows.push(ratatui::widgets::Row::new(vec![
                        Line::from(" 检查单"),
                        Line::from(cl_lines.join("  ")), // joining them since it's one line for now, or we can use multiple lines but Row might crop
                    ]));
                }

                let pomo_count = d.events.iter().filter(|e| e.event_type == "pomodoro").count();
                if pomo_count > 0 {
                    let tomatoes = " ".repeat(pomo_count);
                    rows.push(ratatui::widgets::Row::new(vec![
                        Line::from(" 番茄钟"),
                        Line::from(format!("{} ({})", tomatoes, pomo_count)),
                    ]));
                }

                rows.push(ratatui::widgets::Row::new(vec![Line::from(""), Line::from("")]));
                rows.push(ratatui::widgets::Row::new(vec![
                    Line::from(Span::styled(" 时间线", Style::default().add_modifier(Modifier::UNDERLINED))),
                    Line::from(""),
                ]));
                
                for e in d.events.iter().rev().take(8).rev() {
                    let event_cn = match e.event_type.as_str() {
                        "created" => "创建任务",
                        "status_change" => "状态流转",
                        "pomodoro" => "完成专注",
                        _ => &e.event_type,
                    };
                    
                    let from_cn = e.from_status.as_deref().unwrap_or("-").parse::<crate::model::task::Status>().map(status_cn).unwrap_or("-");
                    let to_cn = e.to_status.as_deref().unwrap_or("-").parse::<crate::model::task::Status>().map(status_cn).unwrap_or("-");
                    
                    let action = if e.event_type == "status_change" {
                        format!("{} -> {}", from_cn, to_cn)
                    } else if e.event_type == "pomodoro" {
                        "🍅 +1".to_string()
                    } else {
                        "".to_string()
                    };

                    rows.push(ratatui::widgets::Row::new(vec![
                        Line::from(time::format_local(Some(e.at))),
                        Line::from(format!("{} {}", event_cn, action)),
                    ]));
                }

                let widths = [Constraint::Length(12), Constraint::Min(0)];
                let table = ratatui::widgets::Table::new(rows, widths).block(block);
                f.render_widget(table, area);
            }
        }
    }

    fn render_review(&self, f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        let all = tasks::list(
            self.conn,
            &ListFilter {
                status: None,
                project: None,
                tags: vec![],
            },
        )
        .unwrap_or_default();
        let c = |s: &str| all.iter().filter(|t| t.status.to_string() == s).count();
        let lines = vec![
            Line::from("Weekly Review"),
            Line::from(format!("  inbox     : {}", c("inbox"))),
            Line::from(format!("  next      : {}", c("next"))),
            Line::from(format!("  waiting   : {}", c("waiting"))),
            Line::from(format!("  scheduled : {}", c("scheduled"))),
            Line::from(format!("  someday   : {}", c("someday"))),
            Line::from(format!("  reference : {}", c("reference"))),
            Line::from(format!("  done      : {}", c("done"))),
        ];
        let para = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Review"));
        f.render_widget(para, area);
    }
}

/// 状态的中文含义，用于引导栏的“状态地图”。
fn status_cn(s: task::Status) -> &'static str {
    match s {
        task::Status::Inbox => "收件箱",
        task::Status::Next => "下一步",
        task::Status::Waiting => "等待中",
        task::Status::Scheduled => "已排程",
        task::Status::Someday => "将来/也许",
        task::Status::Reference => "参考资料",
        task::Status::Done => "已完成",
    }
}

/// 根据当前视图，给出“下一步该做什么”的提示。
fn next_hint(v: View) -> &'static str {
    match v {
        View::Inbox => "按 Enter 理清，决定它的去向",
        View::Next => "选一条开始行动（做）",
        View::Waiting => "跟进被阻塞的事项",
        View::Scheduled => "按排程时间执行",
        View::Someday => "定期回顾是否激活",
        View::Reference => "需要时检索查阅",
        View::Done => "可归档已完成事项",
        View::Projects => "把收件箱行动归入项目",
        View::Review => "清空各类积压",
    }
}

fn row_from(t: &Task, indent: usize, conn: &Connection) -> Result<Row> {
    let tags = tags::get_task_tags(conn, &t.id)?
        .iter()
        .map(|x| x.name.clone())
        .collect();
    Ok(Row {
        id: t.id.clone(),
        title: t.title.clone(),
        status: t.status.to_string(),
        due: t.due_at.or(t.scheduled_start_at),
        tags,
        indent,
    })
}

/// 启动交互式 TUI。
pub fn run(conn: &Connection) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, crossterm::event::EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, conn);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, crossterm::event::DisableMouseCapture)?;
    terminal.show_cursor()?;
    result
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<Stdout>>, conn: &Connection) -> Result<()> {
    let mut app = App::new(conn)?;
    loop {
        terminal.draw(|f| app.render(f))?;
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind == KeyEventKind::Release {
                        continue;
                    }
                    app.handle_key(key)?;
                }
                Event::Mouse(m) => {
                    match m.kind {
                        crossterm::event::MouseEventKind::ScrollDown => app.move_sel(1),
                        crossterm::event::MouseEventKind::ScrollUp => app.move_sel(-1),
                        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                            if m.column > terminal.size()?.width / 2 {
                                app.pane = Pane::Right;
                            } else {
                                app.pane = Pane::Center;
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        if app.should_quit {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrate;
    use crate::repo::tasks::{self, CaptureInput};
    use ratatui::backend::TestBackend;
    use std::io::Write;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty())
    }
    fn kc(k: KeyCode) -> KeyEvent {
        KeyEvent::new(k, KeyModifiers::empty())
    }
    fn ctr(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn seed(conn: &Connection) {
        let proj = tasks::create_capture(
            conn,
            &CaptureInput {
                title: "Website Redesign".into(),
                kind: task::TaskKind::Project,
                parent_id: None,
                status: task::Status::Next,
                due_at: None,
                tag_names: vec![],
                ..Default::default()
            },
        )
        .unwrap();
        let mk = |title: &str, kind: task::TaskKind, parent: Option<&str>, status: task::Status, tags: &[&str]| {
            tasks::create_capture(
                conn,
                &CaptureInput {
                    title: title.into(),
                    kind,
                    parent_id: parent.map(|s| s.to_string()),
                    status,
                    due_at: None,
                    tag_names: tags.iter().map(|s| s.to_string()).collect(),
                    ..Default::default()
                },
            )
            .unwrap();
        };
        mk("Write homepage copy", task::TaskKind::Action, Some(&proj.id), task::Status::Inbox, &["work", "p1"]);
        mk("Buy groceries", task::TaskKind::Action, None, task::Status::Inbox, &["home", "errands"]);
        mk("Read Rust book", task::TaskKind::Action, None, task::Status::Next, &["learning"]);
        mk("Pay taxes", task::TaskKind::Action, None, task::Status::Waiting, &["work", "p2"]);
        mk("Plan vacation", task::TaskKind::Action, None, task::Status::Someday, &["home"]);
        mk("Finish report", task::TaskKind::Action, None, task::Status::Done, &[]);
    }

    fn snap(term: &Terminal<TestBackend>) -> String {
        let buf = term.backend().buffer();
        let w = buf.area().width as usize;
        let h = buf.area().height as usize;
        let content = buf.content();
        let mut s = String::with_capacity(w * h * 2);
        for y in 0..h {
            for x in 0..w {
                s.push_str(content[y * w + x].symbol());
            }
            s.push('\n');
        }
        s
    }

    /// 去掉所有空格，规避无头快照里 CJK 字符被逐字加空格的渲染产物，
    /// 便于对中文文本做 contains 断言（真实终端无此问题）。
    fn norm(s: &str) -> String {
        s.chars().filter(|c| *c != ' ').collect()
    }

    #[test]
    fn drive_tui() {
        let mut conn = Connection::open(":memory:").unwrap();
        migrate::run(&mut conn).unwrap();
        seed(&conn);
        let mut app = App::new(&conn).unwrap();
        let mut term = Terminal::new(TestBackend::new(110, 30)).unwrap();
        let mut out = std::fs::File::create("/tmp/gtp_tui_frames.txt").unwrap();
        let frame = |label: &str, term: &mut Terminal<TestBackend>, app: &mut App, out: &mut std::fs::File| -> String {
            term.draw(|f| app.render(f)).unwrap();
            let s = snap(term);
            writeln!(out, "===== {label} =====").unwrap();
            out.write_all(s.as_bytes()).unwrap();
            s
        };

        // 1) 三栏布局：引导栏 + 列表 + 详情
        let s = norm(&frame("1-initial-inbox", &mut term, &mut app, &mut out));
        assert!(s.contains("Active"), "引导栏应显示分组");
        assert!(s.contains("收件箱"), "引导栏含中文含义");
        assert!(s.contains("Tasks·Inbox"), "中栏列表标题");
        assert!(s.contains("任务详情"), "右侧详情栏");
        assert!(s.contains("Buygroceries"), "inbox 列出已灌入的任务");
        assert!(s.contains("等待中") && s.contains("将来/也许"), "上下文分组已列出");

        // 2) vim 导航：下、上
        app.handle_key(key('j')).unwrap();
        frame("2-nav-down", &mut term, &mut app, &mut out);
        app.handle_key(key('k')).unwrap();
        frame("3-nav-up", &mut term, &mut app, &mut out);

        // 3) h/l 把焦点在 Left, Center, Right 之间切换
        app.handle_key(key('l')).unwrap();
        frame("4-pane-right", &mut term, &mut app, &mut out);
        assert!(app.pane == Pane::Right, "l 把焦点移到右栏");
        app.handle_key(key('h')).unwrap();
        frame("5-pane-center", &mut term, &mut app, &mut out);
        assert!(app.pane == Pane::Center, "h 把焦点移回中栏");
        app.handle_key(key('h')).unwrap();
        assert!(app.pane == Pane::Left, "h 把焦点移到左栏");
        app.handle_key(key('l')).unwrap();
        assert!(app.pane == Pane::Center, "l 把焦点移回中栏");

        // 4) 收集后自动跳回 Inbox
        app.handle_key(key('a')).unwrap();
        let s = norm(&frame("6-capture-mode", &mut term, &mut app, &mut out));
        assert!(s.contains("Newtask"), "收集提示");
        for c in "Buy milk".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let s = norm(&frame("7-after-capture", &mut term, &mut app, &mut out));
        assert!(s.contains("Buymilk"), "新收集的任务出现");
        assert!(s.contains("·Inbox"), "收集后跳到 Inbox");

        // 5) 回车 -> next 触发计划钩子（先问项目，再问时间）
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let s = norm(&frame("8-plan-project", &mut term, &mut app, &mut out));
        assert!(s.contains("Project?"), "计划钩子询问项目");
        // 跳过项目
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let s = norm(&frame("9-plan-time", &mut term, &mut app, &mut out));
        assert!(s.contains("Time?"), "计划钩子询问时间");
        // 跳过时间 -> 回到正常；被计划的任务已是 next（已离开 inbox）
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        frame("10-after-plan", &mut term, &mut app, &mut out);
        assert!(app.mode == Mode::Normal, "计划钩子结束");
        let in_next = tasks::list(
            &conn,
            &ListFilter {
                status: Some(task::Status::Next),
                project: None,
                tags: vec![],
            },
        )
        .unwrap()
        .iter()
        .any(|t| t.title == "Write homepage copy");
        assert!(in_next, "被计划的任务已进入 next");

        // 6) 用数字键切换视图
        for (d, lbl, expect) in [
            ('3', "11-waiting", "Waiting"),
            ('4', "12-scheduled", "Scheduled"),
            ('5', "13-someday", "Someday"),
            ('6', "14-reference", "Reference"),
            ('7', "15-done", "Done"),
            ('1', "16-back-inbox", "Inbox"),
        ] {
            app.handle_key(key(d)).unwrap();
            let s = norm(&frame(lbl, &mut term, &mut app, &mut out));
            assert!(s.contains(expect), "视图 {lbl} 应显示 {expect}");
        }

        // 7) 项目树 + 周回顾
        app.handle_key(key('p')).unwrap();
        let s = norm(&frame("17-projects", &mut term, &mut app, &mut out));
        assert!(s.contains("WebsiteRedesign"), "项目视图");
        app.handle_key(key('r')).unwrap();
        let s = norm(&frame("18-review", &mut term, &mut app, &mut out));
        assert!(s.contains("WeeklyReview"), "回顾视图");

        // 8) 在非 inbox 视图收集后自动跳回 Inbox
        app.handle_key(key('3')).unwrap();
        app.handle_key(key('a')).unwrap();
        for c in "Captured from waiting".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let s = norm(&frame("19-capture-jump", &mut term, &mut app, &mut out));
        assert!(s.contains("·Inbox"), "从 waiting 视图收集后跳到 Inbox");
        assert!(s.contains("Capturedfromwaiting"));

        // 9) 标签 + 排程流程
        app.handle_key(key('t')).unwrap();
        for c in "urgent".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let s = norm(&frame("20-after-tag", &mut term, &mut app, &mut out));
        assert!(s.contains("urgent"), "标签已添加");
        app.handle_key(key('c')).unwrap();
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let s = norm(&frame("21-after-schedule", &mut term, &mut app, &mut out));
        assert!(s.contains("sched"), "显示排程时间");

        // 10) 归档 + 帮助切换 + 退出
        app.handle_key(key('4')).unwrap();
        app.handle_key(key('A')).unwrap();
        frame("22-after-archive", &mut term, &mut app, &mut out);
        app.handle_key(key('?')).unwrap();
        let s = norm(&frame("23-help", &mut term, &mut app, &mut out));
        assert!(s.contains("快捷键"), "help text");
        app.handle_key(key('?')).unwrap();
        frame("24-help-off", &mut term, &mut app, &mut out);
        app.handle_key(key('q')).unwrap();
        assert!(app.should_quit, "q quits");
    }

    #[test]
    fn empty_db_shows_guide() {
        let mut conn = Connection::open(":memory:").unwrap();
        migrate::run(&mut conn).unwrap();
        let mut app = App::new(&conn).unwrap();
        let mut term = Terminal::new(TestBackend::new(110, 30)).unwrap();
        term.draw(|f| app.render(f)).unwrap();
        let raw = snap(&term);
        let mut out = std::fs::File::create("/tmp/gtp_empty_guide.txt").unwrap();
        out.write_all(raw.as_bytes()).unwrap();
        let s = norm(&raw);
        assert!(s.contains("欢迎使用gtp"), "empty db should show welcome guide");
        assert!(s.contains("Active"), "guide shows groups");
    }
}
