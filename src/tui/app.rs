
use anyhow::Result;
use ratatui::widgets::ListState;
use rusqlite::Connection;

use crate::model::event::TaskEvent;
use crate::model::tag::Tag;
use crate::model::task::{self, Task};
use crate::repo::tasks::{self, ListFilter};
use crate::repo::tags;
use super::row_from;

use super::calendar;



pub(crate) fn visual_len(s: &str) -> usize {
    s.chars().map(|c| {
        if c.is_ascii() || (c >= '\u{E000}' && c <= '\u{F8FF}') {
            1
        } else {
            2
        }
    }).sum()
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
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum View {
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
    pub(crate) fn label(self) -> &'static str {
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
            View::Projects | View::Review => None,
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
    /// 计划钩子第 1 步：询问归属项目。
    PlanningProject,
    /// 计划钩子第 2 步：询问预计时间。
    PlanningTime,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pane {
    Left,
    Center,
    Right,
}

#[derive(Clone)]
pub(crate) struct Row {    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) status: String,
    pub(crate) due: Option<i64>,
    pub(crate) tags: Vec<String>,
    pub(crate) indent: usize,
}

pub(crate) struct DetailData {    pub(crate) task: Task,
    pub(crate) tags: Vec<Tag>,
    pub(crate) events: Vec<TaskEvent>,
}

pub(crate) struct App<'a> {    pub(crate) conn: &'a Connection,
    pub(crate) view: View,
    pub(crate) items: Vec<Row>,
    pub(crate) selected: usize,
    pub(crate) list_state: ListState,
    pub(crate) detail: Option<DetailData>,
    pub(crate) mode: Mode,
    pub(crate) pane: Pane,
    pub(crate) input: String,
    pub(crate) status_message: String,
    pub(crate) show_help: bool,
    pub(crate) should_quit: bool,
    pub(crate) calendar: calendar::CalendarState,
    pub(crate) sched_dates: Option<(chrono::NaiveDate, chrono::NaiveDate)>,
    pub(crate) search_query: String,
}

impl<'a> App<'a> {
    pub(crate) fn new(conn: &'a Connection) -> Result<Self> {
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
            calendar: calendar::CalendarState::new(),
            sched_dates: None,
            search_query: String::new(),
        };
        app.refresh()?;
        app.load_detail();
        Ok(app)
    }

    pub(crate) fn total_count(&self) -> usize {
        tasks::count(
            self.conn,
            &ListFilter {
                status: None,
                project: None,
                tags: vec![],
                query: if self.search_query.is_empty() { None } else { Some(self.search_query.clone()) },
            },
        )
        .unwrap_or(0)
    }

    pub(crate) fn context_count(&self, v: View) -> usize {
        match v.status() {
            Some(s) => tasks::count(
                self.conn,
                &ListFilter {
                    status: Some(s.parse::<task::Status>().unwrap_or(task::Status::Inbox)),
                    project: None,
                    tags: vec![],
                    query: if self.search_query.is_empty() { None } else { Some(self.search_query.clone()) },
                },
            )
            .unwrap_or(0),
            None => 0,
        }
    }

    pub(crate) fn refresh(&mut self) -> Result<()> {
        self.items.clear();
        match self.view {
            View::Projects => {
                let projects = tasks::list(
                    self.conn,
                    &ListFilter {
                        status: None,
                        project: None,
                        tags: vec![],
                        query: if self.search_query.is_empty() { None } else { Some(self.search_query.clone()) },
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
                            tags: vec![],
                            query: if self.search_query.is_empty() { None } else { Some(self.search_query.clone()) },
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
                            status: Some(s.parse::<task::Status>().unwrap_or(task::Status::Inbox)),
                            project: None,
                            tags: vec![],
                            query: if self.search_query.is_empty() { None } else { Some(self.search_query.clone()) },
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
        self.load_detail();
    }



    pub(crate) fn next_view(&mut self, delta: isize) {
        let views = [
            View::Inbox,
            View::Next,
            View::Waiting,
            View::Scheduled,
            View::Someday,
            View::Reference,
            View::Done,
            View::Projects,
            View::Review,
        ];
        let idx = views.iter().position(|v| *v == self.view).unwrap_or(0) as isize;
        let mut next_idx = idx + delta;
        if next_idx < 0 {
            next_idx = views.len() as isize - 1;
        } else if next_idx >= views.len() as isize {
            next_idx = 0;
        }
        self.set_view(views[next_idx as usize]);
    }

    pub(crate) fn needs_planning(t: &Task) -> bool {
        let missing_project = t.parent_id.is_none();
        let missing_time = t.due_at.is_none() && t.scheduled_start_at.is_none();
        missing_project || missing_time
    }

    pub(crate) fn needs_time(t: &Task) -> bool {
        t.due_at.is_none() && t.scheduled_start_at.is_none()
    }

    pub(crate) fn planning_hint(t: &Task) -> String {
        let mut missing = Vec::new();
        if t.parent_id.is_none() {
            missing.push("项目");
        }
        if Self::needs_time(t) {
            missing.push("时间");
        }
        if missing.is_empty() {
            String::new()
        } else {
            format!("建议补充{} (t 加项目, c 排期)", missing.join("/"))
        }
    }

    pub(crate) fn act_next(&mut self, row: Row) -> Result<()> {
        let t = tasks::transition(self.conn, &row.id, task::Status::Next)?;
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

    pub(crate) fn act_on_selected(&mut self, to: task::Status) -> Result<()> {
        if let Some(row) = self.items.get(self.selected).cloned() {
            if row.status == to.to_string() {
                self.status_message = format!("already {}", to);
                return Ok(());
            }
            if to == task::Status::Next {
                return self.act_next(row);
            }
            let t = tasks::transition(self.conn, &row.id, to)?;
            self.status_message = format!("{} -> {}", &t.id[..8], t.status);
            self.refresh()?;
            self.load_detail();
        }
        Ok(())
    }

}
