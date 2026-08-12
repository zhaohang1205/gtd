use anyhow::Result;
use ratatui::widgets::ListState;
use rusqlite::Connection;

use super::row_from;
use crate::model::event::TaskEvent;
use crate::model::tag::Tag;
use crate::model::task::{self, Task};
use crate::repo::tags;
use crate::repo::tasks::{self, ListFilter};

use super::calendar;

pub(crate) fn visual_len(s: &str) -> usize {
    s.chars()
        .map(|c| {
            if c.is_ascii() || ('\u{E000}'..='\u{F8FF}').contains(&c) {
                1
            } else {
                2
            }
        })
        .sum()
}

pub(crate) fn pad_right(s: &str, width: usize) -> String {
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
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum View {
    Inbox,
    Today,
    Tomorrow,
    Next,
    Waiting,
    Scheduled,
    Someday,
    Reference,
    Done,
    Review,
    Archived,
    Tags,
}

impl View {
    /// 状态视图对应的状态字符串（用于查询与中文展示）。
    pub(crate) fn status(self) -> Option<&'static str> {
        match self {
            View::Inbox => Some("inbox"),
            View::Next => Some("next"),
            View::Waiting => Some("waiting"),
            View::Scheduled => Some("scheduled"),
            View::Someday => Some("someday"),
            View::Reference => Some("reference"),
            View::Done => Some("done"),
            View::Today | View::Tomorrow | View::Review | View::Archived | View::Tags => None,
        }
    }

    /// 数字键 1-7 映射到的视图。
    pub(crate) fn from_digit(d: char) -> Option<View> {
        match d {
            '1' => Some(View::Inbox),
            '2' => Some(View::Next),
            '3' => Some(View::Waiting),
            '4' => Some(View::Scheduled),
            '5' => Some(View::Someday),
            '6' => Some(View::Reference),
            '7' => Some(View::Done),
            '8' => Some(View::Archived),
            '9' => Some(View::Tags),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Normal,
    EditingTitle,
    Capturing,
    Tagging,
    SchedulingCalendar,
    SchedulingTimeRRule,
    WaitingWho,
    WaitingWhen,
    Search,
    /// 计划钩子第 2 步：询问预计时间。
    PlanningTime,
    ChecklistAdding,
    Visual,
    FilteringTag,
    /// 归档前确认：收集待归档的 id，等待 y/Enter 确认或 n/Esc 取消。
    ConfirmArchive,
    /// 编辑截止时间 (due)
    EditingDue,
    /// 编辑循环规则 (rrule)
    EditingRrule,
    /// 编辑委派对象 (delegated_to)
    EditingDelegated,
    /// 新增自定义标签
    CreatingTag,
    /// 配置番茄钟时长 (工作;短休;长休)
    ConfiguringPomo,
}

impl Mode {
    pub(crate) fn is_input(&self) -> bool {
        !matches!(self, Mode::Normal | Mode::Visual)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pane {
    Left,
    Center,
    Right,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Popup {
    /// Show today's tasks summary on startup
    TodayTasks(Vec<String>),
    /// Prompt to enter Pomodoro mode for a scheduled task
    TaskDueNow(String, String), // task_id, task_title
}

#[derive(Clone)]
pub(crate) struct Row {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) status: String,
    pub(crate) due: Option<i64>,
    pub(crate) tags: Vec<String>,
    pub(crate) indent: usize,
    /// 完成进度（用于项目/带检查单的任务）：已完成数，None 表示无进度概念。
    pub(crate) done: Option<usize>,
    /// 完成进度：总数。
    pub(crate) total: Option<usize>,
    /// 归档原因（仅归档箱视图非空）：completed | deleted。
    pub(crate) archive_reason: Option<String>,
}

pub(crate) struct DetailData {
    pub(crate) task: Task,
    pub(crate) tags: Vec<Tag>,
    pub(crate) events: Vec<TaskEvent>,
}

pub(crate) struct App<'a> {
    pub(crate) conn: &'a Connection,
    pub(crate) view: View,
    pub(crate) items: Vec<Row>,
    pub(crate) selected: usize,
    pub(crate) list_state: ListState,
    pub(crate) detail: Option<DetailData>,
    pub(crate) mode: Mode,
    pub(crate) pane: Pane,
    pub(crate) input: String,
    pub(crate) status_message: String,
    pub(crate) lang: crate::i18n::Lang,
    pub(crate) show_help: bool,
    pub(crate) show_syntax: bool,
    pub(crate) help_scroll: usize,
    pub(crate) should_quit: bool,
    pub(crate) calendar: calendar::CalendarState,
    pub(crate) sched_dates: Option<(chrono::NaiveDate, chrono::NaiveDate)>,
    pub(crate) search_query: String,
    pub(crate) tag_filter: Option<String>,
    pub(crate) visual_start_idx: Option<usize>,
    pub(crate) selected_ids: std::collections::HashSet<String>,
    pub(crate) is_reviewing: bool,
    pub(crate) review_step: u8,
    pub(crate) needs_clear: bool,
    pub(crate) pending_archive_ids: Vec<String>,
    pub(crate) hide_pomo_banner: bool,
    pub(crate) theme: crate::tui::theme::Theme,
    pub(crate) popup: Option<Popup>,
    pub(crate) last_tick_ms: i64,
    pub(crate) notified_events: std::collections::HashSet<String>,
    /// 今日/明日视图任务数缓存：仅在 `refresh`（切视图/操作后）时重算，
    /// 避免引导栏计数每帧都做一次 rrule 展开。
    pub(crate) today_count: usize,
    pub(crate) tomorrow_count: usize,
}

impl<'a> App<'a> {
    pub(crate) fn new(conn: &'a Connection) -> Result<Self> {
        // 从 settings 表恢复语言与主题。
        let lang = match crate::repo::settings::get(conn, "lang")
            .ok()
            .flatten()
            .as_deref()
        {
            Some("en") => crate::i18n::Lang::En,
            _ => crate::i18n::Lang::Zh,
        };
        let theme = match crate::repo::settings::get(conn, "theme")
            .ok()
            .flatten()
            .as_deref()
        {
            Some("latte") => crate::tui::theme::Theme::catppuccin_latte(),
            _ => crate::tui::theme::Theme::catppuccin_mocha(),
        };
        let mut app = App {
            conn,
            view: View::Inbox,
            items: Vec::new(),
            selected: 0,
            list_state: ListState::default(),
            detail: None,
            mode: Mode::Normal,
            pane: Pane::Left,
            input: String::new(),
            status_message: crate::tr!(lang, "按 '?' 或 F1 查看帮助", "Press '?' or F1 for help")
                .to_string(),
            lang,
            show_help: false,
            show_syntax: false,
            help_scroll: 0,
            should_quit: false,
            calendar: calendar::CalendarState::new(),
            sched_dates: None,
            search_query: String::new(),
            tag_filter: None,
            visual_start_idx: None,
            selected_ids: std::collections::HashSet::new(),
            is_reviewing: false,
            review_step: 0,
            needs_clear: false,
            pending_archive_ids: Vec::new(),
            hide_pomo_banner: false,
            theme,
            popup: None,
            last_tick_ms: 0,
            notified_events: std::collections::HashSet::new(),
            today_count: 0,
            tomorrow_count: 0,
        };
        app.refresh()?;

        // --- Startup Today Tasks Popup ---
        let all_tasks = tasks::list(
            conn,
            &ListFilter {
                status: None,
                tags: vec![],
                query: None,
                review_stale: false,
            },
        )
        .unwrap_or_default();

        let today_start = chrono::Local::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp();
        let today_end = chrono::Local::now()
            .date_naive()
            .and_hms_opt(23, 59, 59)
            .unwrap()
            .and_utc()
            .timestamp();

        let mut todays = Vec::new();
        for t in &all_tasks {
            if let Some(due) = t.due_at {
                if due >= today_start && due <= today_end {
                    todays.push(t.title.clone());
                }
            }
        }

        if !todays.is_empty() {
            app.popup = Some(Popup::TodayTasks(todays));
        }

        app.load_detail();
        app.switch_to_english_ime();
        Ok(app)
    }

    pub(crate) fn check_notifications(&mut self) {
        let now = chrono::Local::now().timestamp();
        if now - self.last_tick_ms < 10 {
            return;
        }
        self.last_tick_ms = now;

        let all_tasks = tasks::list(
            self.conn,
            &ListFilter {
                status: None,
                tags: vec![],
                query: None,
                review_stale: false,
            },
        )
        .unwrap_or_default();
        for t in all_tasks {
            if let Some(due) = t.due_at {
                let diff = due - now;

                // 1 hour
                if diff > 0 && diff <= 3600 && diff > 3540 {
                    let key = format!("{}-1h", t.id);
                    if !self.notified_events.contains(&key) {
                        self.notified_events.insert(key);
                        let _ = notify_rust::Notification::new()
                            .summary("任务即将在1小时后开始")
                            .body(&t.title)
                            .appname("GTD")
                            .show();
                    }
                }

                // 10 mins
                if diff > 0 && diff <= 600 && diff > 540 {
                    let key = format!("{}-10m", t.id);
                    if !self.notified_events.contains(&key) {
                        self.notified_events.insert(key);
                        let _ = notify_rust::Notification::new()
                            .summary("任务即将在10分钟后开始")
                            .body(&t.title)
                            .appname("GTD")
                            .show();
                    }
                }

                // Due now
                if diff <= 0 && diff > -60 {
                    let key = format!("{}-now", t.id);
                    if !self.notified_events.contains(&key) {
                        self.notified_events.insert(key);
                        let _ = notify_rust::Notification::new()
                            .summary("任务现在开始!")
                            .body(&t.title)
                            .appname("GTD")
                            .show();
                        self.popup = Some(Popup::TaskDueNow(t.id.clone(), t.title.clone()));
                        self.needs_clear = true; // force redraw to show popup
                    }
                }
            }
        }
    }

    pub(crate) fn set_mode(&mut self, new_mode: Mode) {
        let old_mode = self.mode;
        self.mode = new_mode;
        if old_mode.is_input() && !new_mode.is_input() {
            self.switch_to_english_ime();
        }
    }

    pub(crate) fn switch_to_english_ime(&self) {
        // Try fcitx5-remote
        if std::process::Command::new("fcitx5-remote")
            .arg("-c")
            .status()
            .is_ok()
        {
            return;
        }
        // Try fcitx-remote
        if std::process::Command::new("fcitx-remote")
            .arg("-c")
            .status()
            .is_ok()
        {
            return;
        }
        // Try ibus
        let _ = std::process::Command::new("ibus")
            .args(["engine", "xkb:us::eng"])
            .status();
        // Try im-select (macOS / Windows / Cross-platform helper if installed)
        let _ = std::process::Command::new("im-select")
            .arg("com.apple.keylayout.ABC")
            .status();
        let _ = std::process::Command::new("im-select").arg("1033").status();
    }

    pub(crate) fn update_visual_selection(&mut self) {
        if self.mode == Mode::Visual {
            if let Some(start) = self.visual_start_idx {
                self.selected_ids.clear();
                let min_idx = start.min(self.selected);
                let max_idx = start.max(self.selected);
                for i in min_idx..=max_idx {
                    if let Some(row) = self.items.get(i) {
                        self.selected_ids.insert(row.id.clone());
                    }
                }
            }
        }
    }

    pub(crate) fn total_count(&self) -> usize {
        tasks::count(
            self.conn,
            &ListFilter {
                status: None,
                tags: vec![],
                query: if self.search_query.is_empty() {
                    None
                } else {
                    Some(self.search_query.clone())
                },
                review_stale: false,
            },
        )
        .unwrap_or(0)
    }

    pub(crate) fn context_count(&self, v: View) -> usize {
        match v {
            View::Archived => tasks::list_archived(self.conn)
                .map(|t| t.len())
                .unwrap_or(0),
            View::Tags => tags::list_tags(self.conn).map(|t| t.len()).unwrap_or(0),
            View::Today => self.today_count,
            View::Tomorrow => self.tomorrow_count,
            _ => match v.status() {
                Some(s) => tasks::count(
                    self.conn,
                    &ListFilter {
                        status: Some(s.parse::<task::Status>().unwrap_or(task::Status::Inbox)),
                        tags: vec![],
                        query: if self.search_query.is_empty() {
                            None
                        } else {
                            Some(self.search_query.clone())
                        },
                        review_stale: false,
                    },
                )
                .unwrap_or(0),
                None => 0,
            },
        }
    }

    /// 今日/明日视图：所有未完成、未归档任务，其到期/循环发生时间落在指定
    /// 本地日（含逾期，`include_overdue=true` 时）。明日视图等价于"假设明天
    /// 已到来"，因此也带上逾期（今天及更早到期但未完成）的任务。返回
    /// (任务, 展示用到期时间)。
    fn day_tasks(&self, offset_days: i64, include_overdue: bool) -> Vec<(task::Task, i64)> {
        let (start, end) = crate::time::local_day_bounds(offset_days);
        let mut tags = vec![];
        if let Some(ref tf) = self.tag_filter {
            tags.push(tf.clone());
        }
        tasks::list(
            self.conn,
            &ListFilter {
                status: None,
                tags,
                query: if self.search_query.is_empty() {
                    None
                } else {
                    Some(self.search_query.clone())
                },
                review_stale: false,
            },
        )
        .unwrap_or_default()
        .into_iter()
        .filter(|t| t.status != task::Status::Done)
        .filter(|t| {
            crate::commands::occurs_in_window(t, start, end)
                || (include_overdue
                    && t.rrule.is_none()
                    && t.due_at.or(t.scheduled_start_at).is_some_and(|d| d < start))
        })
        .map(|t| {
            let due = crate::commands::window_due(&t, start, end)
                .or_else(|| crate::commands::effective_due(&t))
                .unwrap_or(i64::MAX);
            (t, due)
        })
        .collect()
    }

    pub(crate) fn refresh(&mut self) -> Result<()> {
        self.items.clear();
        // 缓存今日/明日数量：切视图/操作时才计算，供引导栏徽标使用。
        self.today_count = self.day_tasks(0, true).len();
        self.tomorrow_count = self.day_tasks(1, true).len();
        match self.view {
            View::Today | View::Tomorrow => {
                let (offset, overdue) = if self.view == View::Today {
                    (0, true)
                } else {
                    (1, true)
                };
                let mut ts = self.day_tasks(offset, overdue);
                ts.sort_by_key(|(_, due)| *due);
                for (t, due) in ts {
                    let mut row = row_from(&t, 0, self.conn)?;
                    row.due = Some(due);
                    self.items.push(row);
                }
            }
            View::Archived => {
                for t in tasks::list_archived(self.conn)? {
                    self.items.push(row_from(&t, 0, self.conn)?);
                }
            }
            View::Tags => {
                if let Ok(all_tags) = tags::list_tags(self.conn) {
                    for t in all_tags {
                        self.items.push(Row {
                            id: t.id.to_string(),
                            title: format!("@{}", t.name),
                            status: t.category,
                            due: None,
                            tags: vec![],
                            indent: 0,
                            done: None,
                            total: None,
                            archive_reason: None,
                        });
                    }
                }
            }
            View::Review => {
                let ts = tasks::list(
                    self.conn,
                    &ListFilter {
                        status: None,
                        tags: vec![],
                        query: if self.search_query.is_empty() {
                            None
                        } else {
                            Some(self.search_query.clone())
                        },
                        review_stale: true,
                    },
                )?;
                for t in ts {
                    self.items.push(row_from(&t, 0, self.conn)?);
                }
            }
            _ => {
                if let Some(s) = self.view.status() {
                    let mut tags = vec![];
                    if let Some(ref tf) = self.tag_filter {
                        tags.push(tf.clone());
                    }

                    let ts = tasks::list(
                        self.conn,
                        &ListFilter {
                            status: Some(s.parse::<task::Status>().unwrap_or(task::Status::Inbox)),
                            tags,
                            query: if self.search_query.is_empty() {
                                None
                            } else {
                                Some(self.search_query.clone())
                            },
                            review_stale: false,
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

    pub(crate) fn load_detail(&mut self) {
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

    pub(crate) fn set_view(&mut self, v: View) {
        self.view = v;
        self.selected = 0;
        self.status_message.clear();
        if let Err(e) = self.refresh() {
            self.status_message = format!("err: {}", e);
        }
        self.load_detail();
    }

    pub(crate) fn move_sel(&mut self, delta: isize) {
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
        if self.mode == Mode::Visual {
            self.update_visual_selection();
        }
        self.load_detail();
    }

    pub(crate) fn next_view(&mut self, delta: isize) {
        // 今日/明日视图不参与方向键循环，只通过 Shift+J / Shift+K 快捷键调出。
        let views = [
            View::Inbox,
            View::Next,
            View::Waiting,
            View::Scheduled,
            View::Someday,
            View::Reference,
            View::Done,
            View::Review,
            View::Tags,
            View::Archived,
        ];
        // 若当前停在日视图，映射到相邻主视图，避免方向键跳到随机位置。
        let anchor = match self.view {
            View::Today => View::Inbox,
            View::Tomorrow => View::Next,
            v => v,
        };
        let idx = views.iter().position(|v| *v == anchor).unwrap_or(0) as isize;
        let mut next_idx = idx + delta;
        if next_idx < 0 {
            next_idx = views.len() as isize - 1;
        } else if next_idx >= views.len() as isize {
            next_idx = 0;
        }
        self.set_view(views[next_idx as usize]);
    }

    pub(crate) fn needs_time(t: &Task) -> bool {
        t.due_at.is_none() && t.scheduled_start_at.is_none()
    }

    pub(crate) fn act_next(&mut self, row: Row) -> Result<()> {
        let t = tasks::transition(self.conn, &row.id, task::Status::Next)?;
        self.status_message = crate::tr!(self.lang, "{} -> 下一步", "{} -> next", &t.id[..8]);
        if Self::needs_time(&t) {
            self.set_mode(Mode::PlanningTime);
            self.input.clear();
            self.status_message = crate::tr!(
                self.lang,
                "{} 预计开始/截止? (空/Esc 跳过)",
                "{} start/due? (empty/Esc to skip)",
                &t.id[..8]
            );
        } else {
            self.refresh()?;
            self.load_detail();
        }
        Ok(())
    }

    pub(crate) fn act_on_selected(&mut self, to: task::Status) -> Result<()> {
        let mut ids = vec![];
        if self.mode == Mode::Visual && !self.selected_ids.is_empty() {
            ids.extend(self.selected_ids.iter().cloned());
        } else if let Some(row) = self.items.get(self.selected).cloned() {
            ids.push(row.id);
        }

        if ids.is_empty() {
            return Ok(());
        }

        if ids.len() == 1 {
            let id = &ids[0];
            if let Ok(task) = tasks::get(self.conn, id) {
                if task.status == to {
                    self.status_message = crate::tr!(
                        self.lang,
                        "已是 {} 状态",
                        "already {}",
                        crate::tui::status_cn(self.lang, task.status)
                    );
                    return Ok(());
                }
                if to == task::Status::Next {
                    let row = crate::tui::row_from(&task, 0, self.conn)?;
                    return self.act_next(row);
                }
                // 如果当前变动状态的任务正处于 Pomodoro 专注中，且新状态为 Done/Waiting，终止番茄钟
                if let Ok(pomo) = crate::repo::pomodoro::get_state() {
                    if pomo.task_id.as_deref() == Some(id)
                        && matches!(
                            to,
                            task::Status::Done | task::Status::Waiting | task::Status::Someday
                        )
                    {
                        let _ = crate::commands::pomo::stop();
                    }
                }
                let t = tasks::transition(self.conn, id, to)?;
                self.status_message = format!(
                    "{} -> {}",
                    &t.id[..8],
                    crate::tui::status_cn(self.lang, t.status)
                );
            }
        } else {
            let mut count = 0;
            for id in &ids {
                if let Ok(task) = tasks::get(self.conn, id) {
                    if task.status != to && tasks::transition(self.conn, id, to).is_ok() {
                        count += 1;
                        if let Ok(pomo) = crate::repo::pomodoro::get_state() {
                            if pomo.task_id.as_deref() == Some(id)
                                && matches!(
                                    to,
                                    task::Status::Done
                                        | task::Status::Waiting
                                        | task::Status::Someday
                                )
                            {
                                let _ = crate::commands::pomo::stop();
                            }
                        }
                    }
                }
            }
            self.status_message =
                crate::tr!(self.lang, "批量 {} {} 项", "Bulk {} {} items", to, count);
        }

        if self.mode == Mode::Visual {
            self.set_mode(Mode::Normal);
            self.selected_ids.clear();
            self.visual_start_idx = None;
        }

        self.refresh()?;
        self.load_detail();
        Ok(())
    }
}
