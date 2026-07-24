import re

with open("src/tui/mod.rs", "r") as f:
    content = f.read()

# 1. Update handle_normal for g, G, D, Delete
handle_normal_target = """            KeyCode::Char('A') => {
                if let Some(row) = self.items.get(self.selected).cloned() {
                    tasks::archive(self.conn, &row.id)?;
                    self.status_message = format!("archived {}", &row.id[..8]);
                    self.refresh()?;
                    self.load_detail();
                }
            }"""
handle_normal_repl = """            KeyCode::Char('g') => self.move_sel(-10000),
            KeyCode::Char('G') => self.move_sel(10000),
            KeyCode::Char('A') | KeyCode::Char('D') | KeyCode::Delete => {
                if let Some(row) = self.items.get(self.selected).cloned() {
                    tasks::archive(self.conn, &row.id)?;
                    self.status_message = format!("archived {}", &row.id[..8]);
                    self.refresh()?;
                    self.load_detail();
                }
            }"""
content = content.replace(handle_normal_target, handle_normal_repl)


# 2. Update render() for Modal Popups
render_footer_target = """        // 底栏
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
    }"""
render_footer_repl = """        // 底栏
        let footer = SHORT_HELP.to_string();
        f.render_widget(
            Paragraph::new(footer).block(Block::default().borders(Borders::ALL)),
            chunks[2],
        );

        if self.mode != Mode::Normal {
            let title = match self.mode {
                Mode::Capturing => " New task ",
                Mode::Tagging => " Add tag ",
                Mode::Scheduling => " Schedule <start> [;rrule=...] ",
                Mode::PlanningProject => " Project? ",
                Mode::PlanningTime => " Time? ",
                Mode::Normal => "",
            };
            let text = format!(" {}_", self.input);
            let area = centered_rect(50, 3, size);
            f.render_widget(ratatui::widgets::Clear, area);
            let block = Block::default().title(title).borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow));
            f.render_widget(Paragraph::new(text).block(block), area);
        }

        if self.show_help {
            let area = centered_rect(60, 17, size);
            f.render_widget(ratatui::widgets::Clear, area);
            let rows = vec![
                ratatui::widgets::Row::new(vec!["j/k or ↑/↓", "navigate"]),
                ratatui::widgets::Row::new(vec!["Ctrl+H/Ctrl+L", "switch focus pane"]),
                ratatui::widgets::Row::new(vec!["1-7", "inbox to done"]),
                ratatui::widgets::Row::new(vec!["p", "projects tree"]),
                ratatui::widgets::Row::new(vec!["r", "weekly review"]),
                ratatui::widgets::Row::new(vec!["a", "capture"]),
                ratatui::widgets::Row::new(vec!["x", "mark done"]),
                ratatui::widgets::Row::new(vec!["w", "waiting"]),
                ratatui::widgets::Row::new(vec!["s", "someday"]),
                ratatui::widgets::Row::new(vec!["c", "schedule (<start> [;rrule=...])"]),
                ratatui::widgets::Row::new(vec!["t", "add tag"]),
                ratatui::widgets::Row::new(vec!["A/D/Delete", "archive"]),
                ratatui::widgets::Row::new(vec!["Enter", "next"]),
                ratatui::widgets::Row::new(vec!["q", "quit"]),
                ratatui::widgets::Row::new(vec!["?", "toggle help"]),
            ];
            let widths = [Constraint::Percentage(30), Constraint::Percentage(70)];
            let table = ratatui::widgets::Table::new(rows, widths)
                .block(Block::default().title(" Help ").borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)))
                .column_spacing(2);
            f.render_widget(table, area);
        }
    }"""
content = content.replace(render_footer_target, render_footer_repl)


# 3. Add centered_rect helper above render_guide
centered_rect_target = """    fn render_guide(&self, f: &mut ratatui::Frame, area: Rect) {"""
centered_rect_repl = """fn centered_rect(percent_x: u16, height: u16, r: Rect) -> Rect {
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

    fn render_guide(&self, f: &mut ratatui::Frame, area: Rect) {"""
content = content.replace(centered_rect_target, centered_rect_repl)

# 4. Group GTD statuses in render_guide
render_guide_target = """        // 2) 七状态地图：全部列出，当前状态高亮反转
        lines.push(Line::from(Span::styled(
            " 状态",
            Style::default().add_modifier(Modifier::UNDERLINED),
        )));
        let cur = self.view.status();
        for v in View::all_status_views() {
            let s = v.status().unwrap_or("");
            let cnt = self.context_count(*v);
            let active = cur == Some(s);
            let letter = status_letter(s);
            let label = status_cn(v.status_enum());
            if active {
                lines.push(Line::from(Span::styled(
                    format!(" ▶{} {:<7} {}", letter, label, cnt),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  {} ", letter),
                        Style::default().fg(status_color(s)),
                    ),
                    Span::raw(format!("{:<7} {}", label, cnt)),
                ]));
            }
        }
        lines.push(Line::from(""));"""
