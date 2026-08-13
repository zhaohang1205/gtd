pub mod app;
pub mod calendar;
pub mod handlers;
pub mod keys;
pub mod render;
pub mod theme;
pub mod ui;

pub(crate) use app::{App, Pane, View};
pub(crate) use handlers::AppHandlers;
pub(crate) use render::AppRender;

use crate::model::task::{self, Task};
use crate::repo::tags;
use anyhow::Result;
use app::Row;
use crossterm::event::{self, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::{backend::CrosstermBackend, Terminal};
use rusqlite::Connection;
use std::io::{self, Stdout};
use std::time::Duration;

/// 状态的中文含义，用于引导栏的“状态地图”。按当前语言返回。
pub(crate) fn status_cn(lang: crate::i18n::Lang, s: task::Status) -> &'static str {
    match s {
        task::Status::Inbox => crate::tr!(lang, "收件箱", "Inbox"),
        task::Status::Next => crate::tr!(lang, "下一步", "Next"),
        task::Status::Waiting => crate::tr!(lang, "等待中", "Waiting"),
        task::Status::Scheduled => crate::tr!(lang, "已排程", "Scheduled"),
        task::Status::Someday => crate::tr!(lang, "将来/也许", "Someday"),
        task::Status::Reference => crate::tr!(lang, "参考资料", "Reference"),
        task::Status::Done => crate::tr!(lang, "已完成", "Done"),
    }
}

/// 引导栏里各视图的中文/英文名（含日视图与归档箱等无状态视图）。
pub(crate) fn view_label(lang: crate::i18n::Lang, v: View) -> &'static str {
    match v {
        View::Inbox => crate::tr!(lang, "收件箱", "Inbox"),
        View::Today => crate::tr!(lang, "今日", "Today"),
        View::Tomorrow => crate::tr!(lang, "明日", "Tomorrow"),
        View::Next => crate::tr!(lang, "下一步", "Next"),
        View::Waiting => crate::tr!(lang, "等待中", "Waiting"),
        View::Scheduled => crate::tr!(lang, "已排程", "Scheduled"),
        View::Someday => crate::tr!(lang, "将来/也许", "Someday"),
        View::Reference => crate::tr!(lang, "参考资料", "Reference"),
        View::Done => crate::tr!(lang, "已完成", "Done"),
        View::Review => crate::tr!(lang, "周回顾", "Review"),
        View::Archived => crate::tr!(lang, "归档箱", "Archived"),
        View::Tags => crate::tr!(lang, "标签库", "Tags"),
    }
}

pub(crate) fn row_from(t: &Task, indent: usize, conn: &Connection) -> Result<Row> {
    let tags = tags::get_task_tags(conn, &t.id)?
        .iter()
        .map(|x| x.name.clone())
        .collect();
    Ok(row_from_tags(t, indent, tags))
}

/// 用已取好的标签名构建行，避免每行一次 DB 查询。
pub(crate) fn row_from_tags(t: &Task, indent: usize, tags: Vec<String>) -> Row {
    // 完成进度：行动按检查单完成数。
    let (done, total) = if !t.checklist.is_empty() {
        let total = t.checklist.len();
        let done = t.checklist.iter().filter(|i| i.done).count();
        (Some(done), Some(total))
    } else {
        (None, None)
    };

    Row {
        id: t.id.clone(),
        title: t.title.clone(),
        status: t.status.to_string(),
        due: if t.archived_at.is_some() {
            t.archived_at
        } else if t.status == task::Status::Done {
            t.completed_at.or(t.due_at).or(t.scheduled_start_at)
        } else {
            // 循环任务用 effective_due：错过 slot 即显示其时间（逾期），
            // 已打卡后锚点已推进为下次执行时间。
            crate::commands::effective_due(t)
        },
        tags,
        indent,
        done,
        total,
        archive_reason: t.archive_reason.clone(),
        checked_in_today: false,
    }
}

