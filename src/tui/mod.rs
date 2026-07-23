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
    layout::{Constraint, Direction, Layout},
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
use ui::build_list_items;

const SHORT_HELP: &str =
    " j/k nav · 1-7 view · p project · r review · a add · x done · w wait · s someday · c schedule · t tag · A archive · q quit · ? help";
const LONG_HELP: &str =
    "j/k or ↑/↓ navigate · 1 inbox 2 next 3 waiting 4 scheduled 5 someday 6 reference 7 done\n\
     p projects tree · r weekly review · a capture · x mark done · w waiting · s someday\n\
     c schedule (<start> [;rrule=...]) · t add tag · A archive · Enter=next · q quit · ? toggle";

#[derive(Clone, Copy)]
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

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Normal,
    Capturing,
    Tagging,
    Scheduling,
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
            input: String::new(),
            status_message: String::new(),
            show_help: false,
            should_quit: false,
        };
        app.refresh()?;
        app.load_detail();
        Ok(app)
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

    fn act_on_selected(&mut self, to: &str) -> Result<()> {
        if let Some(row) = self.items.get(self.selected).cloned() {
            if row.status == to {
                self.status_message = format!("already {}", to);
                return Ok(());
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
                self.mode = Mode::Normal;
                self.input.clear();
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
                            let t =
                                tasks::schedule(self.conn, &row.id, start_ms, None, rrule)?;
                            self.status_message = format!("scheduled {}", &t.id[..8]);
                            self.refresh()?;
                            self.load_detail();
                        }
                        Err(e) => self.status_message = format!("bad time: {}", e),
                    }
                }
            }
            Mode::Normal => {}
        }
        Ok(())
    }

    fn render(&mut self, f: &mut ratatui::Frame) {
        self.list_state.select(Some(self.selected));
        let size = f.area();
        let footer_h = if self.show_help {
            Constraint::Length(5)
        } else {
            Constraint::Length(3)
        };
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(0), footer_h])
            .split(size);

        let header = Line::from(vec![
            Span::styled(
                " gtp ",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("· {}   ", self.view.label())),
            Span::styled(&self.status_message, Style::default().fg(Color::Green)),
        ]);
        f.render_widget(Paragraph::new(header), chunks[0]);

        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .split(chunks[1]);

        match self.view {
            View::Review => self.render_review(f, body[0]),
            _ => {
                let items = build_list_items(self);
                let list = List::new(items)
                    .block(Block::default().borders(Borders::ALL).title("Tasks"))
                    .highlight_style(Style::default().bg(Color::DarkGray))
                    .highlight_symbol("▶ ");
                f.render_stateful_widget(list, body[0], &mut self.list_state);
            }
        }
        self.render_detail(f, body[1]);

        let footer = if self.mode != Mode::Normal {
            match self.mode {
                Mode::Capturing => format!(" New task: {}_", self.input),
                Mode::Tagging => format!(" Add tag: {}_", self.input),
                Mode::Scheduling => format!(" Schedule <start> [;rrule=...]: {}_", self.input),
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

/// Launch the interactive TUI.
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
        let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
        let mut out = std::fs::File::create("/tmp/gtp_tui_frames.txt").unwrap();
        let frame = |label: &str, term: &mut Terminal<TestBackend>, app: &mut App, out: &mut std::fs::File| -> String {
            term.draw(|f| app.render(f)).unwrap();
            let s = snap(term);
            writeln!(out, "===== {label} =====").unwrap();
            out.write_all(s.as_bytes()).unwrap();
            s
        };

        let s = frame("1-initial-inbox", &mut term, &mut app, &mut out);
        assert!(s.contains("Inbox"), "header should show Inbox");
        assert!(s.contains("Buy groceries"), "inbox should list seeded task");

        app.handle_key(key('j')).unwrap();
        frame("2-nav-down", &mut term, &mut app, &mut out);
        app.handle_key(kc(KeyCode::Down)).unwrap();
        frame("3-nav-down2", &mut term, &mut app, &mut out);
        app.handle_key(key('k')).unwrap();
        frame("4-nav-up", &mut term, &mut app, &mut out);

        // capture flow
        app.handle_key(key('a')).unwrap();
        let s = frame("5-capture-mode", &mut term, &mut app, &mut out);
        assert!(s.contains("New task:"), "capture mode should show prompt");
        for c in "Buy milk".chars() {
            app.handle_key(key(c)).unwrap();
        }
        frame("6-capture-typing", &mut term, &mut app, &mut out);
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let s = frame("7-after-capture", &mut term, &mut app, &mut out);
        assert!(s.contains("Buy milk"), "newly captured task should appear");
        assert!(s.contains("· Inbox"), "capture should auto-jump to Inbox view");

        // Enter -> next
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        frame("8-to-next", &mut term, &mut app, &mut out);

        for (d, lbl) in [('2', "9-next"), ('3', "10-waiting"), ('4', "11-scheduled"), ('5', "12-someday"), ('6', "13-reference"), ('7', "14-done"), ('1', "15-back-inbox")] {
            app.handle_key(key(d)).unwrap();
            let s = frame(lbl, &mut term, &mut app, &mut out);
            let expect = match d {
                '2' => "Next", '3' => "Waiting", '4' => "Scheduled", '5' => "Someday",
                '6' => "Reference", '7' => "Done", _ => "Inbox",
            };
            assert!(s.contains(expect), "view {lbl} should show header {expect}");
        }

        app.handle_key(key('p')).unwrap();
        let s = frame("16-projects", &mut term, &mut app, &mut out);
        assert!(s.contains("Website Redesign"), "projects view should show project");

        app.handle_key(key('r')).unwrap();
        let s = frame("17-review", &mut term, &mut app, &mut out);
        assert!(s.contains("Weekly Review"), "review view header");

        // capture from a non-inbox view should auto-jump back to Inbox
        app.handle_key(key('3')).unwrap();
        frame("17b-waiting", &mut term, &mut app, &mut out);
        app.handle_key(key('a')).unwrap();
        for c in "Captured from waiting".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let s = frame("17c-capture-jump", &mut term, &mut app, &mut out);
        assert!(s.contains("· Inbox"), "capture from waiting should jump to Inbox");
        assert!(s.contains("Captured from waiting"), "jumped view should list the new task");

        // back to inbox, tag the first task
        app.handle_key(key('1')).unwrap();
        app.handle_key(key('t')).unwrap();
        let s = frame("18-tag-mode", &mut term, &mut app, &mut out);
        assert!(s.contains("Add tag:"), "tag mode prompt");
        for c in "urgent".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let s = frame("19-after-tag", &mut term, &mut app, &mut out);
        assert!(s.contains("urgent"), "detail should show added tag");

        // schedule the first task
        app.handle_key(key('c')).unwrap();
        let s = frame("20-schedule-mode", &mut term, &mut app, &mut out);
        assert!(s.contains("Schedule"), "schedule prompt");
        for c in "+2h".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let s = frame("21-after-schedule", &mut term, &mut app, &mut out);
        assert!(s.contains("sched"), "detail should show scheduled time");

        // now it should leave inbox
        app.handle_key(key('1')).unwrap();
        let s = frame("22-inbox-after-schedule", &mut term, &mut app, &mut out);
        assert!(!s.contains("Write homepage copy"), "scheduled task should leave inbox");

        // archive from scheduled view
        app.handle_key(key('4')).unwrap();
        app.handle_key(key('A')).unwrap();
        frame("23-after-archive", &mut term, &mut app, &mut out);

        // help toggle
        app.handle_key(key('?')).unwrap();
        let s = frame("24-help", &mut term, &mut app, &mut out);
        assert!(s.contains("navigate"), "help text should show");
        app.handle_key(key('?')).unwrap();
        frame("25-help-off", &mut term, &mut app, &mut out);

        // quit
        app.handle_key(key('q')).unwrap();
        assert!(app.should_quit, "q should set quit flag");
    }
}
