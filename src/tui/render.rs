use super::app::{pad_right, App, Mode, Pane, View};
use super::ui;
use super::ui::build_list_items;
use super::{next_hint, status_cn};
use crate::model::{event, task};
use crate::repo::tasks::{self, ListFilter};
use crate::time;
use ratatui::symbols::border;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        canvas::{Canvas, Points},
        Block, Borders, List, Paragraph,
    },
    Frame,
};

pub(crate) trait AppRender {
    fn render(&mut self, f: &mut Frame);
    fn render_focus_mode(&mut self, f: &mut Frame, area: Rect);
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

        // ── 番茄专注模式：全屏接管 ──
        {
            let pomo = crate::repo::pomodoro::get_state().unwrap_or_default();
            if !matches!(pomo.phase, crate::model::pomodoro::Phase::Idle) {
                self.hide_pomo_banner = false; // reset so it shows up next time it becomes Idle
                self.render_focus_mode(f, size);
                return;
            }
        }

        let mut main_area = size;
        if self.is_reviewing {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(0)])
                .split(size);

            let step_names = [
                "",
                "清空收件箱",
                "检视项目",
                "追踪等待事项",
                "重估将来/也许",
            ];
            let step_name = step_names.get(self.review_step as usize).unwrap_or(&"");

            let banner = Paragraph::new(Line::from(Span::styled(
                format!(
                    " 🌟 每周回顾 第 {}/4 步: {} (按 'R' 进入下一步, 'Esc' 退出) ",
                    self.review_step, step_name
                ),
                Style::default()
                    .bg(Color::Cyan)
                    .fg(self.theme.bg)
                    .add_modifier(Modifier::BOLD),
            )));
            f.render_widget(banner, chunks[0]);
            main_area = chunks[1];
        } else if !self.hide_pomo_banner {
            let pomo = crate::repo::pomodoro::get_state().unwrap_or_default();
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            let today_active = pomo.last_date.as_deref() == Some(today.as_str());

            if today_active && pomo.today_count > 0 {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(1), Constraint::Min(0)])
                    .split(size);

                let last_title = pomo
                    .last_completed_task_title
                    .as_deref()
                    .unwrap_or("上一任务");
                let banner = Paragraph::new(Line::from(vec![
                    Span::styled(
                        format!(
                            " 󰗠 成就结清: 今日已积 {} 个番茄 (Streak {} 连击!)  |  ",
                            pomo.today_count, pomo.streak
                        ),
                        Style::default()
                            .fg(self.theme.bg)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(
                            "休息已完成  |  再接再厉? 󰄾 [Space/P] 开启新一轮专注 [{}] ",
                            last_title
                        ),
                        Style::default()
                            .fg(self.theme.bg)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]))
                .alignment(ratatui::layout::Alignment::Center)
                .style(Style::default().bg(self.theme.text_success));

                f.render_widget(banner, chunks[0]);
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
        let mode_str = match self.mode {
            Mode::Normal => " NORMAL ",
            Mode::Visual => " VISUAL ",
            _ => " INSERT ",
        };
        let mode_bg = match self.mode {
            Mode::Normal => self.theme.text_success,
            Mode::Visual => self.theme.accent,
            _ => self.theme.text_urgent,
        };
        let mode_fg = self.theme.bg;

        let status_left = Line::from(vec![
            Span::styled(
                mode_str,
                Style::default()
                    .fg(mode_fg)
                    .bg(mode_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {} ", self.view.label()),
                Style::default()
                    .fg(self.theme.fg)
                    .bg(self.theme.status_bg)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);

        let status_msg = if self.status_message.is_empty() {
            String::new()
        } else {
            format!(" {} ", self.status_message)
        };
        let status_right = Line::from(vec![
            Span::styled(
                status_msg,
                Style::default().fg(self.theme.status_fg).bg(self.theme.status_bg),
            ),
            Span::styled(
                " gtp ".to_string(),
                Style::default()
                    .fg(self.theme.bg)
                    .bg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);

        let status_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(chunks[1]);

        f.render_widget(
            Paragraph::new(status_left).style(Style::default().bg(self.theme.status_bg)),
            status_layout[0],
        );
        f.render_widget(
            Paragraph::new(status_right)
                .style(Style::default().bg(self.theme.status_bg))
                .alignment(Alignment::Right),
            status_layout[1],
        );

        if self.mode != Mode::Normal
            && self.mode != Mode::SchedulingCalendar
            && self.mode != Mode::ConfirmArchive
        {
            let title = match self.mode {
                Mode::Search => " Search Tasks (Title / Notes) ",
                Mode::EditingTitle => " Edit title ",
                Mode::Capturing => {
                    " 快速录入 (支持 @标签 及 Tab 补全: home, work, errands, quick, focus...) "
                }
                Mode::Tagging => {
                    " 添加标签 [支持 Tab 补全] (预设: home, work, errands, quick, focus...) "
                }
                Mode::SchedulingTimeRRule => {
                    " 设定时间与循环规则 (格式: 15:00-16:00 ;FREQ=WEEKLY;BYDAY=SA,SU) "
                }
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
                Mode::ConfiguringPomo => {
                    " 自定义番茄钟时长 (格式: 工作分钟;短休分钟;长休分钟, 如 25;5;15) "
                }
                Mode::Normal | Mode::Visual | Mode::ConfirmArchive => "",
            };

            let mut text_lines = vec![Line::from(format!(" {}_", self.input))];
            let mut height = 3;
            let width = if self.mode == Mode::Capturing { 70 } else { 50 };

            if self.mode == Mode::Capturing {
                text_lines.push(Line::from(""));
                text_lines.push(Line::from(Span::styled(
                    " [语法] @标签 (如 @work)  |  ~时间 (如 ~tomorrow, ~+3d, ~18:00)",
                    Style::default().fg(self.theme.text_dim),
                )));
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
                let block = Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_set(border::ROUNDED).padding(ratatui::widgets::Padding::horizontal(1))
                    .border_style(Style::default().fg(if self.pane == Pane::Right { self.theme.border_active } else { self.theme.border_inactive }));
                f.render_widget(Paragraph::new(text_lines).block(block), left_area);
            } else {
                let area = self.centered_rect(width, height, size);
                f.render_widget(ratatui::widgets::Clear, area);
                let block = Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_set(border::ROUNDED).padding(ratatui::widgets::Padding::horizontal(1))
                    .border_style(Style::default().fg(self.theme.accent));
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

    fn render_focus_mode(&mut self, f: &mut Frame, area: Rect) {
        use crate::model::pomodoro::Phase;

        let pomo = crate::repo::pomodoro::get_state().unwrap_or_default();
        let now = crate::time::now_ms();

        // ── 时间计算 ──
        let start_ts = pomo.start_ts.unwrap_or(now);
        let end_ts = pomo.end_ts.unwrap_or(now);
        let total_ms = (end_ts - start_ts).max(1) as f64;
        let elapsed_fraction = ((now - start_ts) as f64 / total_ms).clamp(0.0, 1.0);

        let diff_secs = ((end_ts - now) / 1000).max(0);
        let mins = diff_secs / 60;
        let secs = diff_secs % 60;
        let time_str = format!("{:02}:{:02}", mins, secs);

        // ── 阶段配色 ──
        let (phase_icon, ring_color, _dim_color, bg_color) = match pomo.phase {
            Phase::Work => (
                "🍅 专注",
                Color::Rgb(230, 60, 60), // Keep the red tomato vibe
                Color::Rgb(70, 25, 25),
                self.theme.bg,
            ),
            Phase::ShortBreak => (
                "☕ 小休",
                Color::Rgb(60, 210, 110),
                Color::Rgb(20, 65, 35),
                self.theme.bg,
            ),
            Phase::LongBreak => (
                "🌿 长休",
                Color::Rgb(60, 150, 230),
                Color::Rgb(20, 45, 75),
                self.theme.bg,
            ),
            Phase::Idle => return,
        };

        // ── 全屏背景 ──
        f.render_widget(Block::default().style(Style::default().bg(bg_color)), area);

        // ── 居中布局 ──
        let total_height = 7 + 2 + 3 + 3; // 7 (time) + 2 (title) + 3 (sloth bar) + 3 (stats)
        let top_padding = area.height.saturating_sub(total_height) / 2;

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(top_padding.max(1)), // Top space
                Constraint::Length(7),                  // Big Time
                Constraint::Length(2),                  // Task Title
                Constraint::Length(3),                  // Sloth Progress
                Constraint::Min(3),                     // Stats & Hints
            ])
            .split(area);

        // ── 1. 大数字倒计时 ──
        let blink = secs % 2 == 0;
        let big_lines = build_big_time(&time_str, ring_color, bg_color, blink);
        f.render_widget(
            Paragraph::new(big_lines)
                .alignment(Alignment::Center)
                .style(Style::default().bg(bg_color)),
            rows[1],
        );

        // ── 2. 当前任务与状态 ──
        let task_title = pomo.task_title.as_deref().unwrap_or("无标题");
        let title_line = Line::from(vec![
            Span::styled(format!(" {} ", phase_icon), Style::default().fg(ring_color).add_modifier(Modifier::BOLD)),
            Span::styled(" │ ", Style::default().fg(self.theme.text_dim)),
            Span::styled(task_title, Style::default().fg(self.theme.fg).add_modifier(Modifier::BOLD)),
        ]);
        f.render_widget(
            Paragraph::new(title_line)
                .alignment(Alignment::Center)
                .style(Style::default().bg(bg_color)),
            rows[2],
        );

        // ── 3. 点阵频谱律动 (Braille Wave) ──
        let gauge_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(15),
                Constraint::Percentage(70),
                Constraint::Percentage(15),
            ])
            .split(rows[3]);

        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64;
        let t = now / 250.0; // 动画速度

        let wave_canvas = Canvas::default()
            .marker(ratatui::symbols::Marker::Braille)
            .x_bounds([0.0, 100.0])
            .y_bounds([0.0, 10.0])
            .paint(move |ctx| {
                let mut points = Vec::new();
                let mut dim_points = Vec::new();
                
                let max_x = 100;
                for x in 0..max_x {
                    let is_active = (x as f64) <= elapsed_fraction * 100.0;
                    
                    let height = if is_active {
                        let wave1 = ((x as f64 / 8.0) - t).sin();
                        let wave2 = ((x as f64 / 15.0) + t * 1.3).cos();
                        let wave3 = ((x as f64 / 3.0) - t * 2.0).sin() * 0.5;
                        let normalized = (wave1 + wave2 + wave3 + 2.5) / 5.0;
                        (normalized * 10.0).clamp(1.0, 10.0)
                    } else {
                        1.0 
                    };
                    
                    for y in 0..=10 {
                        if (y as f64) <= height {
                            if is_active {
                                points.push((x as f64, y as f64));
                            } else {
                                dim_points.push((x as f64, y as f64));
                            }
                        }
                    }
                }
                
                ctx.draw(&Points {
                    coords: &dim_points,
                    color: Color::DarkGray,
                });
                
                ctx.draw(&Points {
                    coords: &points,
                    color: ring_color,
                });
            });

        f.render_widget(wave_canvas, gauge_layout[1]);

        // ── 4. 克制的统计信息 & 快捷键 ──
        let stats_hint_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // empty space
                Constraint::Length(1), // stats and hints
            ])
            .split(rows[4]);

        let hints = if matches!(pomo.phase, Phase::ShortBreak | Phase::LongBreak) {
            "󰄾 [Space/P] 下一轮  |  [S] 结束专注"
        } else {
            "󰄾 [S] 停止番茄钟"
        };
        
        let stats_line = Line::from(vec![
            Span::styled(format!(" 🏆 今日完成: {} ", pomo.today_count), Style::default().fg(self.theme.text_dim)),
            Span::styled(" • ", Style::default().fg(self.theme.text_dim)),
            Span::styled(format!(" 🔥 连击: {} ", pomo.streak), Style::default().fg(self.theme.text_dim)),
            Span::styled("      |      ", Style::default().fg(self.theme.border_inactive)),
            Span::styled(hints, Style::default().fg(self.theme.text_dim)),
        ]);
        
        f.render_widget(
            Paragraph::new(stats_line)
                .alignment(Alignment::Center)
                .style(Style::default().bg(bg_color)),
            stats_hint_layout[1],
        );
    }

    fn render_help_drawer(&self, f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        f.render_widget(ratatui::widgets::Clear, area);
        let keys_block = Block::default()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED).padding(ratatui::widgets::Padding::horizontal(1))
            .border_style(Style::default().fg(self.theme.accent))
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
                ratatui::text::Line::from(Span::styled(
                    format!("{:>6} ", k),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
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
            .borders(Borders::ALL)
            .border_set(border::ROUNDED).padding(ratatui::widgets::Padding::horizontal(1))
            .border_style(Style::default().fg(self.theme.accent))
            .title(" 语法说明指南 (Ctrl+P) ");
        let syntax = vec![
            Line::from(Span::styled(
                "快速录入语法 (按 a 捕获)",
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("@标签", Style::default().fg(self.theme.text_success)),
                Span::raw("    添加情境或优先级, 如 "),
                Span::styled("@work @p1", Style::default().fg(Color::LightBlue)),
                Span::raw(" (支持 Tab 补全)"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("~时间", Style::default().fg(self.theme.text_success)),
                Span::raw("    设置截止时间, 见下方时间语法"),
            ]),
            Line::from(vec![
                Span::raw("  例: "),
                Span::styled(
                    "a买牛奶 @home ~tomorrow",
                    Style::default().fg(Color::LightBlue),
                ),
                Span::raw(" / "),
                Span::styled(
                    "a写周报 @work @p1 ~+3d",
                    Style::default().fg(Color::LightBlue),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "时间语法 (~ 与排期 c)",
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("now / +2h +30m +1d +1w", Style::default().fg(self.theme.text_success)),
                Span::raw("    相对时间偏移"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "today / tomorrow [HH:MM]",
                    Style::default().fg(self.theme.text_success),
                ),
                Span::raw("  今天/明天指定时刻"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("HH:MM", Style::default().fg(self.theme.text_success)),
                Span::raw("                     当天指定时刻, 如 18:00"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("YYYY-MM-DD [HH:MM]", Style::default().fg(self.theme.text_success)),
                Span::raw("        绝对日期与时间"),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "周期 / 循环任务 (Habit / RRULE)",
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(vec![
                Span::raw("  先按 "),
                Span::styled(
                    "c",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" 选排期日期, 再在 '时间;规则' 中输入 RRULE 即成为循环任务"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "FREQ=DAILY|WEEKLY|MONTHLY",
                    Style::default().fg(self.theme.text_success),
                ),
                Span::raw("   循环频率"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("INTERVAL=2", Style::default().fg(self.theme.text_success)),
                Span::raw("                  循环间隔 (如每 2 周)"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("BYDAY=SA,SU", Style::default().fg(self.theme.text_success)),
                Span::raw("                 指定周几 (MO TU WE TH FR SA SU)"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "COUNT=10 / UNTIL=YYYY-MM-DD",
                    Style::default().fg(self.theme.text_success),
                ),
                Span::raw(" 终止条件"),
            ]),
            Line::from(vec![
                Span::raw("  例: "),
                Span::styled(
                    ";FREQ=WEEKLY;BYDAY=SA,SU",
                    Style::default().fg(Color::LightBlue),
                ),
                Span::raw("    "),
                Span::styled(
                    ";FREQ=DAILY;COUNT=30",
                    Style::default().fg(Color::LightBlue),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "其他操作说明",
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(vec![
                Span::raw("  等待 "),
                Span::styled(
                    "w",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" 后可填写 [谁/何时], 如 "),
                Span::styled("w → Alice → +1d", Style::default().fg(Color::LightBlue)),
            ]),
            Line::from(vec![
                Span::raw("  子任务 "),
                Span::styled(
                    "C",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" 新增, "),
                Span::styled(
                    "Space",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" 依次打卡, 全部完成自动重置"),
            ]),
            Line::from(vec![
                Span::raw("  标签库 (视图9): 按 "),
                Span::styled(
                    "a",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" 动态新增, 按 "),
                Span::styled(
                    "D",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" 删除"),
            ]),
            Line::from(vec![
                Span::raw("  按 "),
                Span::styled(
                    "Ctrl+P",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" 弹出/关闭本语法说明指南"),
            ]),
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
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
        }

        let cur = self.view;
        let is_left_pane = self.pane == Pane::Left;

        let mut add_group = |views: &[(char, View)], title: &'static str| {
            lines.push(Line::from(Span::styled(
                title,
                Style::default()
                    .fg(self.theme.text_dim)
                    .add_modifier(Modifier::BOLD),
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
                    View::Tags => ("", "标签库"),
                };
                let padded_label = pad_right(label, 10);

                if active {
                    let mut style = Style::default()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD);
                    if is_left_pane {
                        style = style.add_modifier(Modifier::REVERSED);
                    }
                    lines.push(Line::from(Span::styled(
                        format!(" 󰄾 {} {} {} {:>3} ", key, icon, padded_label, cnt),
                        style,
                    )));
                } else {
                    lines.push(Line::from(vec![
                        Span::styled(format!("   {} ", key), Style::default().fg(self.theme.text_dim)),
                        Span::raw(format!("{} {} {:>3} ", icon, padded_label, cnt)),
                    ]));
                }
            }
            lines.push(Line::from(""));
        };

        add_group(&[('1', View::Inbox), ('2', View::Next)], "  [Active]");
        add_group(
            &[
                ('3', View::Waiting),
                ('4', View::Scheduled),
                ('5', View::Someday),
            ],
            "  [Waiting]",
        );
        add_group(&[('6', View::Reference), ('7', View::Done)], "  [Archive]");

        lines.push(Line::from(Span::styled(
            "  [Modules]",
            Style::default()
                .fg(self.theme.text_dim)
                .add_modifier(Modifier::BOLD),
        )));
        for (key, v) in &[
            ('p', View::Projects),
            ('r', View::Review),
            ('9', View::Tags),
            ('8', View::Archived),
        ] {
            let active = cur == *v;
            let (icon, label) = match v {
                View::Projects => ("", "项目树"),
                View::Review => ("", "周回顾"),
                View::Archived => ("", "归档箱"),
                View::Tags => ("", "标签库"),
                _ => ("", ""),
            };
            let padded_label = pad_right(label, 10);
            if active {
                let mut style = Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD);
                if is_left_pane {
                    style = style.add_modifier(Modifier::REVERSED);
                }
                lines.push(Line::from(Span::styled(
                    format!(" 󰄾 {} {} {}     ", key, icon, padded_label),
                    style,
                )));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(format!("   {} ", key), Style::default().fg(self.theme.text_dim)),
                    Span::raw(format!("{} {}     ", icon, padded_label)),
                ]));
            }
        }
        lines.push(Line::from(""));

        // 提示
        lines.push(Line::from(Span::styled(
            "  [Hint]",
            Style::default()
                .fg(self.theme.text_dim)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            format!("  {}", next_hint(self.view)),
            Style::default().fg(Color::Gray),
        )));

        let border_color = if self.pane == Pane::Left {
            self.theme.accent
        } else {
            self.theme.text_dim
        };
        f.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_set(border::ROUNDED).padding(ratatui::widgets::Padding::horizontal(1))
                    .border_style(Style::default().fg(border_color))
                    .title(" Guide "),
            ),
            area,
        );
    }

    fn render_list(&mut self, f: &mut ratatui::Frame, area: Rect) {
        let border_color = if self.pane == Pane::Center {
            self.theme.accent
        } else {
            self.theme.text_dim
        };
        let items = build_list_items(self);
        let title = format!(
            " Tasks · {}{} ",
            self.view.label(),
            if let Some(ref tf) = self.tag_filter {
                format!(" [@{}]", tf)
            } else {
                String::new()
            }
        );
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_set(border::ROUNDED).padding(ratatui::widgets::Padding::horizontal(1))
                    .border_style(Style::default().fg(border_color))
                    .title(title),
            )
            .highlight_style(if self.pane == Pane::Center {
                Style::default().bg(self.theme.hl_bg).fg(self.theme.hl_fg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().bg(self.theme.hl_bg)
            });
        f.render_stateful_widget(list, area, &mut self.list_state);
    }

    fn render_detail(&mut self, f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        f.render_widget(ratatui::widgets::Clear, area);
        let border_color = if self.pane == Pane::Right {
            self.theme.accent
        } else {
            self.theme.text_dim
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED).padding(ratatui::widgets::Padding::horizontal(1))
            .border_style(Style::default().fg(border_color))
            .title(" 任务详情 ");

        match &self.detail {
            None => {
                let empty_para = Paragraph::new("\n\n󰋔\n\n未选中任务")
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(self.theme.text_dim).add_modifier(Modifier::ITALIC))
                    .block(block);
                f.render_widget(empty_para, area);
            }
            Some(d) => {
                let mut lines: Vec<Line> = vec![];

                // 标题
                lines.push(Line::from(vec![
                    Span::styled(
                        "标题: ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        d.task.title.clone(),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                ]));

                // 状态
                let st_color = ui::status_color(&d.task.status);
                lines.push(Line::from(vec![
                    Span::styled("状态: ", Style::default().fg(self.theme.text_dim)),
                    Span::styled(
                        status_cn(d.task.status),
                        Style::default().fg(st_color).add_modifier(Modifier::BOLD),
                    ),
                ]));

                // 项目
                if let Some(p) = &d.task.parent_id {
                    lines.push(Line::from(vec![
                        Span::styled("项目: ", Style::default().fg(self.theme.text_dim)),
                        Span::raw(p[..8].to_string()),
                    ]));
                }

                // 截止时间
                lines.push(Line::from(vec![
                    Span::styled("截止: ", Style::default().fg(self.theme.text_dim)),
                    Span::raw(time::format_local(d.task.due_at)),
                ]));

                // 计划时间
                if d.task.scheduled_start_at.is_some() || d.task.scheduled_end_at.is_some() {
                    lines.push(Line::from(vec![
                        Span::styled("计划: ", Style::default().fg(self.theme.text_dim)),
                        Span::raw(format!(
                            "{} -> {}",
                            time::format_local(d.task.scheduled_start_at),
                            time::format_local(d.task.scheduled_end_at)
                        )),
                    ]));
                }

                // 循环规则
                if let Some(rr) = &d.task.rrule {
                    let cn_rr = rr
                        .replace("FREQ=DAILY", "每天")
                        .replace("FREQ=WEEKLY", "每周")
                        .replace("FREQ=MONTHLY", "每月")
                        .replace("INTERVAL=", "间隔=")
                        .replace("COUNT=", "次数=")
                        .replace("UNTIL=", "直到=");
                    lines.push(Line::from(vec![
                        Span::styled("循环: ", Style::default().fg(self.theme.text_dim)),
                        Span::raw(cn_rr),
                    ]));
                }

                // 标签
                if !d.tags.is_empty() {
                    let mut tag_spans =
                        vec![Span::styled("标签: ", Style::default().fg(self.theme.text_dim))];
                    for (i, tg) in d.tags.iter().enumerate() {
                        let c = ui::priority_color(&tg.name).unwrap_or(Color::Cyan);
                        tag_spans.push(Span::styled(
                            format!("@{}", tg.name),
                            Style::default().fg(c),
                        ));
                        if i < d.tags.len() - 1 {
                            tag_spans.push(Span::raw(" "));
                        }
                    }
                    lines.push(Line::from(tag_spans));
                }

                // 委派
                if let Some(del) = &d.task.delegated_to {
                    lines.push(Line::from(vec![
                        Span::styled("委派: ", Style::default().fg(self.theme.text_dim)),
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
                    let done = children
                        .iter()
                        .filter(|c| c.status == task::Status::Done)
                        .count();
                    let bar = if total > 0 {
                        let filled = (done as f64 / total as f64 * 10.0).round() as usize;
                        format!(
                            "[{}{}] {}/{}",
                            "█".repeat(filled),
                            "░".repeat(10 - filled),
                            done,
                            total
                        )
                    } else {
                        "[----------] 0/0".to_string()
                    };
                    lines.push(Line::from(vec![
                        Span::styled("进度: ", Style::default().fg(self.theme.text_dim)),
                        Span::styled(bar, Style::default().fg(self.theme.text_success)),
                    ]));
                } else if !d.task.checklist.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "检查单:",
                        Style::default().fg(self.theme.text_dim),
                    )));
                    for item in &d.task.checklist {
                        let check = if item.done { "[x]" } else { "[ ]" };
                        let c = if item.done {
                            self.theme.text_success
                        } else {
                            self.theme.text_dim
                        };
                        lines.push(Line::from(Span::styled(
                            format!("  {} {}", check, item.title),
                            Style::default().fg(c),
                        )));
                    }
                }

                // 番茄钟计数
                let pomo_count = d
                    .events
                    .iter()
                    .filter(|e| e.event_type == event::EV_POMODORO)
                    .count();
                if pomo_count > 0 {
                    let tomatoes = " ".repeat(pomo_count);
                    lines.push(Line::from(vec![
                        Span::styled("专注: ", Style::default().fg(self.theme.text_dim)),
                        Span::styled(
                            format!("{} ({})", tomatoes, pomo_count),
                            Style::default().fg(self.theme.text_urgent),
                        ),
                    ]));
                }

                // 分隔线
                lines.push(Line::from(
                    "─".repeat((area.width.saturating_sub(4)) as usize),
                ));

                // 备注
                if d.task.notes.trim().is_empty() {
                    lines.push(Line::from(Span::styled(
                        "备注: -",
                        Style::default().fg(self.theme.text_dim),
                    )));
                } else {
                    lines.push(Line::from(Span::styled(
                        "备注:",
                        Style::default().add_modifier(Modifier::BOLD),
                    )));
                    for ln in d.task.notes.split('\n') {
                        lines.push(Line::from(format!("  {}", ln)));
                    }
                }

                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "时间线",
                    Style::default().add_modifier(Modifier::UNDERLINED),
                )));
                for e in d.events.iter().rev().take(6).rev() {
                    let event_cn = match e.event_type.as_str() {
                        "created" => "创建",
                        "status_change" => "流转",
                        event::EV_POMODORO => "专注",
                        event::EV_HABIT_COMPLETED => "习惯",
                        event::EV_RESTORED => "恢复",
                        _ => &e.event_type,
                    };

                    let from_cn = e
                        .from_status
                        .as_deref()
                        .unwrap_or("-")
                        .parse::<crate::model::task::Status>()
                        .map(status_cn)
                        .unwrap_or("-");
                    let to_cn = e
                        .to_status
                        .as_deref()
                        .unwrap_or("-")
                        .parse::<crate::model::task::Status>()
                        .map(status_cn)
                        .unwrap_or("-");

                    let action = if e.event_type == "status_change" {
                        format!("{} -> {}", from_cn, to_cn)
                    } else if e.event_type == event::EV_POMODORO {
                        "🍅 +1".to_string()
                    } else {
                        "".to_string()
                    };

                    lines.push(Line::from(format!(
                        "  {} {} {}",
                        time::format_local(Some(e.at)),
                        event_cn,
                        action
                    )));
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
            Line::from(Span::styled(
                " 状态分布",
                Style::default().add_modifier(Modifier::BOLD),
            )),
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
                .borders(Borders::ALL)
                .border_set(border::ROUNDED).padding(ratatui::widgets::Padding::horizontal(1))
                .border_style(Style::default().fg(if self.pane == Pane::Left { self.theme.border_active } else { self.theme.border_inactive }))
                .title(" Review "),
        );
        f.render_widget(para, chunks[0]);

        // 右：近7天完成趋势
        let data = tasks::completed_counts_last_days(self.conn, 7).unwrap_or_default();
        let labels: Vec<String> = data.iter().map(|(d, _)| d.clone()).collect();
        let values: Vec<u64> = data.iter().map(|(_, n)| *n as u64).collect();
        let maxv = values.iter().cloned().max().unwrap_or(0).max(1);
        let chart_data: Vec<(&str, u64)> = labels
            .iter()
            .map(|s| s.as_str())
            .zip(values.iter().cloned())
            .collect();
        let chart = ratatui::widgets::BarChart::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_set(border::ROUNDED).padding(ratatui::widgets::Padding::horizontal(1))
                    .border_style(Style::default().fg(if self.pane == Pane::Center { self.theme.border_active } else { self.theme.border_inactive }))
                    .title(" 近7天完成趋势 "),
            )
            .data(&chart_data)
            .bar_width(6)
            .bar_style(Style::default().fg(self.theme.text_success))
            .value_style(
                Style::default()
                    .fg(self.theme.fg)
                    .add_modifier(Modifier::BOLD),
            )
            .label_style(Style::default().fg(self.theme.text_dim))
            .max(maxv);
        f.render_widget(chart, chunks[1]);
    }
}

// ── 大数字字体辅助（5 行 × 4 列，纯 █ 字符）──

fn big_digit_rows(c: char, blink: bool) -> [&'static str; 5] {
    match c {
        '0' => [" ██ ", "█  █", "█  █", "█  █", " ██ "],
        '1' => [" ▐█ ", " ██ ", "  █ ", "  █ ", " ███"],
        '2' => [" ██ ", "   █", " ██ ", "█   ", "████"],
        '3' => ["███ ", "   █", " ██ ", "   █", "███ "],
        '4' => ["█  █", "█  █", "████", "   █", "   █"],
        '5' => ["████", "█   ", "███ ", "   █", "███ "],
        '6' => [" ██ ", "█   ", "███ ", "█  █", " ██ "],
        '7' => ["████", "   █", "  █ ", " █  ", " █  "],
        '8' => [" ██ ", "█  █", " ██ ", "█  █", " ██ "],
        '9' => [" ██ ", "█  █", " ███", "   █", " ██ "],
        ':' => if blink { ["    ", " ██ ", "    ", " ██ ", "    "] } else { ["    ", "    ", "    ", "    ", "    "] },
        _ => ["    ", "    ", "    ", "    ", "    "],
    }
}

/// 将形如 "23:45" 的字符串渲染为 5 行大数字（每行是一个 Span）。
fn build_big_time(s: &str, color: Color, bg: Color, blink: bool) -> Vec<Line<'static>> {
    let chars: Vec<char> = s.chars().collect();
    let mut rows: [String; 5] = Default::default();
    for &c in &chars {
        let digit = big_digit_rows(c, blink);
        for (i, part) in digit.iter().enumerate() {
            rows[i].push_str(part);
            rows[i].push(' '); // 字符间距
        }
    }
    rows.into_iter()
        .map(|row| {
            Line::from(Span::styled(
                row,
                Style::default()
                    .fg(color)
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            ))
        })
        .collect()
}



