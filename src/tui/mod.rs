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
use crate::model::task::Task;
use crate::repo::tasks::{self, CaptureInput, ListFilter};
use crate::repo::tags;
use crate::time;

mod ui;
use ui::{build_list_items, status_color, status_letter};

const SHORT_HELP: &str =
    " j/k nav · Ctrl+H/L pane · 1 inbox 2 next · p project · r review · a add · x done · w wait · s someday · c schedule · t tag · A archive · q quit · ? help";
const LONG_HELP: &str =
    "j/k or ↑/↓ navigate · Ctrl+H/Ctrl+L switch focus pane\n\
     1 inbox · 2 next · 3 waiting · 4 scheduled · 5 someday · 6 reference · 7 done\n\
     p projects tree · r weekly review · a capture · x mark done · w waiting · s someday\n\
     c schedule (<start> [;rrule=...]) · t add tag · A archive · Enter=next · q quit · ? toggle";

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
    /// 当前视图对应 GTD 工作流的哪个阶段，用于在引导栏高亮。
    fn stage(self) -> &'static str {
        match self {
            View::Inbox => "clarify",
            View::Next => "engage",
            View::Projects => "organize",
            View::Review => "reflect",
            _ => "engage",
        }
    }
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
    /// 在左侧栏中以“折叠”形式展示的上下文分组。
    fn context_groups() -> &'static [View] {
        &[
            View::Waiting,
            View::Scheduled,
            View::Someday,
            View::Reference,
            View::Done,
        ]
    }
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
    Scheduling,
    /// 计划钩子第 1 步：询问归属项目。
    PlanningProject,
    /// 计划钩子第 2 步：询问预计时间。
    PlanningTime,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pane {
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
        };
        app.refresh()?;
        app.load_detail();
        Ok(app)
    }

    fn total_count(&self) -> usize {
        tasks::list(
            self.conn,
            &ListFilter {
                status: None,
                project: None,
                tags: vec![],
            },
        )
        .map(|v| v.len())
        .unwrap_or(0)
    }

    fn context_count(&self, v: View) -> usize {
        if let Some(s) = v.status() {
            tasks::list(
                self.conn,
                &ListFilter {
                    status: Some(s.to_string()),
                    project: None,
                    tags: vec![],
                },
            )
            .map(|v| v.len())
            .unwrap_or(0)
        } else {
            0
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
                .filter(|t| t.kind == "project")
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
                            status: Some(s.to_string()),
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

    fn switch_pane(&mut self) {
        self.pane = match self.pane {
            Pane::Center => Pane::Right,
            Pane::Right => Pane::Center,
        };
    }

    /// 一个 next 任务若仍缺计划信息（项目 和/或 时间），返回 true。
    fn needs_planning(t: &Task) -> bool {
        let missing_project = t.parent_id.is_none();
        let missing_time = t.due_at.is_none() && t.scheduled_start_at.is_none();
        missing_project || missing_time
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

    /// 把任务置为 next；若缺计划信息，则启动可选的分步补全钩子（可跳过）。
    fn act_next(&mut self, row: Row) -> Result<()> {
        let t = tasks::transition(self.conn, &row.id, "next")?;
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

    fn act_on_selected(&mut self, to: &str) -> Result<()> {
        if let Some(row) = self.items.get(self.selected).cloned() {
            if row.status == to {
                self.status_message = format!("already {}", to);
                return Ok(());
            }
            if to == "next" {
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
            KeyCode::Char('?') => self.show_help = !self.show_help,
            KeyCode::Down | KeyCode::Char('j') => self.move_sel(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_sel(-1),
            KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.pane = Pane::Center
            }
            KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => self.switch_pane(),
            KeyCode::Char(d) if d.is_ascii_digit() => {
                if let Some(v) = View::from_digit(d) {
                    self.set_view(v);
                }
            }
            KeyCode::Char('p') => self.set_view(View::Projects),
            KeyCode::Char('r') => self.set_view(View::Review),
            KeyCode::Char('a') => {
                self.mode = Mode::Capturing;
                self.input.clear();
            }
            KeyCode::Char('x') => self.act_on_selected("done")?,
            KeyCode::Char('w') => self.act_on_selected("waiting")?,
            KeyCode::Char('s') => self.act_on_selected("someday")?,
            KeyCode::Char('c') => {
                self.mode = Mode::Scheduling;
                self.input.clear();
            }
            KeyCode::Char('t') => {
                self.mode = Mode::Tagging;
                self.input.clear();
            }
            KeyCode::Char('A') => {
                if let Some(row) = self.items.get(self.selected).cloned() {
                    tasks::archive(self.conn, &row.id)?;
                    self.status_message = format!("archived {}", &row.id[..8]);
                    self.refresh()?;
                    self.load_detail();
                }
            }
            KeyCode::Enter => self.act_on_selected("next")?,
            _ => {}
        }
        Ok(())
    }

    fn handle_input(&mut self, key: KeyEvent) -> Result<()> {
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
                            kind: "action".to_string(),
                            parent_id: None,
                            status: "inbox".to_string(),
                            due_at: None,
                            tag_names: vec![],
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
            Mode::Scheduling => {
                let parts: Vec<&str> = input.splitn(2, ';').collect();
                let start_s = parts[0].trim();
                let rrule = parts
                    .get(1)
                    .map(|s| s.trim_start_matches("rrule=").trim().to_string());
                if let Some(row) = self.items.get(self.selected).cloned() {
                    match time::parse_time(start_s) {
                        Ok(start_ms) => {
                            let t = tasks::schedule(
                                self.conn,
                                &row.id,
                                start_ms,
                                None,
                                rrule,
                            )?;
                            self.status_message = format!("scheduled {}", &t.id[..8]);
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

    fn render(&mut self, f: &mut ratatui::Frame) {
        self.list_state.select(Some(self.selected));
        let size = f.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(0), Constraint::Length(3)])
            .split(size);

        // 顶栏
        let header = Line::from(vec![
            Span::styled(
                " gtp ",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("· {}   ", self.view.label())),
            Span::styled(&self.status_message, Style::default().fg(Color::Green)),
        ]);
        f.render_widget(Paragraph::new(header), chunks[0]);

        // 三栏：引导栏 | 列表 | 详情
        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(22),
                Constraint::Percentage(46),
                Constraint::Percentage(32),
            ])
            .split(chunks[1]);

        self.render_guide(f, body[0]);
        match self.view {
            View::Review => self.render_review(f, body[1]),
            _ => self.render_list(f, body[1]),
        }
        self.render_detail(f, body[2]);

        // 底栏
        let footer = if self.mode != Mode::Normal {
            match self.mode {
                Mode::Capturing => format!(" New task: {}_", self.input),
                Mode::Tagging => format!(" Add tag: {}_", self.input),
                Mode::Scheduling => format!(" Schedule <start> [;rrule=...]: {}_", self.input),
                Mode::PlanningProject => format!(" Project? {}_", self.input),
                Mode::PlanningTime => format!(" Time? {}_", self.input),
                Mode::Normal => String::new(),
            }
        } else if self.show_help {
            LONG_HELP.to_string()
        } else {
            SHORT_HELP.to_string()
        };
        f.render_widget(
            Paragraph::new(footer).block(Block::default().borders(Borders::ALL)),
            chunks[2],
        );
    }

    fn render_guide(&self, f: &mut ratatui::Frame, area: Rect) {
        let empty = self.total_count() == 0;
        let mut lines: Vec<Line> = Vec::new();

        if empty {
            lines.push(Line::from(Span::styled(
                " Welcome to gtp",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(" Press 'a' to capture"));
            lines.push(Line::from(" your first task and"));
            lines.push(Line::from(" start the GTD flow"));
            lines.push(Line::from(""));
        }

        lines.push(Line::from(Span::styled(
            " Workflow",
            Style::default().add_modifier(Modifier::UNDERLINED),
        )));
        let stages = [
            ("收集 Capture", "a 收集", "capture"),
            ("理清 Clarify", "Enter→next", "clarify"),
            ("组织 Organize", "p 项目", "organize"),
            ("回顾 Reflect", "r 周回顾", "reflect"),
            ("行动 Engage", "做 next", "engage"),
        ];
        for (name, key, stage) in stages {
            let active = self.view.stage() == stage;
            let style = if active {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            lines.push(Line::from(vec![
                Span::styled(format!(" {}{}", if active { "▸ " } else { "  " }, name), style),
                Span::styled(format!("  {}", key), Style::default().fg(Color::DarkGray)),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " Contexts",
            Style::default().add_modifier(Modifier::UNDERLINED),
        )));
        for v in View::context_groups() {
            let c = self.context_count(*v);
            let color = status_color(v.status().unwrap_or(""));
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {} {} ", status_letter(v.status().unwrap_or("")), v.label()),
                    Style::default().fg(color),
                ),
                Span::raw(format!(" {}", c)),
            ]));
        }

        let title = if self.pane == Pane::Center {
            " Guide "
        } else {
            " Guide "
        };
        f.render_widget(
            Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title)),
            area,
        );
    }

    fn render_list(&mut self, f: &mut ratatui::Frame, area: Rect) {
        let items = build_list_items(self);
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!("Tasks · {}", self.view.label())),
            )
            .highlight_style(
                if self.pane == Pane::Center {
                    Style::default().bg(Color::DarkGray)
                } else {
                    Style::default()
                },
            )
            .highlight_symbol("▶ ");
        f.render_stateful_widget(list, area, &mut self.list_state);
    }

    fn render_detail(&self, f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        let block = Block::default().borders(Borders::ALL).title("Detail");
        let para = match &self.detail {
            None => Paragraph::new("No selection").block(block),
            Some(d) => {
                let mut lines: Vec<Line> = Vec::new();
                lines.push(Line::from(Span::styled(
                    d.task.title.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(format!("status : {}", d.task.status)));
                if let Some(p) = &d.task.parent_id {
                    lines.push(Line::from(format!("project: {}", &p[..8])));
                }
                lines.push(Line::from(format!(
                    "due    : {}",
                    time::format_local(d.task.due_at)
                )));
                lines.push(Line::from(format!(
                    "sched  : {} -> {}",
                    time::format_local(d.task.scheduled_start_at),
                    time::format_local(d.task.scheduled_end_at)
                )));
                lines.push(Line::from(format!(
                    "tags   : {}",
                    d.tags
                        .iter()
                        .map(|t| t.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "timeline",
                    Style::default().add_modifier(Modifier::UNDERLINED),
                )));
                for e in d.events.iter().rev().take(8).rev() {
                    lines.push(Line::from(format!(
                        "  {}  {:<14} {} -> {}",
                        time::format_local(Some(e.at)),
                        e.event_type,
                        e.from_status.as_deref().unwrap_or("-"),
                        e.to_status.as_deref().unwrap_or("-")
                    )));
                }
                Paragraph::new(lines).block(block)
            }
        };
        f.render_widget(para, area);
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
        let c = |s: &str| all.iter().filter(|t| t.status == s).count();
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

fn row_from(t: &Task, indent: usize, conn: &Connection) -> Result<Row> {
    let tags = tags::get_task_tags(conn, &t.id)?
        .iter()
        .map(|x| x.name.clone())
        .collect();
    Ok(Row {
        id: t.id.clone(),
        title: t.title.clone(),
        status: t.status.clone(),
        due: t.due_at.or(t.scheduled_start_at),
        tags,
        indent,
    })
}

/// 启动交互式 TUI。
pub fn run(conn: &Connection) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, conn);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<Stdout>>, conn: &Connection) -> Result<()> {
    let mut app = App::new(conn)?;
    loop {
        terminal.draw(|f| app.render(f))?;
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Release {
                    continue;
                }
                app.handle_key(key)?;
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
                kind: "project".into(),
                parent_id: None,
                status: "next".into(),
                due_at: None,
                tag_names: vec![],
            },
        )
        .unwrap();
        let mk = |title: &str, kind: &str, parent: Option<&str>, status: &str, tags: &[&str]| {
            tasks::create_capture(
                conn,
                &CaptureInput {
                    title: title.into(),
                    kind: kind.into(),
                    parent_id: parent.map(|s| s.to_string()),
                    status: status.into(),
                    due_at: None,
                    tag_names: tags.iter().map(|s| s.to_string()).collect(),
                },
            )
            .unwrap();
        };
        mk("Write homepage copy", "action", Some(&proj.id), "inbox", &["work", "p1"]);
        mk("Buy groceries", "action", None, "inbox", &["home", "errands"]);
        mk("Read Rust book", "action", None, "next", &["learning"]);
        mk("Pay taxes", "action", None, "waiting", &["work", "p2"]);
        mk("Plan vacation", "action", None, "someday", &["home"]);
        mk("Finish report", "action", None, "done", &[]);
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
        let s = frame("1-initial-inbox", &mut term, &mut app, &mut out);
        assert!(s.contains("Workflow"), "左侧引导栏应渲染");
        assert!(s.contains("Contexts"), "上下文分组应渲染");
        assert!(s.contains("Tasks · Inbox"), "中栏列表标题");
        assert!(s.contains("Detail"), "右侧详情栏");
        assert!(s.contains("Buy groceries"), "inbox 列出已灌入的任务");
        assert!(s.contains("Waiting") && s.contains("Someday"), "上下文分组已列出");

        // 2) vim 导航：下、上
        app.handle_key(key('j')).unwrap();
        frame("2-nav-down", &mut term, &mut app, &mut out);
        app.handle_key(key('k')).unwrap();
        frame("3-nav-up", &mut term, &mut app, &mut out);

        // 3) Ctrl+L 把焦点切到右栏（详情高亮关闭，列表正常）
        app.handle_key(ctr('l')).unwrap();
        frame("4-pane-right", &mut term, &mut app, &mut out);
        assert!(app.pane == Pane::Right, "Ctrl+L 把焦点移到右栏");
        app.handle_key(ctr('h')).unwrap();
        frame("5-pane-center", &mut term, &mut app, &mut out);
        assert!(app.pane == Pane::Center, "Ctrl+H 把焦点移回中栏");

        // 4) 收集后自动跳回 Inbox
        app.handle_key(key('a')).unwrap();
        let s = frame("6-capture-mode", &mut term, &mut app, &mut out);
        assert!(s.contains("New task:"), "收集提示");
        for c in "Buy milk".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let s = frame("7-after-capture", &mut term, &mut app, &mut out);
        assert!(s.contains("Buy milk"), "新收集的任务出现");
        assert!(s.contains("· Inbox"), "收集后跳到 Inbox");

        // 5) 回车 -> next 触发计划钩子（先问项目，再问时间）
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let s = frame("8-plan-project", &mut term, &mut app, &mut out);
        assert!(s.contains("Project?"), "计划钩子询问项目");
        // 跳过项目
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let s = frame("9-plan-time", &mut term, &mut app, &mut out);
        assert!(s.contains("Time?"), "计划钩子询问时间");
        // 跳过时间 -> 回到正常；被计划的任务已是 next（已离开 inbox）
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let _ = frame("10-after-plan", &mut term, &mut app, &mut out);
        assert!(app.mode == Mode::Normal, "计划钩子结束");
        let in_next = tasks::list(
            &conn,
            &ListFilter {
                status: Some("next".into()),
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
            let s = frame(lbl, &mut term, &mut app, &mut out);
            assert!(s.contains(expect), "视图 {lbl} 应显示 {expect}");
        }

        // 7) 项目树 + 周回顾
        app.handle_key(key('p')).unwrap();
        let s = frame("17-projects", &mut term, &mut app, &mut out);
        assert!(s.contains("Website Redesign"), "项目视图");
        app.handle_key(key('r')).unwrap();
        let s = frame("18-review", &mut term, &mut app, &mut out);
        assert!(s.contains("Weekly Review"), "回顾视图");

        // 8) 在非 inbox 视图收集后自动跳回 Inbox
        app.handle_key(key('3')).unwrap();
        app.handle_key(key('a')).unwrap();
        for c in "Captured from waiting".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let s = frame("19-capture-jump", &mut term, &mut app, &mut out);
        assert!(s.contains("· Inbox"), "从 waiting 视图收集后跳到 Inbox");
        assert!(s.contains("Captured from waiting"));

        // 9) 标签 + 排程流程
        app.handle_key(key('t')).unwrap();
        for c in "urgent".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let s = frame("20-after-tag", &mut term, &mut app, &mut out);
        assert!(s.contains("urgent"), "标签已添加");
        app.handle_key(key('c')).unwrap();
        for c in "+2h".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let s = frame("21-after-schedule", &mut term, &mut app, &mut out);
        assert!(s.contains("sched"), "显示排程时间");

        // 10) 归档 + 帮助切换 + 退出
        app.handle_key(key('4')).unwrap();
        app.handle_key(key('A')).unwrap();
        frame("22-after-archive", &mut term, &mut app, &mut out);
        app.handle_key(key('?')).unwrap();
        let s = frame("23-help", &mut term, &mut app, &mut out);
        assert!(s.contains("navigate"), "help text");
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
        let s = snap(&term);
        let mut out = std::fs::File::create("/tmp/gtp_empty_guide.txt").unwrap();
        out.write_all(s.as_bytes()).unwrap();
        assert!(s.contains("Welcome to gtp"), "empty db should show welcome guide");
        assert!(s.contains("GTD flow"), "welcome copy");
    }
}