render_guide_repl = """        // 2) 七状态地图：分组列出
        lines.push(Line::from(Span::styled(
            " 状态",
            Style::default().add_modifier(Modifier::UNDERLINED),
        )));
        let cur = self.view.status();
        
        let mut add_views = |views: &[View], title: &str| {
            lines.push(Line::from(Span::styled(
                title,
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)
            )));
            for v in views {
                let s = v.status().unwrap_or("");
                let cnt = self.context_count(*v);
                let active = cur == Some(s);
                let letter = status_letter(s);
                let label = status_cn(v.status_enum());
                if active {
                    lines.push(Line::from(Span::styled(
                        format!(" ▶{} {:<7} {}", letter, label, cnt),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )));
                } else {
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("  {} ", letter),
                            Style::default().fg(status_color(s)),
                        ),
                        Span::raw(format!("{:<7} {}", label, cnt)),
                    ]));
                }
            }
        };

        add_views(&[View::Inbox, View::Next], "  [Active]");
        add_views(&[View::Waiting, View::Scheduled, View::Someday], "  [Waiting]");
        add_views(&[View::Reference, View::Done], "  [Archive]");
        lines.push(Line::from(""));"""
content = content.replace(render_guide_target, render_guide_repl)


# 5. Update render_list for Border colors & reversed highlight
render_list_target = """    fn render_list(&mut self, f: &mut ratatui::Frame, area: Rect) {
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
    }"""
render_list_repl = """    fn render_list(&mut self, f: &mut ratatui::Frame, area: Rect) {
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
            )
            .highlight_symbol("▶ ");
        f.render_stateful_widget(list, area, &mut self.list_state);
    }"""
content = content.replace(render_list_target, render_list_repl)

# 6. Rewrite render_detail using ratatui::widgets::Table
render_detail_target = """    fn render_detail(&self, f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
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
    }"""
render_detail_repl = """    fn render_detail(&self, f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        let border_color = if self.pane == Pane::Right { Color::Yellow } else { Color::DarkGray };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title("Detail");

        match &self.detail {
            None => {
                f.render_widget(Paragraph::new("No selection").block(block), area);
            }
            Some(d) => {
                let mut rows = vec![];
                rows.push(ratatui::widgets::Row::new(vec![
                    Line::from(Span::styled("Title", Style::default().add_modifier(Modifier::BOLD))),
                    Line::from(Span::styled(d.task.title.clone(), Style::default().add_modifier(Modifier::BOLD))),
                ]));
                rows.push(ratatui::widgets::Row::new(vec![
                    Line::from("Status"),
                    Line::from(d.task.status.to_string()),
                ]));
                if let Some(p) = &d.task.parent_id {
                    rows.push(ratatui::widgets::Row::new(vec![
                        Line::from("Project"),
                        Line::from(p[..8].to_string()),
                    ]));
                }
                rows.push(ratatui::widgets::Row::new(vec![
                    Line::from("Due"),
                    Line::from(time::format_local(d.task.due_at)),
                ]));
                rows.push(ratatui::widgets::Row::new(vec![
                    Line::from("Sched"),
                    Line::from(format!("{} -> {}", time::format_local(d.task.scheduled_start_at), time::format_local(d.task.scheduled_end_at))),
                ]));
                
                let mut tag_line = vec![];
                for (i, t) in d.tags.iter().enumerate() {
                    tag_line.push(Span::styled(t.name.clone(), Style::default().fg(Color::Cyan)));
                    if i < d.tags.len() - 1 {
                        tag_line.push(Span::raw(", "));
                    }
                }
                rows.push(ratatui::widgets::Row::new(vec![
                    Line::from("Tags"),
                    Line::from(tag_line),
                ]));

                rows.push(ratatui::widgets::Row::new(vec![Line::from(""), Line::from("")]));
                rows.push(ratatui::widgets::Row::new(vec![
                    Line::from(Span::styled("Timeline", Style::default().add_modifier(Modifier::UNDERLINED))),
                    Line::from(""),
                ]));
                
                for e in d.events.iter().rev().take(8).rev() {
                    rows.push(ratatui::widgets::Row::new(vec![
                        Line::from(time::format_local(Some(e.at))),
                        Line::from(format!("{:<14} {} -> {}", e.event_type, e.from_status.as_deref().unwrap_or("-"), e.to_status.as_deref().unwrap_or("-"))),
                    ]));
                }

                let widths = [Constraint::Length(12), Constraint::Min(0)];
                let table = ratatui::widgets::Table::new(rows, widths).block(block);
                f.render_widget(table, area);
            }
        }
    }"""
content = content.replace(render_detail_target, render_detail_repl)

# 7. Add mouse support
# In run(): enable mouse
run_target = """pub fn run(conn: &Connection) -> Result<()> {
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
}"""
run_repl = """pub fn run(conn: &Connection) -> Result<()> {
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
}"""
content = content.replace(run_target, run_repl)

# Event loop: handle mouse
event_loop_target = """        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Release {
                    continue;
                }
                app.handle_key(key)?;
            }
        }"""
event_loop_repl = """        if event::poll(Duration::from_millis(100))? {
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
        }"""
content = content.replace(event_loop_target, event_loop_repl)


with open("src/tui/mod.rs", "w") as f:
    f.write(content)

