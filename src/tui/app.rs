use anyhow::Result;
use ratatui::widgets::ListState;
use rusqlite::Connection;

use super::row_from_tags;
use crate::model::event::TaskEvent;
use crate::model::tag::Tag;
use crate::model::task::{self, Task};
use crate::repo::tags;
use crate::repo::tasks::{self, ListFilter};

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

/// 有状态的 7 个主视图（Inbox..Done），用于按状态统计计数。
const STATUS_VIEWS: [View; 7] = [
    View::Inbox,
    View::Next,
    View::Waiting,
    View::Scheduled,
    View::Someday,
    View::Reference,
    View::Done,
];

/// 今日/明日列表元素：(任务, 展示用到期时间)。
type DayList = Vec<(task::Task, i64)>;

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

    /// 固定索引，用于 `App.counts` 计数缓存数组。
    pub(crate) fn idx(self) -> usize {
        match self {
            View::Inbox => 0,
            View::Today => 1,
            View::Tomorrow => 2,
            View::Next => 3,
            View::Waiting => 4,
            View::Scheduled => 5,
            View::Someday => 6,
            View::Reference => 7,
            View::Done => 8,
            View::Review => 9,
            View::Archived => 10,
            View::Tags => 11,
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Mode {
    Normal,
    EditingTitle,
    Capturing,
    Tagging,
    WaitingWho,
    WaitingWhen,
    Search,
    ChecklistAdding,
    Visual,
    FilteringTag,
    /// 归档前确认：收集待归档的 id，等待 y/Enter 确认或 n/Esc 取消。
    ConfirmArchive,
    /// 永久删除确认：等待 y/Enter 确认或 n/Esc 取消。
    ConfirmPurge,
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
    /// 循环任务今日是否已打卡（存在今日的 habit_completed 事件）。
    pub(crate) checked_in_today: bool,
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
    /// 组织/编辑模式正在编辑的任务 id。
    pub(crate) organizing_id: Option<String>,
    pub(crate) status_message: String,
    pub(crate) lang: crate::i18n::Lang,
    pub(crate) show_help: bool,
    pub(crate) show_syntax: bool,
    pub(crate) show_shortcut_bar: bool,
    pub(crate) help_scroll: usize,
    pub(crate) should_quit: bool,
    pub(crate) search_query: String,
    pub(crate) tag_filter: Option<String>,
    pub(crate) visual_start_idx: Option<usize>,
    pub(crate) selected_ids: std::collections::HashSet<String>,
    pub(crate) is_reviewing: bool,
    pub(crate) review_step: u8,
    pub(crate) needs_clear: bool,
    pub(crate) pending_archive_ids: Vec<String>,
    pub(crate) pending_purge_ids: Vec<String>,
    pub(crate) hide_pomo_banner: bool,
    pub(crate) theme: crate::tui::theme::Theme,
    pub(crate) popup: Option<Popup>,
    pub(crate) last_tick_ms: i64,
    pub(crate) notified_events: std::collections::HashSet<String>,
    /// 各视图计数缓存：`refresh` 时一次性算好，渲染帧内零 DB 查询。
    pub(crate) counts: [usize; 12],
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
            organizing_id: None,
            status_message: crate::tr!(lang, "按 '?' 或 F1 查看帮助", "Press '?' or F1 for help")
                .to_string(),
            lang,
            show_help: false,
            show_syntax: false,
            show_shortcut_bar: true,
            help_scroll: 0,
            should_quit: false,
            search_query: String::new(),
            tag_filter: None,
            visual_start_idx: None,
            selected_ids: std::collections::HashSet::new(),
            is_reviewing: false,
            review_step: 0,
            needs_clear: false,
            pending_archive_ids: Vec::new(),
            pending_purge_ids: Vec::new(),
            hide_pomo_banner: false,
            theme,
            popup: None,
            last_tick_ms: 0,
            notified_events: std::collections::HashSet::new(),
            counts: [0; 12],
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

        // 每日心智维护摘要（合并成一条，同一天至多一次）。
        let _ = crate::commands::notify::check(self.conn);

        // 只拉取即将到期（±1h 窗口）的任务，不再全表扫描。
        if let Ok(rows) = tasks::due_in_range(self.conn, (now - 60) * 1000, (now + 3600) * 1000) {
            for (id, title, due) in rows {
                let Some(due) = due else { continue };
                let diff = due / 1000 - now;

                // 1 hour
                if diff > 3540 && diff <= 3600 {
                    let key = format!("{id}-{due}-1h");
                    if !self.notified_events.contains(&key) {
                        self.notified_events.insert(key);
                        let _ = notify_rust::Notification::new()
                            .summary("任务即将在1小时后开始")
                            .body(&title)
                            .appname("GTD")
                            .show();
                    }
                }

                // 10 mins
                if diff > 540 && diff <= 600 {
                    let key = format!("{id}-{due}-10m");
                    if !self.notified_events.contains(&key) {
                        self.notified_events.insert(key);
                        let _ = notify_rust::Notification::new()
                            .summary("任务即将在10分钟后开始")
                            .body(&title)
                            .appname("GTD")
                            .show();
                    }
                }

                // Due now
                if diff <= 0 && diff > -60 {
                    let key = format!("{id}-{due}-now");
                    if !self.notified_events.contains(&key) {
                        self.notified_events.insert(key);
                        let _ = notify_rust::Notification::new()
                            .summary("任务现在开始!")
                            .body(&title)
                            .appname("GTD")
                            .show();
                        self.popup = Some(Popup::TaskDueNow(id.clone(), title));
                        self.needs_clear = true; // force redraw to show popup
                    }
                }
            }
        }

        // 防止 key 无限增长（长会话中到期任务不断累积）。
        if self.notified_events.len() > 1024 {
            self.notified_events.clear();
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
        STATUS_VIEWS.iter().map(|v| self.counts[v.idx()]).sum()
    }

    pub(crate) fn context_count(&self, v: View) -> usize {
        self.counts[v.idx()]
    }

    /// 一次全量加载同时算出今日/明日列表：每个任务只读一次、每个循环规则只
    /// 展开一次，供今日/明日视图与侧栏徽标复用，避免重复查询与重复 RRULE 展开。
    /// `checked_today` 是今日已打卡的循环任务 id 集合（由 `refresh` 一次性查询）。
    fn day_lists(&self, checked_today: &std::collections::HashSet<String>) -> (DayList, DayList) {
        let (t0s, t0e) = crate::time::local_day_bounds(0);
        let (t1s, t1e) = crate::time::local_day_bounds(1);
        let mut tags = vec![];
        if let Some(ref tf) = self.tag_filter {
            tags.push(tf.clone());
        }
        let query = if self.search_query.is_empty() {
            None
        } else {
            Some(self.search_query.clone())
        };
        let tasks = tasks::list(
            self.conn,
            &ListFilter {
                status: None,
                tags,
                query,
                review_stale: false,
            },
        )
        .unwrap_or_default();

        let mut today = Vec::new();
        let mut tomorrow = Vec::new();
        let now = crate::time::now_ms();
        for t in tasks {
            if t.status == task::Status::Done {
                continue;
            }
            let anchor = t.scheduled_start_at.or(t.due_at);
            let occs = match &t.rrule {
                Some(rr) => anchor.and_then(|a| crate::time::rrule_occurrences(rr, a, 366).ok()),
                None => None,
            };
            // 今日已打卡的循环任务：保留在今日视图，展示下一次执行时间。
            if t.rrule.is_some() && checked_today.contains(&t.id) {
                if let Some(first) = occs
                    .as_ref()
                    .and_then(|o| o.iter().find(|m| **m >= now).copied())
                {
                    today.push((t.clone(), first));
                }
            }
            // 非循环任务：今日/明日命中 ⇔ 锚点时间落在该日结束之前（含逾期结转）。
            let (d0, d1) = match &occs {
                Some(occs) => (
                    occs.iter().find(|m| **m >= t0s && **m <= t0e).copied(),
                    occs.iter().find(|m| **m >= t1s && **m <= t1e).copied(),
                ),
                None => (anchor.filter(|d| *d <= t0e), anchor.filter(|d| *d <= t1e)),
            };
            match (d0, d1) {
                (Some(a), Some(b)) => {
                    today.push((t.clone(), a));
                    tomorrow.push((t, b));
                }
                (Some(a), None) => today.push((t, a)),
                (None, Some(b)) => tomorrow.push((t, b)),
                (None, None) => {}
            }
        }
        (today, tomorrow)
    }

    pub(crate) fn refresh(&mut self) -> Result<()> {
        self.items.clear();
        let today_start = crate::time::local_day_bounds(0).0;
        let checked_today: std::collections::HashSet<String> =
            tasks::checked_in_today(self.conn, today_start)
                .unwrap_or_default()
                .into_iter()
                .collect();
        let (today, tomorrow) = self.day_lists(&checked_today);
        self.counts[View::Today.idx()] = today.len();
        self.counts[View::Tomorrow.idx()] = tomorrow.len();
        self.refresh_counts()?;

        // 标签视图单独构建行（没有任务主体）。
        if self.view == View::Tags {
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
                        checked_in_today: false,
                    });
                }
            }
            if self.selected >= self.items.len() {
                self.selected = self.items.len().saturating_sub(1);
            }
            return Ok(());
        }

        // 加载当前视图的任务（今日/明日带展示用到期时间）。
        let tasks: Vec<(task::Task, Option<i64>)> = match self.view {
            View::Today | View::Tomorrow => {
                let mut ts = if self.view == View::Today {
                    today
                } else {
                    tomorrow
                };
                ts.sort_by_key(|(_, due)| *due);
                ts.into_iter().map(|(t, d)| (t, Some(d))).collect()
            }
            View::Archived => tasks::list_archived(self.conn)?
                .into_iter()
                .map(|t| (t, None))
                .collect(),
            View::Review => tasks::list(
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
            )?
            .into_iter()
            .map(|t| (t, None))
            .collect(),
            _ => {
                if let Some(s) = self.view.status() {
                    let mut tag_f = vec![];
                    if let Some(ref tf) = self.tag_filter {
                        tag_f.push(tf.clone());
                    }
                    tasks::list(
                        self.conn,
                        &ListFilter {
                            status: Some(s.parse::<task::Status>().unwrap_or(task::Status::Inbox)),
                            tags: tag_f,
                            query: if self.search_query.is_empty() {
                                None
                            } else {
                                Some(self.search_query.clone())
                            },
                            review_stale: false,
                        },
                    )?
                    .into_iter()
                    .map(|t| (t, None))
                    .collect()
                } else {
                    Vec::new()
                }
            }
        };

        // 单次查询取所有行的标签，避免逐行 `get_task_tags`。
        let ids: Vec<&str> = tasks.iter().map(|(t, _)| t.id.as_str()).collect();
        let tag_map = tags::get_tags_for_tasks(self.conn, &ids)?;
        for (t, due) in tasks {
            let mut row = row_from_tags(&t, 0, tag_map.get(&t.id).cloned().unwrap_or_default());
            if let Some(d) = due {
                row.due = Some(d);
            }
            row.checked_in_today = checked_today.contains(&t.id);
            self.items.push(row);
        }

        if self.selected >= self.items.len() {
            self.selected = self.items.len().saturating_sub(1);
        }
        Ok(())
    }

    /// 一次算好所有视图计数（除今日/明日已在 `refresh` 中赋值），渲染时零查询。
    fn refresh_counts(&mut self) -> Result<()> {
        self.counts[View::Review.idx()] = 0;
        self.counts[View::Archived.idx()] = tasks::count_archived(self.conn)?;
        self.counts[View::Tags.idx()] = tags::count_tags(self.conn)?;
        let query = if self.search_query.is_empty() {
            None
        } else {
            Some(self.search_query.clone())
        };
        let mut f = ListFilter {
            status: None,
            tags: vec![],
            query,
            review_stale: false,
        };
        for v in STATUS_VIEWS {
            let s = v.status().expect("status view");
            f.status = Some(s.parse::<task::Status>().unwrap_or(task::Status::Inbox));
            self.counts[v.idx()] = tasks::count(self.conn, &f)?;
        }
        Ok(())
    }

    /// 刷新列表并重新加载详情（编辑/操作后的统一收尾）。
    pub(crate) fn reload(&mut self) -> Result<()> {
        self.refresh()?;
        self.load_detail();
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
        // 方向键环与侧栏显示顺序完全一致（今日/明日也参与循环）。
        let views = [
            View::Today,
            View::Tomorrow,
            View::Inbox,
            View::Next,
            View::Waiting,
            View::Scheduled,
            View::Someday,
            View::Reference,
            View::Done,
            View::Archived,
            View::Tags,
            View::Review,
        ];
        let idx = views.iter().position(|v| *v == self.view).unwrap_or(0) as isize;
        let next_idx = (idx + delta).rem_euclid(views.len() as isize);
        self.set_view(views[next_idx as usize]);
    }

    /// 回车进入组织/编辑模式：与 capture 同一个一句话编辑器，预填当前任务内容。
    pub(crate) fn open_organize(&mut self) -> Result<()> {
        if self.mode == Mode::Visual {
            self.set_mode(Mode::Normal);
            self.selected_ids.clear();
            self.visual_start_idx = None;
            self.status_message = crate::tr!(
                self.lang,
                "可视模式不支持编辑",
                "editing unavailable in visual mode"
            )
            .into();
            return Ok(());
        }
        if matches!(self.view, View::Tags | View::Archived) {
            return Ok(());
        }
        let Some(row) = self.items.get(self.selected).cloned() else {
            return Ok(());
        };
        let Ok(task) = tasks::get(self.conn, &row.id) else {
            return Ok(());
        };
        self.organizing_id = Some(task.id.clone());
        self.input = self.task_to_quick_add(&task);
        self.set_mode(Mode::Capturing);
        self.status_message = crate::tr!(
            self.lang,
            "组织: 编辑 @标签 ~时间 *周期 (空/Esc 跳过)",
            "organize: edit @tags ~time *rrule (empty/Esc to skip)"
        )
        .into();
        Ok(())
    }

    /// 把任务序列化成 quick-add 一句话（标题 @标签 ~时间 *周期），可解析回原字段。
    pub(crate) fn task_to_quick_add(&self, task: &Task) -> String {
        let row = crate::tui::row_from(task, 0, self.conn)
            .unwrap_or_else(|_| crate::tui::row_from_tags(task, 0, Vec::new()));
        let mut s = task.title.clone();
        for tag in &row.tags {
            s.push(' ');
            s.push('@');
            s.push_str(tag);
        }
        if let Some(start) = task.scheduled_start_at {
            s.push_str(" ~");
            s.push_str(&crate::time::format_quick_time(start));
        }
        if let Some(rr) = &task.rrule {
            s.push(' ');
            s.push('*');
            s.push_str(rr);
        }
        s
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
                // 习惯打卡一天一次：今日已打过卡则只提示，不重复推进排程。
                let already_checked_in = to == task::Status::Done
                    && task.rrule.is_some()
                    && crate::repo::tasks::checked_in_today(
                        self.conn,
                        crate::time::local_day_bounds(0).0,
                    )
                    .unwrap_or_default()
                    .iter()
                    .any(|tid| tid == id);
                if already_checked_in {
                    self.status_message = crate::tr!(
                        self.lang,
                        "{} 今日已打卡",
                        "{} already checked in today",
                        &id[..8]
                    );
                } else {
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
            }
        } else {
            let mut count = 0;
            for id in &ids {
                if let Ok(task) = tasks::get(self.conn, id) {
                    if task.status != to
                        && task.status != task::Status::Scheduled
                        && tasks::transition(self.conn, id, to).is_ok()
                    {
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

        if to == task::Status::Done {
            let _ = crate::commands::notify::completed_feedback(self.conn);
        }

        self.refresh()?;
        self.load_detail();
        Ok(())
    }
}