/// 启动交互式 TUI。
pub fn run(conn: &Connection) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, conn);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    result
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<Stdout>>, conn: &Connection) -> Result<()> {
    let mut app = App::new(conn)?;
    loop {
        if app.needs_clear {
            terminal.clear()?;
            app.needs_clear = false;
        }
        app.check_notifications();
        terminal.draw(|f| app.render(f))?;
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind == KeyEventKind::Release {
                        continue;
                    }
                    app.handle_key(key)?;
                }
                Event::Mouse(m) => {
                    let left_width = terminal.size()?.width * 22 / 100;
                    let is_left_panel = m.column < left_width;
                    match m.kind {
                        crossterm::event::MouseEventKind::ScrollDown => {
                            if is_left_panel && app.show_help {
                                app.help_scroll = app.help_scroll.saturating_add(1);
                            } else {
                                app.move_sel(1);
                            }
                        }
                        crossterm::event::MouseEventKind::ScrollUp => {
                            if is_left_panel && app.show_help {
                                app.help_scroll = app.help_scroll.saturating_sub(1);
                            } else {
                                app.move_sel(-1);
                            }
                        }
                        crossterm::event::MouseEventKind::Down(
                            crossterm::event::MouseButton::Left,
                        ) => {
                            if m.column > terminal.size()?.width / 2 {
                                app.pane = Pane::Right;
                            } else if is_left_panel {
                                app.pane = Pane::Left;
                            } else {
                                app.pane = Pane::Center;
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
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
    use crate::repo::tasks::ListFilter;
    use crate::repo::tasks::{self, CaptureInput};
    use crate::tui::app::Mode;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::backend::TestBackend;
    use std::io::Write;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty())
    }
    fn kc(k: KeyCode) -> KeyEvent {
        KeyEvent::new(k, KeyModifiers::empty())
    }

    fn seed(conn: &Connection) {
        let mk = |title: &str, status: task::Status, tags: &[&str]| {
            tasks::create_capture(
                conn,
                &CaptureInput {
                    title: title.into(),
                    status,
                    due_at: None,
                    tag_names: tags.iter().map(|s| s.to_string()).collect(),
                    ..Default::default()
                },
            )
            .unwrap();
        };
        mk("Write homepage copy", task::Status::Inbox, &["work", "p1"]);
        mk("Buy groceries", task::Status::Inbox, &["home", "errands"]);
        mk("Read Rust book", task::Status::Next, &["learning"]);
        mk("Pay taxes", task::Status::Waiting, &["work", "p2"]);
        mk("Plan vacation", task::Status::Someday, &["home"]);
        mk("Finish report", task::Status::Done, &[]);
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

    /// 去掉所有空格，规避无头快照里 CJK 字符被逐字加空格的渲染产物，
    /// 便于对中文文本做 contains 断言（真实终端无此问题）。
    fn norm(s: &str) -> String {
        s.chars().filter(|c| *c != ' ').collect()
    }

    #[test]
    fn drive_tui() {
        crate::repo::pomodoro::set_pomo_idle_for_tests();
        let mut conn = Connection::open(":memory:").unwrap();
        migrate::run(&mut conn).unwrap();
        seed(&conn);
        let mut app = App::new(&conn).unwrap();
        let mut term = Terminal::new(TestBackend::new(110, 30)).unwrap();
        let mut out = std::fs::File::create("/tmp/gtp_tui_frames.txt").unwrap();
        let frame = |label: &str,
                     term: &mut Terminal<TestBackend>,
                     app: &mut App,
                     out: &mut std::fs::File|
         -> String {
            term.clear().unwrap();
            term.draw(|f| app.render(f)).unwrap();
            let s = snap(term);
            writeln!(out, "===== {label} =====").unwrap();
            out.write_all(s.as_bytes()).unwrap();
            s
        };

        // 1) 三栏布局：引导栏 + 列表 + 详情
        let s = norm(&frame("1-initial-inbox", &mut term, &mut app, &mut out));
        assert!(s.contains("Active"), "引导栏应显示分组");
        assert!(s.contains("收件箱"), "引导栏含中文含义");
        assert!(s.contains("任务·收件箱"), "中栏列表标题");
        assert!(s.contains("任务详情"), "右侧详情栏");
        assert!(s.contains("Buygroceries"), "inbox 列出已灌入的任务");
        assert!(
            s.contains("等待中") && s.contains("将来/也许"),
            "上下文分组已列出"
        );

        // 2) vim 导航：下、上
        app.handle_key(key('j')).unwrap();
        frame("2-nav-down", &mut term, &mut app, &mut out);
        app.handle_key(key('k')).unwrap();
        frame("3-nav-up", &mut term, &mut app, &mut out);

        // 3) h/l 把焦点在 Left, Center, Right 之间切换
        app.pane = Pane::Center;
        app.handle_key(key('l')).unwrap();
        frame("4-pane-right", &mut term, &mut app, &mut out);
        assert!(app.pane == Pane::Right, "l 把焦点移到右栏");
        app.handle_key(key('h')).unwrap();
        frame("5-pane-center", &mut term, &mut app, &mut out);
        assert!(app.pane == Pane::Center, "h 把焦点移回中栏");
        app.handle_key(key('h')).unwrap();
        assert!(app.pane == Pane::Left, "h 把焦点移到左栏");
        app.handle_key(key('l')).unwrap();
        assert!(app.pane == Pane::Center, "l 把焦点移回中栏");

        // 4) 收集后自动跳回 Inbox
        app.handle_key(key('a')).unwrap();
        let s = norm(&frame("6-capture-mode", &mut term, &mut app, &mut out));
        assert!(s.contains("快速录入"), "收集提示");
        for c in "Buy milk".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let s = norm(&frame("7-after-capture", &mut term, &mut app, &mut out));
        assert!(s.contains("Buymilk"), "新收集的任务出现");
        assert!(s.contains("·收件箱"), "收集后跳到 Inbox");

        // 5) 回车 -> next 触发计划钩子（缺时间则询问时间）
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let s = norm(&frame("8-plan-time", &mut term, &mut app, &mut out));
        assert!(s.contains("预计时间"), "计划钩子询问时间");
        // 跳过时间 -> 回到正常；被计划的任务已是 next（已离开 inbox）
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        frame("9-after-plan", &mut term, &mut app, &mut out);
        assert!(app.mode == Mode::Normal, "计划钩子结束");
        let in_next = tasks::list(
            &conn,
            &ListFilter {
                status: Some(task::Status::Next),
                tags: vec![],
                query: None,
                review_stale: false,
            },
        )
        .unwrap()
        .iter()
        .any(|t| t.title == "Write homepage copy");
        assert!(in_next, "被计划的任务已进入 next");

        // 6) 用数字键切换视图
        for (d, lbl, expect) in [
            ('3', "11-waiting", "等待中"),
            ('4', "12-scheduled", "已排程"),
            ('5', "13-someday", "将来/也许"),
            ('6', "14-reference", "参考资料"),
            ('7', "15-done", "已完成"),
            ('1', "16-back-inbox", "收件箱"),
        ] {
            app.handle_key(key(d)).unwrap();
            let s = norm(&frame(lbl, &mut term, &mut app, &mut out));
            assert!(s.contains(expect), "视图 {lbl} 应显示 {expect}");
        }

        // 7) 周回顾向导
        app.handle_key(key('r')).unwrap();
        let s = norm(&frame("17-review", &mut term, &mut app, &mut out));
        assert!(s.contains("每周回顾"), "回顾向导");
        app.handle_key(kc(KeyCode::Esc)).unwrap(); // Cancel wizard

        // 8) 在非 inbox 视图收集后自动跳回 Inbox
        app.handle_key(key('3')).unwrap();
        app.handle_key(key('a')).unwrap();
        for c in "Captured from waiting".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let s = norm(&frame("19-capture-jump", &mut term, &mut app, &mut out));
        assert!(s.contains("·收件箱"), "从 waiting 视图收集后跳到 Inbox");
        assert!(s.contains("Capturedfromwaiting"));

        // 9) 标签 + 排程流程
        app.handle_key(key('t')).unwrap();
        for c in "urgent".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let s = norm(&frame("20-after-tag", &mut term, &mut app, &mut out));
        assert!(s.contains("urgent"), "标签已添加");
        app.handle_key(key('c')).unwrap();
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let sched_id = tasks::list(
            &conn,
            &ListFilter {
                status: None,
                tags: vec!["urgent".to_string()],
                query: None,
                review_stale: false,
            },
        )
        .unwrap()
        .into_iter()
        .next()
        .map(|t| t.id)
        .expect("urgent 任务已打标签");
        let s = norm(&frame("21-after-schedule", &mut term, &mut app, &mut out));
        assert!(
            s.contains(&format!("已排程{}", &sched_id[..8])),
            "显示排程状态消息"
        );

        // 10) 归档(需确认) + 帮助切换 + 退出
        app.handle_key(key('4')).unwrap();
        app.handle_key(key('A')).unwrap();
        frame("22-archive-confirm", &mut term, &mut app, &mut out);
        // 归档需要 y 确认，确认后回到 Normal 才能打开帮助
        app.handle_key(key('y')).unwrap();
        frame("22-after-archive", &mut term, &mut app, &mut out);
        app.handle_key(key('?')).unwrap();
        let s = norm(&frame("23-help", &mut term, &mut app, &mut out));
        assert!(s.contains("快捷键"), "help text");
        app.handle_key(key('?')).unwrap();
        frame("24-help-off", &mut term, &mut app, &mut out);
        app.handle_key(key('q')).unwrap();
        assert!(app.should_quit, "q quits");

        // --- NEW FEATURES TESTS ---

        // Visual Mode
        app.should_quit = false;
        app.handle_key(key('1')).unwrap(); // Switch to Inbox
        app.handle_key(key('v')).unwrap();
        assert!(app.mode == Mode::Visual, "进入可视模式");
        app.handle_key(key('j')).unwrap(); // Move down to select two items
        assert!(!app.selected_ids.is_empty(), "选中了多个任务");
        // Tag them in bulk
        app.handle_key(key('t')).unwrap();
        assert!(app.mode == Mode::Tagging);
        for c in "bulk_tag".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let s = norm(&frame("8-bulk-tagged", &mut term, &mut app, &mut out));
        assert!(s.contains("bulk_tag"), "批量打标签成功");

        // Context Filter
        app.handle_key(key('f')).unwrap();
        assert!(app.mode == Mode::FilteringTag);
        for c in "bulk_tag".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        assert_eq!(app.tag_filter.as_deref(), Some("bulk_tag"));
        let s = norm(&frame("9-context-filter", &mut term, &mut app, &mut out));
        assert!(s.contains("bulk_tag"), "过滤成功");
        app.handle_key(kc(KeyCode::Esc)).unwrap();
        assert_eq!(app.tag_filter, None);

        // Weekly Review Wizard
        app.handle_key(key('r')).unwrap();
        assert!(app.is_reviewing);
        assert_eq!(app.review_step, 1);
        assert_eq!(app.view, View::Inbox);
        let s = norm(&frame("10-review-step1", &mut term, &mut app, &mut out));
        assert!(s.contains("每周回顾"));

        app.handle_key(key('R')).unwrap(); // Step 2
        assert_eq!(app.review_step, 2);
        assert_eq!(app.view, View::Waiting);

        app.handle_key(key('R')).unwrap(); // Step 3
        assert_eq!(app.review_step, 3);
        assert_eq!(app.view, View::Someday);

        app.handle_key(key('R')).unwrap(); // Step 4 (view=Done)
        assert_eq!(app.view, View::Done);
        let s = norm(&frame("11-review-done", &mut term, &mut app, &mut out));
        assert!(s.contains("已完成"), "周回顾第4步显示已完成视图");

        app.handle_key(key('R')).unwrap(); // Finish
        assert!(!app.is_reviewing);
        assert_eq!(app.view, View::Next);
    }

    #[test]
    fn empty_db_shows_guide() {
        crate::repo::pomodoro::set_pomo_idle_for_tests();
        let mut conn = Connection::open(":memory:").unwrap();
        migrate::run(&mut conn).unwrap();
        let mut app = App::new(&conn).unwrap();
        let mut term = Terminal::new(TestBackend::new(110, 30)).unwrap();
        term.draw(|f| app.render(f)).unwrap();
        let raw = snap(&term);
        let mut out = std::fs::File::create("/tmp/gtp_empty_guide.txt").unwrap();
        out.write_all(raw.as_bytes()).unwrap();
        let s = norm(&raw);
        assert!(
            s.contains("欢迎使用gtp"),
            "empty db should show welcome guide"
        );
        assert!(s.contains("Active"), "guide shows groups");
    }

    #[test]
    fn today_tomorrow_views() {
        let mut conn = Connection::open(":memory:").unwrap();
        migrate::run(&mut conn).unwrap();

        let mk = |title: &str, status: task::Status, due_at: i64| {
            tasks::create_capture(
                &conn,
                &CaptureInput {
                    title: title.into(),
                    status,
                    due_at: Some(due_at),
                    tag_names: vec![],
                    ..Default::default()
                },
            )
            .unwrap();
        };
        let day_ms = 24 * 3600 * 1000i64;
        mk(
            "due-today",
            task::Status::Next,
            crate::time::parse_time("today 12:00").unwrap(),
        );
        mk(
            "due-tomorrow",
            task::Status::Scheduled,
            crate::time::parse_time("tomorrow 12:00").unwrap(),
        );
        mk(
            "overdue",
            task::Status::Next,
            crate::time::now_ms() - 2 * day_ms,
        );

        let rec = tasks::create_capture(
            &conn,
            &CaptureInput {
                title: "daily-habit".into(),
                status: task::Status::Scheduled,
                ..Default::default()
            },
        )
        .unwrap();
        tasks::schedule(
            &conn,
            &rec.id,
            crate::time::parse_time("today 09:00").unwrap(),
            None,
            Some("FREQ=DAILY".into()),
        )
        .unwrap();

        let mut app = App::new(&conn).unwrap();
        app.popup = None; // 关闭启动时的今日任务弹窗，避免吞掉按键

        let collect =
            |app: &App| -> Vec<String> { app.items.iter().map(|r| r.title.clone()).collect() };

        app.handle_key(key('J')).unwrap();
        assert_eq!(app.view, View::Today, "Shift+J 切换到今日视图");
        let t = collect(&app);
        assert!(t.iter().any(|s| s == "due-today"), "今日视图含今天到期任务");
        assert!(t.iter().any(|s| s == "overdue"), "今日视图含逾期任务");
        assert!(
            t.iter().any(|s| s == "daily-habit"),
            "今日视图含今日循环发生"
        );

        app.handle_key(key('K')).unwrap();
        assert_eq!(app.view, View::Tomorrow, "Shift+K 切换到明日视图");
        let t = collect(&app);
        assert!(
            t.iter().any(|s| s == "due-tomorrow"),
            "明日视图含明天到期任务"
        );
        assert!(
            t.iter().any(|s| s == "daily-habit"),
            "明日视图含明日循环发生"
        );
        assert!(t.iter().any(|s| s == "overdue"), "明日视图含逾期任务");
        assert!(
            t.iter().any(|s| s == "due-today"),
            "明日视图含今天到期但未完成的任务（结转）"
        );

        // 循环独立性：把今天这次循环标记完成后，明天的发生仍应显示在明日视图
        tasks::transition(&conn, &rec.id, task::Status::Done).unwrap();
        app.handle_key(key('K')).unwrap();
        let t = collect(&app);
        assert!(
            t.iter().any(|s| s == "daily-habit"),
            "今日执行不影响明日循环显示"
        );
    }

    #[test]
    fn checked_in_habit_stays_in_today_with_next_time() {
        crate::repo::pomodoro::set_pomo_idle_for_tests();
        let mut conn = Connection::open(":memory:").unwrap();
        migrate::run(&mut conn).unwrap();

        let rec = tasks::create_capture(
            &conn,
            &CaptureInput {
                title: "daily-habit".into(),
                status: task::Status::Scheduled,
                tag_names: vec![],
                ..Default::default()
            },
        )
        .unwrap();
        tasks::schedule(
            &conn,
            &rec.id,
            crate::time::parse_time("today 09:00").unwrap(),
            None,
            Some("FREQ=DAILY".into()),
        )
        .unwrap();

        // 今日打卡 → 锚点推进到下次 occurrence
        tasks::transition(&conn, &rec.id, task::Status::Done).unwrap();

        let mut app = App::new(&conn).unwrap();
        app.popup = None;
        app.handle_key(key('J')).unwrap(); // 今日视图
        let row = app
            .items
            .iter()
            .find(|r| r.title == "daily-habit")
            .expect("已打卡习惯仍保留在今日视图");
        assert!(row.checked_in_today, "标记为已打卡");
        let next = row.due.expect("有下一次执行时间");
        assert!(next > crate::time::now_ms(), "展示的是未来的下次时间");

        // Scheduled 视图同样标记已打卡
        app.handle_key(key('4')).unwrap();
        let row = app
            .items
            .iter()
            .find(|r| r.title == "daily-habit")
            .expect("Scheduled 视图含该习惯");
        assert!(row.checked_in_today, "Scheduled 视图也标记已打卡");
    }

    #[test]
    fn missed_habit_shows_overdue_in_today_view() {
        crate::repo::pomodoro::set_pomo_idle_for_tests();
        let mut conn = Connection::open(":memory:").unwrap();
        migrate::run(&mut conn).unwrap();

        // 今天的 slot 取凌晨后 1 分钟（几乎必然已过），未打卡 → 今日视图显示逾期
        let slot = crate::time::local_day_bounds(0).0 + 60_000;
        let rec = tasks::create_capture(
            &conn,
            &CaptureInput {
                title: "missed-habit".into(),
                status: task::Status::Scheduled,
                tag_names: vec![],
                ..Default::default()
            },
        )
        .unwrap();
        tasks::schedule(&conn, &rec.id, slot, None, Some("FREQ=DAILY".into())).unwrap();

        let mut app = App::new(&conn).unwrap();
        app.popup = None;
        app.handle_key(key('J')).unwrap();
        let row = app
            .items
            .iter()
            .find(|r| r.title == "missed-habit")
            .expect("今日视图含该习惯");
        assert!(!row.checked_in_today, "未打卡");
        let due = row.due.expect("有 due");
        assert!(
            crate::time::is_overdue(Some(due)),
            "错过的 slot 显示为逾期: {:?}",
            due
        );

        let mut term = Terminal::new(TestBackend::new(110, 30)).unwrap();
        term.draw(|f| app.render(f)).unwrap();
        let s = norm(&snap(&term));
        assert!(s.contains("逾期"), "列表行显示逾期措辞");
    }

    #[test]
    fn relative_due_direction_and_precision() {
        let now = crate::time::now_ms();
        let h = 3600 * 1000i64;
        let d = 24 * 3600 * 1000i64;
        let zh = crate::i18n::Lang::Zh;

        // 未来
        assert_eq!(
            crate::time::relative_due(zh, Some(now + 5 * h)).as_deref(),
            Some("5小时后")
        );
        assert_eq!(
            crate::time::relative_due(zh, Some(now + 30 * 60 * 1000)).as_deref(),
            Some("30分钟后")
        );
        assert_eq!(
            crate::time::relative_due(zh, Some(now + 2 * d)).as_deref(),
            Some("2天后")
        );

        // 过去（统一逾期措辞）
        assert_eq!(
            crate::time::relative_due(zh, Some(now - 5 * h)).as_deref(),
            Some("逾期5小时")
        );
        assert_eq!(
            crate::time::relative_due(zh, Some(now - 40 * 60 * 1000)).as_deref(),
            Some("逾期40分钟")
        );
        assert_eq!(
            crate::time::relative_due(zh, Some(now - 2 * d)).as_deref(),
            Some("逾期2天")
        );
        assert_eq!(
            crate::time::relative_due(zh, Some(now - d)).as_deref(),
            Some("逾期1天")
        );

        // 完成时间展示
        assert_eq!(
            crate::time::relative_past(zh, Some(now - 3 * h)).as_deref(),
            Some("3小时前")
        );
        assert_eq!(
            crate::time::relative_past(zh, Some(now - 2 * d)).as_deref(),
            Some("2天前")
        );
        assert_eq!(crate::time::relative_past(zh, None), None);
    }

    #[test]
    fn recurring_with_due_only_reschedules_on_done() {
        let mut conn = Connection::open(":memory:").unwrap();
        migrate::run(&mut conn).unwrap();

        let t = tasks::create_capture(
            &conn,
            &CaptureInput {
                title: "standup".into(),
                status: task::Status::Scheduled,
                due_at: Some(crate::time::parse_time("today 09:00").unwrap()),
                tag_names: vec![],
                rrule: Some("FREQ=DAILY".into()),
                ..Default::default()
            },
        )
        .unwrap();

        // 只有 due_at + rrule（快速录入场景），完成后应重新排程而非结束
        let done = tasks::transition(&conn, &t.id, task::Status::Done).unwrap();
        assert_eq!(
            done.status,
            task::Status::Scheduled,
            "循环任务完成后被重新排程"
        );
        assert_eq!(done.completed_at, None, "循环任务不进入已完成");
        let next = done.due_at.unwrap();
        let (tom_start, tom_end) = crate::time::local_day_bounds(1);
        assert!(
            next >= tom_start && next <= tom_end,
            "下一次发生落在明日窗口内"
        );

        // 明日视图应仍包含它
        let mut app = App::new(&conn).unwrap();
        app.popup = None;
        app.handle_key(key('K')).unwrap();
        assert!(app.items.iter().any(|r| r.title == "standup"));
    }

    #[test]
    fn next_view_cycles_full_ring() {
        let mut conn = Connection::open(":memory:").unwrap();
        migrate::run(&mut conn).unwrap();
        let mut app = App::new(&conn).unwrap();
        app.popup = None;

        let ring = [
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
        for (i, v) in ring.iter().enumerate() {
            app.view = *v;
            app.next_view(1);
            assert_eq!(
                app.view,
                ring[(i + 1) % ring.len()],
                "正向：{:?} 的下一个",
                v
            );
        }
        for (i, v) in ring.iter().enumerate() {
            app.view = *v;
            app.next_view(-1);
            assert_eq!(
                app.view,
                ring[(i + ring.len() - 1) % ring.len()],
                "反向：{:?} 的上一个",
                v
            );
        }
    }

    #[test]
    fn key_table_respects_view_selection_and_mode() {
        use crate::i18n::Lang;
        use crate::tui::keys::{status_strip, strip_keys, Ctx, KEY_TABLE, NON_TASK_VIEWS};

        let ctx = |v: View, sel: bool| Ctx {
            view: v,
            mode: Mode::Normal,
            has_selection: sel,
            is_reviewing: false,
            pomo_active: false,
        };
        let keys = |v: View, sel: bool| strip_keys(&ctx(v, sel), Lang::Zh);

        // 任务操作键仅在选中时出现，且全局键不进动态条
        let inbox_sel = keys(View::Inbox, true);
        assert!(
            inbox_sel.iter().any(|(k, _)| *k == "Enter"),
            "有选中→含 Enter"
        );
        assert!(
            !inbox_sel.iter().any(|(k, _)| *k == "hjkl"),
            "全局导航键不进动态条"
        );
        assert!(
            !inbox_sel.iter().any(|(k, _)| *k == "q"),
            "全局退出键不进动态条"
        );
        // 空 Inbox 无任务操作 → 动态条为空（渲染层隐藏整块）
        assert!(keys(View::Inbox, false).is_empty(), "无选中→任务操作条为空");

        // 归档箱：u/D 需要选中
        assert!(keys(View::Archived, false).is_empty());
        let arch_sel = keys(View::Archived, true);
        assert!(arch_sel.iter().any(|(k, _)| *k == "u"));
        assert!(arch_sel.iter().any(|(k, _)| *k == "D"));

        // 非任务视图：任务操作键不出现（即使有选中行）
        let tags_sel = keys(View::Tags, true);
        assert!(!tags_sel.iter().any(|(k, _)| *k == "Enter"));
        assert!(tags_sel.iter().any(|(k, _)| *k == "a"), "Tags 有新增标签");
        assert!(tags_sel.iter().any(|(k, _)| *k == "D"), "Tags 有删除标签");

        // 周回顾进行中才出现 R
        let mut reviewing = ctx(View::Inbox, true);
        reviewing.is_reviewing = true;
        assert!(
            strip_keys(&reviewing, Lang::Zh)
                .iter()
                .any(|(k, _)| *k == "R"),
            "周回顾中→含 R"
        );
        assert!(!keys(View::Inbox, true).iter().any(|(k, _)| *k == "R"));

        // 输入/确认模式 → 模式键
        let confirm = Ctx {
            mode: Mode::ConfirmArchive,
            ..ctx(View::Inbox, true)
        };
        assert!(
            strip_keys(&confirm, Lang::Zh)
                .iter()
                .any(|(k, _)| *k == "y/Enter"),
            "确认模式→含 y/Enter"
        );

        // 状态栏全局条：含压缩后的 hjkl 与捕获/退出，不含低频键 g/G 与视图键
        let strip = status_strip(Lang::Zh);
        assert!(strip.iter().any(|(k, _)| *k == "hjkl"), "全局条含 hjkl");
        assert!(strip.iter().any(|(k, _)| *k == "q"), "全局条含 q");
        assert!(!strip.iter().any(|(k, _)| *k == "g/G"), "全局条不含 g/G");
        assert!(!strip.iter().any(|(k, _)| *k == "1-9"), "全局条不含视图键");

        // 表不变量：每条都有键与双语描述；NON_TASK_VIEWS 恰好是 Tags/Archived
        assert!(!KEY_TABLE.is_empty());
        for k in KEY_TABLE {
            assert!(!k.keys.is_empty());
            assert!(!k.zh.is_empty());
            assert!(!k.en.is_empty());
        }
        assert_eq!(NON_TASK_VIEWS, &[View::Tags, View::Archived]);
    }

    #[test]
    fn done_row_shows_completion_time() {
        let mut conn = Connection::open(":memory:").unwrap();
        migrate::run(&mut conn).unwrap();
        let t = tasks::create_capture(
            &conn,
            &CaptureInput {
                title: "finished-thing".into(),
                status: task::Status::Next,
                due_at: Some(crate::time::now_ms() - 3 * 24 * 3600 * 1000i64),
                tag_names: vec![],
                ..Default::default()
            },
        )
        .unwrap();
        let done = tasks::transition(&conn, &t.id, task::Status::Done).unwrap();
        let row = row_from(&done, 0, &conn).unwrap();
        assert_eq!(row.status, "done");
        assert_eq!(
            row.due, done.completed_at,
            "已完成行显示完成时间而非截止时间"
        );
        assert!(row.due.is_some());
    }

    #[test]
    fn done_view_shows_completion_not_overdue() {
        crate::repo::pomodoro::set_pomo_idle_for_tests();
        let mut conn = Connection::open(":memory:").unwrap();
        migrate::run(&mut conn).unwrap();
        let t = tasks::create_capture(
            &conn,
            &CaptureInput {
                title: "finishedlongago".into(),
                status: task::Status::Next,
                due_at: Some(crate::time::now_ms() - 3 * 24 * 3600 * 1000i64),
                tag_names: vec![],
                ..Default::default()
            },
        )
        .unwrap();
        tasks::transition(&conn, &t.id, task::Status::Done).unwrap();
        let mut app = App::new(&conn).unwrap();
        app.popup = None;
        app.handle_key(key('7')).unwrap(); // Done 视图
        let mut term = Terminal::new(TestBackend::new(110, 30)).unwrap();
        term.draw(|f| app.render(f)).unwrap();
        let s = norm(&snap(&term));
        assert!(s.contains("finishedlongago"), "已完成任务显示在 Done 视图");
        assert!(!s.contains("逾期"), "已完成任务不应显示逾期");
    }

    #[test]
    fn archived_view_shows_reason_not_status_or_overdue() {
        crate::repo::pomodoro::set_pomo_idle_for_tests();
        let mut conn = Connection::open(":memory:").unwrap();
        migrate::run(&mut conn).unwrap();
        let t = tasks::create_capture(
            &conn,
            &CaptureInput {
                title: "completed-then-archived".into(),
                status: task::Status::Next,
                due_at: Some(crate::time::now_ms() - 3 * 24 * 3600 * 1000i64),
                tag_names: vec![],
                ..Default::default()
            },
        )
        .unwrap();
        tasks::transition(&conn, &t.id, task::Status::Done).unwrap();
        let arch = tasks::archive(&conn, &t.id).unwrap();
        assert_eq!(arch.archive_reason.as_deref(), Some("completed"));

        let del = tasks::create_capture(
            &conn,
            &CaptureInput {
                title: "deleted-straight-away".into(),
                status: task::Status::Inbox,
                due_at: Some(crate::time::now_ms() - 5 * 24 * 3600 * 1000i64),
                tag_names: vec![],
                ..Default::default()
            },
        )
        .unwrap();
        let arch = tasks::archive(&conn, &del.id).unwrap();
        assert_eq!(arch.archive_reason.as_deref(), Some("deleted"));

        let mut app = App::new(&conn).unwrap();
        app.popup = None;
        app.handle_key(key('8')).unwrap(); // 归档箱视图
        assert_eq!(app.view, View::Archived);
        let mut term = Terminal::new(TestBackend::new(110, 30)).unwrap();
        term.draw(|f| app.render(f)).unwrap();
        let s = norm(&snap(&term));
        assert!(
            s.contains("completed-then-archived"),
            "已完成并归档的任务在归档箱"
        );
        assert!(
            s.contains("deleted-straight-away"),
            "直接删除的任务在归档箱"
        );
        assert!(s.contains("完成"), "显示归档原因：完成");
        assert!(s.contains("删除"), "显示归档原因：删除");
        assert!(!s.contains("逾期"), "归档箱不再显示逾期");
    }

    #[test]
    fn archived_view_can_purge_task() {
        crate::repo::pomodoro::set_pomo_idle_for_tests();
        let mut conn = Connection::open(":memory:").unwrap();
        migrate::run(&mut conn).unwrap();
        let t = tasks::create_capture(
            &conn,
            &CaptureInput {
                title: "purge-from-archive".into(),
                status: task::Status::Inbox,
                tag_names: vec![],
                ..Default::default()
            },
        )
        .unwrap();
        tasks::archive(&conn, &t.id).unwrap();

        let mut app = App::new(&conn).unwrap();
        app.popup = None;
        app.handle_key(key('8')).unwrap(); // 归档箱视图
        assert_eq!(app.view, View::Archived);
        app.handle_key(key('D')).unwrap(); // 触发永久删除确认
        assert_eq!(app.mode, Mode::ConfirmPurge, "进入永久删除确认");
        app.handle_key(key('y')).unwrap(); // 确认
        assert!(tasks::get(&conn, &t.id).is_err(), "任务已被永久删除");
        assert!(app.items.is_empty(), "归档箱列表已刷新为空");
        assert_eq!(app.view, View::Archived, "仍停留在归档箱视图");

        // 取消路径：再归档一条，按 D 后按 n 应保留任务
        let t2 = tasks::create_capture(
            &conn,
            &CaptureInput {
                title: "keep-me".into(),
                status: task::Status::Inbox,
                tag_names: vec![],
                ..Default::default()
            },
        )
        .unwrap();
        tasks::archive(&conn, &t2.id).unwrap();
        app.handle_key(key('8')).unwrap();
        app.handle_key(key('D')).unwrap();
        assert_eq!(app.mode, Mode::ConfirmPurge);
        app.handle_key(key('n')).unwrap();
        assert_eq!(app.mode, Mode::Normal, "取消后回到 Normal");
        assert!(tasks::get(&conn, &t2.id).is_ok(), "取消删除后任务仍在");
    }

    #[test]
    fn archived_view_can_purge_multiple_in_visual_mode() {
        crate::repo::pomodoro::set_pomo_idle_for_tests();
        let mut conn = Connection::open(":memory:").unwrap();
        migrate::run(&mut conn).unwrap();
        for i in 0..3 {
            let t = tasks::create_capture(
                &conn,
                &CaptureInput {
                    title: format!("bulk-purge-{i}"),
                    status: task::Status::Inbox,
                    tag_names: vec![],
                    ..Default::default()
                },
            )
            .unwrap();
            tasks::archive(&conn, &t.id).unwrap();
        }

        let mut app = App::new(&conn).unwrap();
        app.popup = None;
        app.handle_key(key('8')).unwrap(); // 归档箱视图
                                           // 归档按 archived_at DESC 排序，但快速连建三条可能落同一毫秒导致并列序不定，
                                           // 故按实际列表顺序取前两行，而非依赖插入顺序。
        let top_two: Vec<String> = tasks::list_archived(&conn)
            .unwrap()
            .into_iter()
            .take(2)
            .map(|t| t.id)
            .collect();
        assert_eq!(top_two.len(), 2, "归档箱至少两条用于批量测试");
        app.handle_key(key('v')).unwrap(); // 进入可视模式
        app.handle_key(key('j')).unwrap(); // 选中前两项
        app.handle_key(key('D')).unwrap(); // 触发批量永久删除确认
        assert_eq!(app.mode, Mode::ConfirmPurge, "进入批量永久删除确认");
        assert_eq!(app.pending_purge_ids.len(), 2, "可视模式选中了 2 项");
        app.handle_key(key('y')).unwrap(); // 确认

        for id in &top_two {
            assert!(tasks::get(&conn, id).is_err(), "选中项已删除: {}", id);
        }
        let remaining: Vec<_> = tasks::list_archived(&conn).unwrap();
        assert_eq!(remaining.len(), 1, "归档箱只剩未被选中的任务");
        assert!(
            remaining[0].id != top_two[0] && remaining[0].id != top_two[1],
            "剩余项未被选中"
        );
        assert_eq!(app.mode, Mode::Normal, "删除后退出可视模式");
        assert!(app.selected_ids.is_empty(), "选择集已清空");
    }

    #[test]
    fn biweekly_shorthand_reschedules_after_done() {
        let mut conn = Connection::open(":memory:").unwrap();
        migrate::run(&mut conn).unwrap();

        // 一句话录入：*2w[1,3] → 每两周周一/周三
        let q = crate::parser::parse_quick_add("上体育课 *2w[1,3] ~2026-08-12 09:00");
        assert_eq!(
            q.rrule.as_deref(),
            Some("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE")
        );
        let due = crate::time::parse_time("2026-08-12 09:00").unwrap(); // 周三

        let t = tasks::create_capture(
            &conn,
            &CaptureInput {
                title: q.title,
                status: task::Status::Scheduled,
                due_at: Some(due),
                tag_names: q.tags,
                rrule: q.rrule,
                ..Default::default()
            },
        )
        .unwrap();

        // 完成后被重新排程到隔周的周一 (08-24), 而非结束
        let done = tasks::transition(&conn, &t.id, task::Status::Done).unwrap();
        assert_eq!(done.status, task::Status::Scheduled, "循环任务重新排程");
        assert_eq!(done.completed_at, None);
        assert_eq!(
            crate::time::format_local(done.due_at),
            "2026-08-24 09:00",
            "下一次发生 = 2 周后的周一"
        );
    }

    #[test]
    fn lang_and_theme_toggle_persist_to_settings() {
        crate::repo::pomodoro::set_pomo_idle_for_tests();
        let mut conn = Connection::open(":memory:").unwrap();
        migrate::run(&mut conn).unwrap();

        // 默认中文 + 深色主题
        let mut app = App::new(&conn).unwrap();
        app.popup = None;
        assert_eq!(app.lang, crate::i18n::Lang::Zh);
        assert!(app.theme.is_dark, "默认 Catppuccin Mocha 深色");
        assert_eq!(crate::repo::settings::get(&conn, "lang").unwrap(), None);
        assert_eq!(crate::repo::settings::get(&conn, "theme").unwrap(), None);

        let mut term = Terminal::new(TestBackend::new(110, 30)).unwrap();
        term.draw(|f| app.render(f)).unwrap();
        let s = norm(&snap(&term));
        assert!(s.contains("收件箱"), "中文默认显示收件箱");

        // F6 切英文 → 写入 settings 表，界面文案切换
        app.handle_key(kc(KeyCode::F(6))).unwrap();
        assert_eq!(app.lang, crate::i18n::Lang::En);
        assert_eq!(
            crate::repo::settings::get(&conn, "lang")
                .unwrap()
                .as_deref(),
            Some("en")
        );
        term.draw(|f| app.render(f)).unwrap();
        let s = norm(&snap(&term));
        assert!(s.contains("Inbox"), "英文侧边栏显示 Inbox");

        // F5 切亮色主题 → 写入 settings 表
        app.handle_key(kc(KeyCode::F(5))).unwrap();
        assert!(!app.theme.is_dark, "F5 切到 Latte 亮色");
        assert_eq!(
            crate::repo::settings::get(&conn, "theme")
                .unwrap()
                .as_deref(),
            Some("latte")
        );

        // 模拟重启：从 DB 恢复语言与主题
        drop(app);
        let mut app = App::new(&conn).unwrap();
        app.popup = None;
        assert_eq!(app.lang, crate::i18n::Lang::En, "重启后恢复英文");
        assert!(!app.theme.is_dark, "重启后恢复亮色主题");
    }

    #[test]
    fn shift_c_enters_checklist_adding_and_pomo_config_moved_to_bracket() {
        let mut conn = Connection::open(":memory:").unwrap();
        migrate::run(&mut conn).unwrap();
        seed(&conn);
        let mut app = App::new(&conn).unwrap();
        app.popup = None;

        // Shift+C（Char('C')）→ 新增检查单（不再被番茄钟配置遮蔽）
        app.handle_key(key('C')).unwrap();
        assert_eq!(app.mode, Mode::ChecklistAdding, "Shift+C 进入新增检查单");
        app.handle_key(kc(KeyCode::Esc)).unwrap();

        // '[' → 自定义番茄钟时长
        app.handle_key(key('[')).unwrap();
        assert_eq!(app.mode, Mode::ConfiguringPomo, "'[' 进入番茄钟配置");
        app.handle_key(kc(KeyCode::Esc)).unwrap();
    }
}
