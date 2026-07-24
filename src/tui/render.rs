use super::app::{App, Mode, Pane, View, pad_right};
use super::{status_cn, next_hint};
use ratatui::{layout::{Constraint, Direction, Layout, Rect}, style::{Color, Modifier, Style}, text::{Line, Span}, widgets::{Block, Borders, List, Paragraph}, Frame};
use crate::model::task;
use crate::time;
use crate::repo::tasks::{self, ListFilter};
use super::ui::build_list_items;

pub(crate) trait AppRender {
    fn render(&mut self, f: &mut Frame);
    fn render_help_drawer(&self, f: &mut Frame, area: Rect);
    fn centered_rect(&self, percent_x: u16, percent_y: u16, r: Rect) -> Rect;
    fn render_guide(&self, f: &mut Frame, area: Rect);
    fn render_list(&mut self, f: &mut Frame, area: Rect);
    fn render_detail(&mut self, f: &mut Frame, area: Rect);
    fn render_review(&mut self, f: &mut Frame, area: Rect);
}

impl<'a> AppRender for App<'a> {
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
                Mode::Search => " Search Tasks (Title / Notes) ",
                Mode::EditingTitle => " Edit title ",
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
            let area = self.centered_rect(50, 3, size);
            f.render_widget(ratatui::widgets::Clear, area);
            let block = Block::default().title(title).borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow));
            f.render_widget(Paragraph::new(text).block(block), area);
        }
        if self.mode == Mode::SchedulingCalendar {
            let area = self.centered_rect(60, 15, size);
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
            ("/", "搜索"),
            ("a", "收集任务"),
            ("e", "编辑标题"),
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

    fn centered_rect(&self, percent_x: u16, height: u16, r: Rect) -> Rect {
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

    fn render_detail(&mut self, f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
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

    fn render_review(&mut self, f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        let all = tasks::list(
            self.conn,
            &ListFilter {
                status: None,
                project: None,
                tags: vec![],
                query: None,
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
