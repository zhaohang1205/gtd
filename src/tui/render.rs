use super::app::{App, Mode, Pane, View, pad_right};
use super::{status_cn, next_hint};
use ratatui::{layout::{Constraint, Direction, Layout, Rect}, style::{Color, Modifier, Style}, text::{Line, Span}, widgets::{Block, Borders, List, Paragraph}, Frame};
use crate::model::{event, task};
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
        let size = f.area();
        let mut main_area = size;
        if self.is_reviewing {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(0)])
                .split(size);
            
            let step_names = ["", "清空收件箱", "检视项目", "追踪等待事项", "重估将来/也许"];
            let step_name = step_names.get(self.review_step as usize).unwrap_or(&"");
            
            let banner = Paragraph::new(Line::from(Span::styled(
                format!(" 🌟 每周回顾 第 {}/4 步: {} (按 'R' 进入下一步, 'Esc' 退出) ", self.review_step, step_name),
                Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)
            )));
            f.render_widget(banner, chunks[0]);
            main_area = chunks[1];
        }

        self.list_state.select(Some(self.selected));
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(main_area);

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

        if self.mode != Mode::Normal && self.mode != Mode::SchedulingCalendar && self.mode != Mode::ConfirmArchive {
            let title = match self.mode {
                Mode::Search => " Search Tasks (Title / Notes) ",
                Mode::EditingTitle => " Edit title ",
                Mode::Capturing => " 快速录入 (支持自然语言) ",
                Mode::Tagging => " Add tag (Hints: home, work, errands...) ",
                Mode::SchedulingTimeRRule => " 设定时间与循环规则 (格式: 15:00-16:00 ;FREQ=WEEKLY;BYDAY=SA,SU) ",
                Mode::SchedulingCalendar => "", // Not reached
                Mode::WaitingWho => " Waiting for who/what? ",
                Mode::WaitingWhen => " Reminder time? (e.g. +1d, tomorrow 10:00) ",
                Mode::PlanningProject => " Project? ",
                Mode::PlanningTime => " Time? ",
                Mode::ChecklistAdding => " 新增检查单 ",
                Mode::FilteringTag => " 过滤标签 (Context) ",
                Mode::EditingDue => " 截止时间? (空=清除, 如 +3d, tomorrow 10:00) ",
                Mode::EditingRrule => " 循环规则? (空=清除, 如 FREQ=WEEKLY;BYDAY=SA,SU) ",
                Mode::EditingDelegated => " 委派给? (空=清除) ",
                Mode::EditingProjectType => " 项目类型? (parallel/sequential) ",
                Mode::Normal | Mode::Visual | Mode::ConfirmArchive => "",
            };
            
            let mut text_lines = vec![Line::from(format!(" {}_", self.input))];
            let mut height = 3;
            let width = if self.mode == Mode::Capturing { 70 } else { 50 };

            if self.mode == Mode::Capturing {
                text_lines.push(Line::from(""));
                text_lines.push(Line::from(Span::styled(" [语法] @标签 (如 @work)  |  ~时间 (如 ~tomorrow, ~+3d, ~18:00)", Style::default().fg(Color::DarkGray))));
                height = 5;
            }

            let area = self.centered_rect(width, height, size);
            f.render_widget(ratatui::widgets::Clear, area);
            let block = Block::default().title(title).borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow));
            f.render_widget(Paragraph::new(text_lines).block(block), area);
        }
        if self.mode == Mode::SchedulingCalendar {
            let area = self.centered_rect(60, 15, size);
            self.calendar.render(f, area);
        }

        // 帮助弹窗最后绘制，作为置顶模态层（ratatui 无图层概念，后画者覆盖先画者）
        if self.show_help {
            self.render_help_drawer(f, self.centered_rect(76, 52, size));
        }
    }

    fn render_help_drawer(&self, f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        // 先擦除底层内容（ratatui 无透明/图层，必须显式 Clear 否则会透出后方文字）
        f.render_widget(ratatui::widgets::Clear, area);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(23), Constraint::Min(0)])
            .split(area);

        // 上：快捷键
        let keys_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" 快捷键 (F1/?) ");
        let keys = [
            ("h/l", "切换面板"),
            ("j/k", "上下移动"),
            ("1-8", "切换主视图 (8=归档箱)"),
            ("/", "全局搜索 (标题/备注)"),
            ("f", "情境过滤 (Context)"),
            ("a", "捕获到收件箱"),
            ("n", "编辑备注 ($EDITOR)"),
            ("e", "编辑标题"),
            ("d", "修改截止时间 (due)"),
            ("L", "修改循环规则 (rrule)"),
            ("b", "修改所属项目"),
            ("W", "修改委派对象"),
            ("T", "修改项目类型 (项目)"),
            ("C", "新增子任务"),
            ("SPC", "完成子任务"),
            ("p", "切换到项目视图"),
            ("r", "开启每周回顾"),
            ("Enter", "理清 / 标记下一步"),
            ("c", "为任务排期 (日历)"),
            ("w", "标记为等待中"),
            ("s", "标记为将来/也许"),
            ("x", "标记为已完成"),
            ("A/D", "归档任务 (y 确认 / n 取消)"),
            ("u", "归档箱中恢复任务"),
            ("P/S", "开始 / 停止番茄钟"),
            ("q", "退出"),
        ];
        let mut rows = vec![];
        for (k, desc) in keys.iter() {
            rows.push(ratatui::widgets::Row::new(vec![
                ratatui::text::Line::from(Span::styled(format!("{:>5} ", k), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
                ratatui::text::Line::from(Span::raw(*desc)),
            ]));
        }
        let widths = [Constraint::Length(6), Constraint::Min(0)];
        let table = ratatui::widgets::Table::new(rows, widths)
            .block(keys_block)
            .column_spacing(1);
        f.render_widget(table, chunks[0]);

        // 下：语法说明
        let syntax_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" 语法说明 ");
        let syntax = vec![
            Line::from(Span::styled("快速录入 (a)", Style::default().add_modifier(Modifier::BOLD))),
            Line::from("  @标签    添加情境/优先级, 如 @work @p1 (可多个)"),
            Line::from("  ~时间    设置截止, 见下方时间语法"),
            Line::from("  例: a买牛奶 @home ~tomorrow   /   a写周报 @work @p1 ~+3d"),
            Line::from(""),
            Line::from(Span::styled("时间语法 (~ 与排期 c)", Style::default().add_modifier(Modifier::BOLD))),
            Line::from("  now / +2h +30m +1d +1w   相对时间"),
            Line::from("  today / tomorrow [HH:MM]   今天/明天 (默认 00:00)"),
            Line::from("  HH:MM                    今天该时刻, 如 18:00"),
            Line::from("  2026-07-24 / 2026-07-24 14:30   绝对日期/时间"),
            Line::from(""),
            Line::from(Span::styled("周期 / 循环任务 (习惯)", Style::default().add_modifier(Modifier::BOLD))),
            Line::from("  先 c 排期选日期, 再在 '时间;规则' 中输入 RRULE 即成为循环任务"),
            Line::from("  循环任务标记为完成时不会消失, 而是自动顺延到下一周期"),
            Line::from("  (状态回到 Scheduled, 时间推进到下一个发生点, 记 habit_completed)"),
            Line::from("  FREQ=DAILY|WEEKLY|MONTHLY      频率"),
            Line::from("  INTERVAL=2                      间隔, 如每 2 周"),
            Line::from("  BYDAY=SA,SU                     周几 (MO TU WE TH FR SA SU)"),
            Line::from("  COUNT=10 / UNTIL=2026-12-31     终止条件(做到第N次/到某日停)"),
            Line::from("  例: ;FREQ=WEEKLY;BYDAY=SA,SU    ;FREQ=DAILY;COUNT=30"),
            Line::from("  在列表/排序中, 循环任务按'下一个发生时间'参与排期"),
            Line::from(""),
            Line::from(Span::styled("其他", Style::default().add_modifier(Modifier::BOLD))),
            Line::from("  等待 w 后可填 [谁/何时], 如 w → Alice → +1d"),
            Line::from("  子任务 C 新增, SPC 依次打卡, 全部完成自动重置"),
            Line::from("  归档箱(8)内 u 恢复; 归档需 y 确认"),
            Line::from("  按 F1/? 关闭本帮助"),
        ];
        let para = Paragraph::new(syntax)
            .block(syntax_block)
            .wrap(ratatui::widgets::Wrap { trim: false });
        f.render_widget(para, chunks[1]);
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
                    View::Archived => ("", "归档箱"),
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
        for (key, v) in &[('p', View::Projects), ('r', View::Review), ('8', View::Archived)] {
            let active = cur == *v;
            let (icon, label) = match v {
                View::Projects => ("", "项目树"),
                View::Review => ("", "周回顾"),
                View::Archived => ("", "归档箱"),
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
                    .title(format!("Tasks · {}{}", self.view.label(), if let Some(ref tf) = self.tag_filter { format!(" [@{}]", tf) } else { "".to_string() })),
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
                    Line::from(status_cn(d.task.status)),
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

                let pomo_count = d.events.iter().filter(|e| e.event_type == event::EV_POMODORO).count();
                if pomo_count > 0 {
                    let tomatoes = " ".repeat(pomo_count);
                    rows.push(ratatui::widgets::Row::new(vec![
                        Line::from(" 番茄钟"),
                        Line::from(format!("{} ({})", tomatoes, pomo_count)),
                    ]));
                }

                let table_h = (rows.len() as u16) + 2; // 行数 + 上下边框
                let detail_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(table_h), Constraint::Min(0)])
                    .split(area);
                let top = detail_chunks[0];
                let bottom = detail_chunks[1];

                let widths = [Constraint::Length(12), Constraint::Min(0)];
                let table = ratatui::widgets::Table::new(rows, widths).block(block);
                f.render_widget(table, top);

                // 备注 + 时间线：多行文本，单独用可换行的 Paragraph 渲染（Table 行不换行会裁切）
                let mut detail_lines: Vec<Line> = vec![];
                if d.task.notes.trim().is_empty() {
                    detail_lines.push(Line::from(Span::styled("备注: -", Style::default().fg(Color::DarkGray))));
                } else {
                    detail_lines.push(Line::from(Span::styled("备注:", Style::default().add_modifier(Modifier::BOLD))));
                    for ln in d.task.notes.split('\n') {
                        detail_lines.push(Line::from(format!("  {}", ln)));
                    }
                }
                detail_lines.push(Line::from(""));
                detail_lines.push(Line::from(Span::styled("时间线", Style::default().add_modifier(Modifier::UNDERLINED))));
                for e in d.events.iter().rev().take(8).rev() {
                    let event_cn = match e.event_type.as_str() {
                        "created" => "创建任务",
                        "status_change" => "状态流转",
                        event::EV_POMODORO => "完成专注",
                        event::EV_HABIT_COMPLETED => "习惯完成",
                        event::EV_RESTORED => "已恢复",
                        _ => &e.event_type,
                    };

                    let from_cn = e.from_status.as_deref().unwrap_or("-").parse::<crate::model::task::Status>().map(status_cn).unwrap_or("-");
                    let to_cn = e.to_status.as_deref().unwrap_or("-").parse::<crate::model::task::Status>().map(status_cn).unwrap_or("-");

                    let action = if e.event_type == "status_change" {
                        format!("{} -> {}", from_cn, to_cn)
                    } else if e.event_type == event::EV_POMODORO {
                        "🍅 +1".to_string()
                    } else {
                        "".to_string()
                    };

                    detail_lines.push(Line::from(format!("  {}  {} {}", time::format_local(Some(e.at)), event_cn, action)));
                }

                let detail_block = Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(border_color))
                    .title(" 备注 / 时间线 ");
                let detail_para = Paragraph::new(detail_lines)
                    .block(detail_block)
                    .wrap(ratatui::widgets::Wrap { trim: false });
                f.render_widget(detail_para, bottom);
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
