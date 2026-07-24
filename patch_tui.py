import sys

def main():
    with open("src/tui/ui.rs", "r") as f:
        ui_code = f.read()

    # ui.rs patch
    ui_code = ui_code.replace("pub fn status_letter(s: &str) -> &'static str {", """use crate::model::task::Status;

pub fn status_letter(s: &Status) -> &'static str {""")
    
    ui_code = ui_code.replace("""match s {
        "inbox" => ".",
        "next" => ">",
        "waiting" => "W",
        "scheduled" => "#",
        "someday" => "?",
        "reference" => "*",
        "done" => "x",
        _ => " ",
    }""", """match s {
        Status::Inbox => ".",
        Status::Next => ">",
        Status::Waiting => "W",
        Status::Scheduled => "#",
        Status::Someday => "?",
        Status::Reference => "*",
        Status::Done => "x",
    }""")
    ui_code = ui_code.replace("""pub fn status_color(s: &str) -> Color {""", """pub fn status_color(s: &Status) -> Color {""")
    ui_code = ui_code.replace("""match s {
        "inbox" => Color::Gray,
        "next" => Color::Yellow,
        "waiting" => Color::Blue,
        "scheduled" => Color::Cyan,
        "someday" => Color::Magenta,
        "reference" => Color::White,
        "done" => Color::Green,
        _ => Color::Gray,
    }""", """match s {
        Status::Inbox => Color::Gray,
        Status::Next => Color::Yellow,
        Status::Waiting => Color::Blue,
        Status::Scheduled => Color::Cyan,
        Status::Someday => Color::Magenta,
        Status::Reference => Color::White,
        Status::Done => Color::Green,
    }""")

    with open("src/tui/ui.rs", "w") as f:
        f.write(ui_code)


    # mod.rs patch
    with open("src/tui/mod.rs", "r") as f:
        mod_code = f.read()

    # Imports
    mod_code = mod_code.replace("use crate::model::task::Task;", "use crate::model::task::{self, Task};")
    
    # View::status
    mod_code = mod_code.replace("""fn status(self) -> Option<&'static str> {
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
    }""", """fn status(self) -> Option<task::Status> {
        match self {
            View::Inbox => Some(task::Status::Inbox),
            View::Next => Some(task::Status::Next),
            View::Waiting => Some(task::Status::Waiting),
            View::Scheduled => Some(task::Status::Scheduled),
            View::Someday => Some(task::Status::Someday),
            View::Reference => Some(task::Status::Reference),
            View::Done => Some(task::Status::Done),
            View::Projects | View::Review => None,
        }
    }""")

    # Mode
    mod_code = mod_code.replace("""    /// 计划钩子第 2 步：询问预计时间。
    PlanningTime,
}""", """    /// 计划钩子第 2 步：询问预计时间。
    PlanningTime,
    SwitchBoard,
}""")

    # Row struct
    mod_code = mod_code.replace("status: String,", "status: task::Status,")
    mod_code = mod_code.replace("status: t.status.clone(),", "status: t.status,")

    # App struct
    mod_code = mod_code.replace("""show_help: bool,
    should_quit: bool,
}""", """show_help: bool,
    should_quit: bool,
    active_context: Option<String>,
    review_step: Option<usize>,
}""")

    # App::new
    mod_code = mod_code.replace("""show_help: false,
            should_quit: false,
        };""", """show_help: false,
            should_quit: false,
            active_context: None,
            review_step: None,
        };""")

    # total_count, context_count, refresh list tasks
    mod_code = mod_code.replace("""fn total_count(&self) -> usize {
        tasks::list(
            self.conn,
            &ListFilter {
                status: None,
                project: None,
                tags: vec![],
            },
        )""", """fn total_count(&self) -> usize {
        tasks::list(
            self.conn,
            &ListFilter {
                status: None,
                project: None,
                tags: self.active_context.clone().into_iter().collect(),
            },
        )""")

    mod_code = mod_code.replace("""fn context_count(&self, v: View) -> usize {
        if let Some(s) = v.status() {
            tasks::list(
                self.conn,
                &ListFilter {
                    status: Some(s.to_string()),
                    project: None,
                    tags: vec![],
                },
            )""", """fn context_count(&self, v: View) -> usize {
        if let Some(s) = v.status() {
            tasks::list(
                self.conn,
                &ListFilter {
                    status: Some(s),
                    project: None,
                    tags: self.active_context.clone().into_iter().collect(),
                },
            )""")

    mod_code = mod_code.replace("""fn refresh(&mut self) -> Result<()> {
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
                    )?;""", """fn refresh(&mut self) -> Result<()> {
        self.items.clear();
        match self.view {
            View::Projects => {
                let projects = tasks::list(
                    self.conn,
                    &ListFilter {
                        status: None,
                        project: None,
                        tags: self.active_context.clone().into_iter().collect(),
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
                            tags: self.active_context.clone().into_iter().collect(),
                        },
                    )?;""")

    mod_code = mod_code.replace("""            _ => {
                if let Some(s) = self.view.status() {
                    let ts = tasks::list(
                        self.conn,
                        &ListFilter {
                            status: Some(s.to_string()),
                            project: None,
                            tags: vec![],
                        },
                    )?;""", """            _ => {
                if let Some(s) = self.view.status() {
                    let ts = tasks::list(
                        self.conn,
                        &ListFilter {
                            status: Some(s),
                            project: None,
                            tags: self.active_context.clone().into_iter().collect(),
                        },
                    )?;""")

    # Actions: act_next, act_on_selected
    mod_code = mod_code.replace("""fn act_next(&mut self, row: Row) -> Result<()> {
        let t = tasks::transition(self.conn, &row.id, "next")?;""", """fn act_next(&mut self, row: Row) -> Result<()> {
        let t = tasks::transition(self.conn, &row.id, task::Status::Next)?;""")

    mod_code = mod_code.replace("""fn act_on_selected(&mut self, to: &str) -> Result<()> {
        if let Some(row) = self.items.get(self.selected).cloned() {
            if row.status == to {
                self.status_message = format!("already {}", to);
                return Ok(());
            }
            if to == "next" {""", """fn act_on_selected(&mut self, to: task::Status) -> Result<()> {
        if let Some(row) = self.items.get(self.selected).cloned() {
            if row.status == to {
                self.status_message = format!("already {}", to);
                return Ok(());
            }
            if to == task::Status::Next {""")

    # Normal mode handlers
    mod_code = mod_code.replace("""KeyCode::Char('x') => self.act_on_selected("done")?,
            KeyCode::Char('w') => self.act_on_selected("waiting")?,
            KeyCode::Char('s') => self.act_on_selected("someday")?,""", """KeyCode::Char('x') => self.act_on_selected(task::Status::Done)?,
            KeyCode::Char('w') => self.act_on_selected(task::Status::Waiting)?,
            KeyCode::Char('s') => self.act_on_selected(task::Status::Someday)?,""")
    mod_code = mod_code.replace("""KeyCode::Enter => self.act_on_selected("next")?,""", """KeyCode::Enter => self.act_on_selected(task::Status::Next)?,""")
    
    # NLP parser helper
    nlp = """fn parse_nlp_capture(input: &str) -> (String, Vec<String>, Option<String>, Option<i64>) {
    let mut title_parts = Vec::new();
    let mut tags = Vec::new();
    let mut project = None;
    let mut time_str = Vec::new();

    for token in input.split_whitespace() {
        if token.starts_with('@') && token.len() > 1 {
            tags.push(token[1..].to_string());
        } else if token.starts_with('+') && token.len() > 1 {
            project = Some(token[1..].to_string());
        } else if token.starts_with('~') && token.len() > 1 {
            time_str.push(token[1..].to_string());
        } else {
            title_parts.push(token);
        }
    }

    let title = title_parts.join(" ");
    let time = if !time_str.is_empty() {
        crate::time::parse_time(&time_str.join(" ")).ok()
    } else {
        None
    };

    (title, tags, project, time)
}
"""
    mod_code = mod_code.replace("impl<'a> App<'a> {", nlp + "\nimpl<'a> App<'a> {")

    # Review mode and SwitchBoard bindings
    mod_code = mod_code.replace("KeyCode::Char('r') => self.set_view(View::Review),", """KeyCode::Char('r') | KeyCode::Char('R') => {
                if self.review_step.is_none() {
                    self.review_step = Some(1);
                    self.set_view(View::Inbox);
                    self.status_message = "Review Step 1/3: Clear Inbox. Press R for next.".into();
                } else if self.review_step == Some(1) {
                    self.review_step = Some(2);
                    self.set_view(View::Waiting);
                    self.status_message = "Review Step 2/3: Check Waiting. Press R for next.".into();
                } else if self.review_step == Some(2) {
                    self.review_step = Some(3);
                    self.set_view(View::Someday);
                    self.status_message = "Review Step 3/3: Check Someday. Press R to finish.".into();
                } else {
                    self.review_step = None;
                    self.set_view(View::Review);
                    self.status_message = "Review finished.".into();
                }
            }""")
    mod_code = mod_code.replace("KeyCode::Enter => self.act_on_selected(task::Status::Next)?,\n            _ => {}", """KeyCode::Enter => self.act_on_selected(task::Status::Next)?,
            KeyCode::Char('b') | KeyCode::Char('B') => {
                if key.code == KeyCode::Char('B') || key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.mode = Mode::SwitchBoard;
                    self.input.clear();
                }
            }
            _ => {}""")

    # Capturing logic
    mod_code = mod_code.replace("""            Mode::Capturing => {
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
            }""", """            Mode::Capturing => {
                let trimmed = input.trim();
                if !trimmed.is_empty() {
                    let (title, tags, project, due_at) = parse_nlp_capture(trimmed);
                    if !title.is_empty() {
                        let mut parent_id = None;
                        if let Some(p) = project {
                            if let Ok(pid) = tasks::resolve_project(self.conn, &p) {
                                parent_id = Some(pid);
                            }
                        }
                        let t = tasks::create_capture(
                            self.conn,
                            &CaptureInput {
                                title,
                                kind: task::TaskKind::Action,
                                parent_id,
                                status: task::Status::Inbox,
                                due_at,
                                tag_names: tags,
                                ..Default::default()
                            },
                        )?;
                        self.set_view(View::Inbox);
                        self.status_message = format!("captured {}", &t.id[..8]);
                    }
                }
            }
            Mode::SwitchBoard => {
                let board = input.trim();
                if board.is_empty() {
                    self.active_context = None;
                    self.status_message = "Board cleared.".into();
                } else {
                    self.active_context = Some(board.to_string());
                    self.status_message = format!("Switched to board: {}", board);
                }
                self.refresh()?;
                self.load_detail();
            }""")

    # Header and Footer
    mod_code = mod_code.replace("""        let header = Line::from(vec![
            Span::styled(
                " gtp ",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("· {}   ", self.view.label())),
            Span::styled(&self.status_message, Style::default().fg(Color::Green)),
        ]);""", """        let board_span = if let Some(ctx) = &self.active_context {
            Span::styled(format!("[Board: {}] ", ctx), Style::default().fg(Color::Cyan))
        } else {
            Span::raw("")
        };
        let header = Line::from(vec![
            Span::styled(
                " gtp ",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("· {}   ", self.view.label())),
            board_span,
            Span::styled(&self.status_message, Style::default().fg(Color::Green)),
        ]);""")

    mod_code = mod_code.replace("""Mode::PlanningTime => format!(" Time? {}_", self.input),
                Mode::Normal => String::new(),""", """Mode::PlanningTime => format!(" Time? {}_", self.input),
                Mode::SwitchBoard => format!(" Board context: {}_", self.input),
                Mode::Normal => String::new(),""")

    # Guide UI - unwrap v.status() because v is never Projects/Review here
    mod_code = mod_code.replace("""let color = status_color(v.status().unwrap_or(""));
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {} {} ", status_letter(v.status().unwrap_or("")), v.label()),
                    Style::default().fg(color),
                ),""", """let color = status_color(&v.status().unwrap());
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {} {} ", status_letter(&v.status().unwrap()), v.label()),
                    Style::default().fg(color),
                ),""")

    # Detail view
    mod_code = mod_code.replace("""                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "timeline",
                    Style::default().add_modifier(Modifier::UNDERLINED),
                )));""", """                if let Some(d_to) = &d.task.delegated_to {
                    lines.push(Line::from(format!("delegated : {}", d_to)));
                }
                lines.push(Line::from(format!("proj_type : {}", d.task.project_type)));
                if !d.task.checklist.is_empty() {
                    lines.push(Line::from("checklist :"));
                    for item in &d.task.checklist {
                        let mark = if item.done { "[x]" } else { "[ ]" };
                        lines.push(Line::from(format!("  {} {}", mark, item.title)));
                    }
                }
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "timeline",
                    Style::default().add_modifier(Modifier::UNDERLINED),
                )));""")

    # Review list
    mod_code = mod_code.replace("""        let c = |s: &str| all.iter().filter(|t| t.status == s).count();
        let lines = vec![
            Line::from("Weekly Review"),
            Line::from(format!("  inbox     : {}", c("inbox"))),
            Line::from(format!("  next      : {}", c("next"))),
            Line::from(format!("  waiting   : {}", c("waiting"))),
            Line::from(format!("  scheduled : {}", c("scheduled"))),
            Line::from(format!("  someday   : {}", c("someday"))),
            Line::from(format!("  reference : {}", c("reference"))),
            Line::from(format!("  done      : {}", c("done"))),
        ];""", """        let c = |s: task::Status| all.iter().filter(|t| t.status == s).count();
        let lines = vec![
            Line::from("Weekly Review"),
            Line::from(format!("  inbox     : {}", c(task::Status::Inbox))),
            Line::from(format!("  next      : {}", c(task::Status::Next))),
            Line::from(format!("  waiting   : {}", c(task::Status::Waiting))),
            Line::from(format!("  scheduled : {}", c(task::Status::Scheduled))),
            Line::from(format!("  someday   : {}", c(task::Status::Someday))),
            Line::from(format!("  reference : {}", c(task::Status::Reference))),
            Line::from(format!("  done      : {}", c(task::Status::Done))),
        ];""")

    # Tests fixes
    mod_code = mod_code.replace("""kind: "project".into(),
                parent_id: None,
                status: "next".into(),
                due_at: None,
                tag_names: vec![],
            },""", """kind: task::TaskKind::Project,
                parent_id: None,
                status: task::Status::Next,
                due_at: None,
                tag_names: vec![],
                ..Default::default()
            },""")

    mod_code = mod_code.replace("""kind: kind.into(),
                    parent_id: parent.map(|s| s.to_string()),
                    status: status.into(),
                    due_at: None,
                    tag_names: tags.iter().map(|s| s.to_string()).collect(),
                },""", """kind: kind.parse().unwrap(),
                    parent_id: parent.map(|s| s.to_string()),
                    status: status.parse().unwrap(),
                    due_at: None,
                    tag_names: tags.iter().map(|s| s.to_string()).collect(),
                    ..Default::default()
                },""")

    mod_code = mod_code.replace("""status: Some("next".into()),""", """status: Some(task::Status::Next),""")

    with open("src/tui/mod.rs", "w") as f:
        f.write(mod_code)

if __name__ == '__main__':
    main()
