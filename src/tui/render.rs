use super::app::{App, Mode, Pane, View, pad_right};
use super::{status_cn, next_hint};
use ratatui::{layout::{Constraint, Direction, Layout, Rect}, style::{Color, Modifier, Style}, text::{Line, Span}, widgets::{Block, Borders, List, Paragraph}, Frame};
use ratatui::symbols::border;
use crate::model::{event, task};
use crate::time;
use crate::repo::tasks::{self, ListFilter};
use super::ui::build_list_items;
use super::ui;

pub(crate) trait AppRender {
    fn render(&mut self, f: &mut Frame);
    fn render_help_drawer(&self, f: &mut Frame, area: Rect);
    fn render_syntax_drawer(&self, f: &mut Frame, area: Rect);
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
                Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD)
            )));
            f.render_widget(banner, chunks[0]);
            main_area = chunks[1];
        } else {
            let pomo = crate::repo::pomodoro::get_state().unwrap_or_default();
            let show_banner = pomo.phase != crate::model::pomodoro::Phase::Idle || pomo.today_count > 0;
            if show_banner {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(1), Constraint::Min(0)])
                    .split(size);

                let now = crate::time::now_ms();
                let end_ts = pomo.end_ts.unwrap_or(now);
                let mut diff = (end_ts - now) / 1000;
                if diff < 0 { diff = 0; }
                let m = diff / 60;
                let s = diff % 60;

                if pomo.phase == crate::model::pomodoro::Phase::Work {
                    let title = pomo.task_title.as_deref().unwrap_or("无标题");
                    let banner = Paragraph::new(Line::from(vec![
                        Span::styled(" 🎯 当前专注: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                        Span::styled(format!(" {} ", title), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                        Span::styled(format!("  |  ⏱️ 倒计时 {:02}:{:02}  |  (按 'S' 终止专注) ", m, s), Style::default().fg(Color::White)),
                    ]))
                    .alignment(ratatui::layout::Alignment::Center)
                    .style(Style::default().bg(Color::Red));
                    f.render_widget(banner, chunks[0]);
                } else if matches!(pomo.phase, crate::model::pomodoro::Phase::ShortBreak | crate::model::pomodoro::Phase::LongBreak) {
                    let break_name = if pomo.phase == crate::model::pomodoro::Phase::LongBreak { "☕ 长休中" } else { "☕ 小休中" };
                    let banner = Paragraph::new(Line::from(vec![
                        Span::styled(format!(" 🏆 成就: 今日已积 {} 个番茄 (Streak {} 连击!)  |  ", pomo.today_count, pomo.streak), Style::default().fg(Color::Black).add_modifier(Modifier::BOLD)),
                        Span::styled(format!("{} {:02}:{:02}  |  ", break_name, m, s), Style::default().fg(Color::Black)),
                        Span::styled("再接再厉? 👉 [Space/P] 开启新一轮  |  [S] 退出休息 ", Style::default().fg(Color::Black).add_modifier(Modifier::BOLD)),
                    ]))
                    .alignment(ratatui::layout::Alignment::Center)
                    .style(Style::default().bg(Color::Green));
                    f.render_widget(banner, chunks[0]);
                } else {
                    // 休息自然结束 (Phase::Idle)，保留“成就结清 & 一键续杯”的常驻入口 Banner
                    let last_title = pomo.last_completed_task_title.as_deref().unwrap_or("上一任务");
                    let banner = Paragraph::new(Line::from(vec![
                        Span::styled(format!(" 🏆 成就结清: 今日已积 {} 个番茄 (Streak {} 连击!)  |  ", pomo.today_count, pomo.streak), Style::default().fg(Color::Black).add_modifier(Modifier::BOLD)),
                        Span::styled(format!("休息已完成  |  再接再厉? 👉 [Space/P] 开启新一轮专注 [{}] ", last_title), Style::default().fg(Color::Black).add_modifier(Modifier::BOLD)),
                    ]))
                    .alignment(ratatui::layout::Alignment::Center)
                    .style(Style::default().bg(Color::Green));
                    f.render_widget(banner, chunks[0]);
                }
                main_area = chunks[1];
            }
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
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
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
            _ => {
                if self.is_reviewing {
                    self.render_review(f, body[1]);
                } else {
                    self.render_list(f, body[1]);
                }
            }
        }
        self.render_detail(f, body[2]);

        // 状态栏 (Statusline)
        let mode_str = if self.mode == Mode::Normal { " NORMAL " } else { " INSERT " };
        let mode_bg = if self.mode == Mode::Normal { Color::Cyan } else { Color::Yellow };
        let mode_fg = Color::Black;

        let status_left = Line::from(vec![
            Span::styled(mode_str, Style::default().fg(mode_fg).bg(mode_bg).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {} ", self.view.label()), Style::default().fg(Color::White).bg(Color::Indexed(238)).add_modifier(Modifier::BOLD)),
        ]);

        let status_msg = if self.status_message.is_empty() {
            String::new()
        } else {
            format!(" {} ", self.status_message)
        };
        let status_right = Line::from(vec![
            Span::styled(status_msg, Style::default().fg(Color::White).bg(Color::Indexed(238))),
            Span::styled(" gtp ".to_string(), Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)),
        ]);

        let status_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(chunks[1]);
            
        f.render_widget(Paragraph::new(status_left).style(Style::default().bg(Color::Indexed(238))), status_layout[0]);
        f.render_widget(Paragraph::new(status_right).alignment(ratatui::layout::Alignment::Right).style(Style::default().bg(Color::Indexed(238))), status_layout[1]);

        if self.mode != Mode::Normal && self.mode != Mode::SchedulingCalendar && self.mode != Mode::ConfirmArchive {
            let title = match self.mode {
                Mode::Search => " Search Tasks (Title / Notes) ",
                Mode::EditingTitle => " Edit title ",
                Mode::Capturing => " 快速录入 (支持 @标签 及 Tab 补全: home, work, errands, quick, focus...) ",
                Mode::Tagging => " 添加标签 [支持 Tab 补全] (预设: home, work, errands, quick, focus...) ",
                Mode::SchedulingTimeRRule => " 设定时间与循环规则 (格式: 15:00-16:00 ;FREQ=WEEKLY;BYDAY=SA,SU) ",
                Mode::SchedulingCalendar => "", // Not reached
                Mode::WaitingWho => " Waiting for who/what? ",
                Mode::WaitingWhen => " Reminder time? (e.g. +1d, tomorrow 10:00) ",
                Mode::PlanningProject => " Project? ",
                Mode::PlanningTime => " Time? ",
                Mode::ChecklistAdding => " 新增检查单 ",
                Mode::FilteringTag => " 过滤标签 (Context) ",
                Mode::CreatingTag => " 新增自定义标签 (输入标签名称，按 Enter 保存) ",
                Mode::EditingDue => " 截止时间? (空=清除, 如 +3d, tomorrow 10:00) ",
                Mode::EditingRrule => " 循环规则? (空=清除, 如 FREQ=WEEKLY;BYDAY=SA,SU) ",
                Mode::EditingDelegated => " 委派给? (空=清除) ",
                Mode::EditingProjectType => " 项目类型? (parallel/sequential) ",
                Mode::ConfiguringPomo => " 自定义番茄钟时长 (格式: 工作分钟;短休分钟;长休分钟, 如 25;5;15) ",
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

            if self.show_syntax {
                // 当处于编辑/录入模式且按了 Ctrl+P 双开时，编辑输入框居左靠上绘制
                let left_area = Rect {
                    x: size.width / 20,
                    y: size.height / 10,
                    width: (size.width * 42 / 100).min(65),
                    height,
                };
                f.render_widget(ratatui::widgets::Clear, left_area);
                let block = Block::default().title(title).borders(Borders::ALL).border_set(border::ROUNDED).border_style(Style::default().fg(Color::Yellow));
                f.render_widget(Paragraph::new(text_lines).block(block), left_area);
            } else {
                let area = self.centered_rect(width, height, size);
                f.render_widget(ratatui::widgets::Clear, area);
                let block = Block::default().title(title).borders(Borders::ALL).border_set(border::ROUNDED).border_style(Style::default().fg(Color::Yellow));
                f.render_widget(Paragraph::new(text_lines).block(block), area);
            }
        }
        if self.mode == Mode::SchedulingCalendar {
            let area = self.centered_rect(60, 15, size);
            self.calendar.render(f, area);
        }

        if self.show_syntax {
            // 当 show_syntax 为 true 时，如果处于输入/编辑模式，则将语法面板放右半屏实现“左右双开”；否则居中
            let syntax_area = if self.mode.is_input() || self.mode == Mode::SchedulingCalendar {
                Rect {
                    x: size.width * 50 / 100,
                    y: size.height / 10,
                    width: size.width * 46 / 100,
                    height: (size.height * 80 / 100).min(30),
                }
            } else {
                self.centered_rect(76, 30, size)
            };
            self.render_syntax_drawer(f, syntax_area);
        }
    }

    fn render_help_drawer(&self, f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        f.render_widget(ratatui::widgets::Clear, area);
        let keys_block = Block::default()
            .borders(Borders::ALL).border_set(border::ROUNDED)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" 快捷键指南 (F1/?) ");
        let keys = [
            ("h/l", "切换面板 (左/中/右栏)"),
            ("j/k", "上下移动列表选项"),
            ("1-9", "切换视图 (9=标签库, 8=归档箱)"),
            ("/", "全局搜索 (标题与备注)"),
            ("f", "情境/标签过滤 (Context)"),
            ("a", "快速捕获任务 (Inbox)"),
            ("n", "编辑长备注 ($EDITOR)"),
            ("e", "编辑任务标题"),
            ("d", "修改截止时间 (due)"),
            ("L", "修改循环规则 (rrule)"),
            ("b", "修改归属项目 (Project)"),
            ("W", "修改委派对象 (Delegated)"),
            ("T", "修改项目类型 (Parallel/Seq)"),
            ("C", "新增检查单项 / SPC 勾选"),
            ("p", "切换到项目树视图"),
            ("r", "开启每周回顾 Hook"),
            ("Enter", "理清任务 / 标记下一步"),
            ("c", "日历排期 (Schedule)"),
            ("w", "标记为等待中 (Waiting)"),
            ("s", "标记为将来/也许 (Someday)"),
            ("x", "标记为已完成 (Done)"),
            ("A/D", "归档任务 (y确认/n取消)"),
            ("u", "恢复归档箱中的任务"),
            ("P/S", "开启专注(番茄钟) / 停止专注"),
            ("Shift+C", "自定义番茄钟时长 (工作/休息)"),
            ("Ctrl+p", "弹出语法说明指南"),
            ("F1/?", "关闭/展开快捷键面板"),
            ("q", "退出 TUI 系统"),
        ];
        let mut rows = vec![];
        let visible_keys = if self.help_scroll < keys.len() {
            &keys[self.help_scroll..]
        } else {
            &keys[..]
        };
        for (k, desc) in visible_keys.iter() {
            rows.push(ratatui::widgets::Row::new(vec![
                ratatui::text::Line::from(Span::styled(format!("{:>6} ", k), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
                ratatui::text::Line::from(Span::raw(*desc)),
            ]));
        }
        let widths = [Constraint::Length(8), Constraint::Min(0)];
        let table = ratatui::widgets::Table::new(rows, widths)
            .block(keys_block)
            .column_spacing(1);
        f.render_widget(table, area);
    }

    fn render_syntax_drawer(&self, f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        f.render_widget(ratatui::widgets::Clear, area);
        let syntax_block = Block::default()
            .borders(Borders::ALL).border_set(border::ROUNDED)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" 语法说明指南 (Ctrl+P) ");
        let syntax = vec![
            Line::from(Span::styled("快速录入语法 (按 a 捕获)", Style::default().add_modifier(Modifier::BOLD))),
            Line::from("  @标签    添加情境或优先级, 如 @work @p1 @focus (支持 Tab 智能补全)"),
            Line::from("  ~时间    设置截止时间, 见下方时间语法"),
            Line::from("  例: a买牛奶 @home ~tomorrow   /   a写周报 @work @p1 ~+3d"),
            Line::from(""),
            Line::from(Span::styled("时间语法 (~ 与排期 c)", Style::default().add_modifier(Modifier::BOLD))),
            Line::from("  now / +2h +30m +1d +1w   相对时间偏移"),
            Line::from("  today / tomorrow [HH:MM]   今天/明天指定时刻 (默认 00:00)"),
            Line::from("  HH:MM                    当天指定时刻, 如 18:00"),
            Line::from("  2026-07-24 / 2026-07-24 14:30   绝对日期与时间"),
            Line::from(""),
            Line::from(Span::styled("周期 / 循环任务 (Habit / RRULE)", Style::default().add_modifier(Modifier::BOLD))),
            Line::from("  先按 c 选排期日期, 再在 '时间;规则' 中输入 RRULE 即成为循环任务"),
            Line::from("  循环任务标记为完成时不会消失, 而是自动顺延到下一周期"),
            Line::from("  (状态自动回到 Scheduled, 时间推进到下一个发生点, 记 habit_completed)"),
            Line::from("  FREQ=DAILY|WEEKLY|MONTHLY      循环频率"),
            Line::from("  INTERVAL=2                      循环间隔 (如每 2 周)"),
            Line::from("  BYDAY=SA,SU                     指定周几 (MO TU WE TH FR SA SU)"),
            Line::from("  COUNT=10 / UNTIL=2026-12-31     终止条件 (做到第N次或特定日期截至)"),
            Line::from("  例: ;FREQ=WEEKLY;BYDAY=SA,SU    ;FREQ=DAILY;COUNT=30"),
            Line::from(""),
            Line::from(Span::styled("其他操作说明", Style::default().add_modifier(Modifier::BOLD))),
            Line::from("  等待 w 后可填写 [谁/何时], 如 w → Alice → +1d"),
            Line::from("  子任务 C 新增, SPC 依次打卡, 全部完成自动重置"),
            Line::from("  标签库 (视图9): 按 a 动态添加自定义标签, 按 D 删除标签"),
            Line::from("  按 Ctrl+P 弹出/关闭本语法说明指南"),
        ];
        let para = Paragraph::new(syntax)
            .block(syntax_block)
            .wrap(ratatui::widgets::Wrap { trim: false });
        f.render_widget(para, area);
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
                    View::Tags => ("🏷", "标签库"),
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
        for (key, v) in &[('p', View::Projects), ('r', View::Review), ('9', View::Tags), ('8', View::Archived)] {
            let active = cur == *v;
            let (icon, label) = match v {
                View::Projects => ("", "项目树"),
                View::Review => ("", "周回顾"),
                View::Archived => ("", "归档箱"),
                View::Tags => ("🏷", "标签库"),
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
            Paragraph::new(lines).block(Block::default().borders(Borders::ALL).border_set(border::ROUNDED).border_style(Style::default().fg(border_color)).title(" Guide ")),
            area,
        );
    }

    fn render_list(&mut self, f: &mut ratatui::Frame, area: Rect) {
        let border_color = if self.pane == Pane::Center { Color::Yellow } else { Color::DarkGray };
        let items = build_list_items(self);
        let title = format!(
            " Tasks · {}{} ",
            self.view.label(),
            if let Some(ref tf) = self.tag_filter { format!(" [@{}]", tf) } else { String::new() }
        );
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL).border_set(border::ROUNDED)
                    .border_style(Style::default().fg(border_color))
                    .title(title),
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
        f.render_widget(ratatui::widgets::Clear, area);
        let border_color = if self.pane == Pane::Right { Color::Yellow } else { Color::DarkGray };
        let block = Block::default()
            .borders(Borders::ALL).border_set(border::ROUNDED)
            .border_style(Style::default().fg(border_color))
            .title(" 任务详情 ");

        match &self.detail {
            None => {
                f.render_widget(Paragraph::new(" 未选中任务").block(block), area);
            }
            Some(d) => {
                let mut lines: Vec<Line> = vec![];
                
                // 标题
                lines.push(Line::from(vec![
                    Span::styled("标题: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::styled(d.task.title.clone(), Style::default().add_modifier(Modifier::BOLD)),
                ]));

                // 状态
                let st_color = ui::status_color(&d.task.status);
                lines.push(Line::from(vec![
                    Span::styled("状态: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(status_cn(d.task.status), Style::default().fg(st_color).add_modifier(Modifier::BOLD)),
                ]));

                // 项目
                if let Some(p) = &d.task.parent_id {
                    lines.push(Line::from(vec![
                        Span::styled("项目: ", Style::default().fg(Color::DarkGray)),
                        Span::raw(p[..8].to_string()),
                    ]));
                }

                // 截止时间
                lines.push(Line::from(vec![
                    Span::styled("截止: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(time::format_local(d.task.due_at)),
                ]));

                // 计划时间
                if d.task.scheduled_start_at.is_some() || d.task.scheduled_end_at.is_some() {
                    lines.push(Line::from(vec![
                        Span::styled("计划: ", Style::default().fg(Color::DarkGray)),
                        Span::raw(format!("{} -> {}", time::format_local(d.task.scheduled_start_at), time::format_local(d.task.scheduled_end_at))),
                    ]));
                }

                // 循环规则
                if let Some(rr) = &d.task.rrule {
                    let cn_rr = rr.replace("FREQ=DAILY", "每天")
                        .replace("FREQ=WEEKLY", "每周")
                        .replace("FREQ=MONTHLY", "每月")
                        .replace("INTERVAL=", "间隔=")
                        .replace("COUNT=", "次数=")
                        .replace("UNTIL=", "直到=");
                    lines.push(Line::from(vec![
                        Span::styled("循环: ", Style::default().fg(Color::DarkGray)),
                        Span::raw(cn_rr),
                    ]));
                }

                // 标签
                if !d.tags.is_empty() {
                    let mut tag_spans = vec![Span::styled("标签: ", Style::default().fg(Color::DarkGray))];
                    for (i, tg) in d.tags.iter().enumerate() {
                        let c = ui::priority_color(&tg.name).unwrap_or(Color::Cyan);
                        tag_spans.push(Span::styled(format!("@{}", tg.name), Style::default().fg(c)));
                        if i < d.tags.len() - 1 {
                            tag_spans.push(Span::raw(" "));
                        }
                    }
                    lines.push(Line::from(tag_spans));
                }

                // 委派
                if let Some(del) = &d.task.delegated_to {
                    lines.push(Line::from(vec![
                        Span::styled("委派: ", Style::default().fg(Color::DarkGray)),
                        Span::raw(del.clone()),
                    ]));
                }

                // 检查单 / 进度
                if d.task.kind == task::TaskKind::Project {
                    let children = tasks::list(
                        self.conn,
                        &ListFilter {
                            status: None,
                            project: Some(d.task.id.clone()),
                            tags: vec![],
                            query: None,
                        },
                    )
                    .unwrap_or_default();
                    let total = children.len();
                    let done = children.iter().filter(|c| c.status == task::Status::Done).count();
                    let bar = if total > 0 {
                        let filled = (done as f64 / total as f64 * 10.0).round() as usize;
                        format!("[{}{}] {}/{}", "█".repeat(filled), "░".repeat(10 - filled), done, total)
                    } else {
                        "[----------] 0/0".to_string()
                    };
                    lines.push(Line::from(vec![
                        Span::styled("进度: ", Style::default().fg(Color::DarkGray)),
                        Span::styled(bar, Style::default().fg(Color::Green)),
                    ]));
                } else if !d.task.checklist.is_empty() {
                    lines.push(Line::from(Span::styled("检查单:", Style::default().fg(Color::DarkGray))));
                    for item in &d.task.checklist {
                        let check = if item.done { "[x]" } else { "[ ]" };
                        let c = if item.done { Color::Green } else { Color::DarkGray };
                        lines.push(Line::from(Span::styled(format!("  {} {}", check, item.title), Style::default().fg(c))));
                    }
                }

                // 番茄钟计数
                let pomo_count = d.events.iter().filter(|e| e.event_type == event::EV_POMODORO).count();
                if pomo_count > 0 {
                    let tomatoes = " ".repeat(pomo_count);
                    lines.push(Line::from(vec![
                        Span::styled("专注: ", Style::default().fg(Color::DarkGray)),
                        Span::styled(format!("{} ({})", tomatoes, pomo_count), Style::default().fg(Color::Red)),
                    ]));
                }

                // 分隔线
                lines.push(Line::from("─".repeat((area.width.saturating_sub(4)) as usize)));

                // 备注
                if d.task.notes.trim().is_empty() {
                    lines.push(Line::from(Span::styled("备注: -", Style::default().fg(Color::DarkGray))));
                } else {
                    lines.push(Line::from(Span::styled("备注:", Style::default().add_modifier(Modifier::BOLD))));
                    for ln in d.task.notes.split('\n') {
                        lines.push(Line::from(format!("  {}", ln)));
                    }
                }

                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled("时间线", Style::default().add_modifier(Modifier::UNDERLINED))));
                for e in d.events.iter().rev().take(6).rev() {
                    let event_cn = match e.event_type.as_str() {
                        "created" => "创建",
                        "status_change" => "流转",
                        event::EV_POMODORO => "专注",
                        event::EV_HABIT_COMPLETED => "习惯",
                        event::EV_RESTORED => "恢复",
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

                    lines.push(Line::from(format!("  {} {} {}", time::format_local(Some(e.at)), event_cn, action)));
                }

                let para = Paragraph::new(lines)
                    .block(block)
                    .wrap(ratatui::widgets::Wrap { trim: false });
                f.render_widget(para, area);
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

        // 左右分栏：左=状态分布，右=近7天完成趋势图
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
            .split(area);

        // 左：状态分布
        let lines = vec![
            Line::from(Span::styled(" 状态分布", Style::default().add_modifier(Modifier::BOLD))),
            Line::from(""),
            Line::from(format!(" 收件箱   : {}", c("inbox"))),
            Line::from(format!(" 下一步   : {}", c("next"))),
            Line::from(format!(" 等待中   : {}", c("waiting"))),
            Line::from(format!(" 已排程   : {}", c("scheduled"))),
            Line::from(format!(" 将来/也许: {}", c("someday"))),
            Line::from(format!(" 参考资料 : {}", c("reference"))),
            Line::from(format!(" 已完成   : {}", c("done"))),
        ];
        let para = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL).border_set(border::ROUNDED)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Review "),
        );
        f.render_widget(para, chunks[0]);

        // 右：近7天完成趋势
        let data = tasks::completed_counts_last_days(self.conn, 7).unwrap_or_default();
        let labels: Vec<String> = data.iter().map(|(d, _)| d.clone()).collect();
        let values: Vec<u64> = data.iter().map(|(_, n)| *n as u64).collect();
        let maxv = values.iter().cloned().max().unwrap_or(0).max(1);
        let chart_data: Vec<(&str, u64)> = labels.iter().map(|s| s.as_str()).zip(values.iter().cloned()).collect();
        let chart = ratatui::widgets::BarChart::default()
            .block(
                Block::default()
                    .borders(Borders::ALL).border_set(border::ROUNDED)
                    .border_style(Style::default().fg(Color::DarkGray))
                    .title(" 近7天完成趋势 "),
            )
            .data(&chart_data)
            .bar_width(6)
            .bar_style(Style::default().fg(Color::Green))
            .value_style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
            .label_style(Style::default().fg(Color::DarkGray))
            .max(maxv);
        f.render_widget(chart, chunks[1]);
    }
}
