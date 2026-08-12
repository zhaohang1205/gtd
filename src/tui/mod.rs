pub mod app;
pub mod calendar;
pub mod handlers;
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

/// 根据当前视图，给出“下一步该做什么”的提示。
pub(crate) fn next_hint(lang: crate::i18n::Lang, v: View) -> &'static str {
    match v {
        View::Inbox => crate::tr!(
            lang,
            "按 Enter 理清，决定它的去向",
            "Enter to clarify and decide its fate"
        ),
        View::Today => crate::tr!(
            lang,
            "今日到期/逾期任务，逐条动手完成",
            "Due/overdue today — knock them out one by one"
        ),
        View::Tomorrow => crate::tr!(
            lang,
            "明日任务与需结转的未完成任务",
            "Tomorrow's tasks plus overdue carry-overs"
        ),
        View::Next => crate::tr!(lang, "选一条开始行动（做）", "Pick one and get moving"),
        View::Waiting => crate::tr!(lang, "跟进被阻塞的事项", "Follow up on blocked items"),
        View::Scheduled => crate::tr!(lang, "按排程时间执行", "Execute on schedule"),
        View::Someday => crate::tr!(
            lang,
            "定期回顾是否激活",
            "Review periodically whether to activate"
        ),
        View::Reference => crate::tr!(lang, "需要时检索查阅", "Look up when needed"),
        View::Done => crate::tr!(lang, "可归档已完成事项", "Archive completed items"),
        View::Review => crate::tr!(lang, "清空各类积压", "Clear the backlogs"),
        View::Archived => crate::tr!(lang, "选中后按 u 恢复任务", "Press u to restore a task"),
        View::Tags => crate::tr!(
            lang,
            "按 a 新增标签，按 D 删除自定义标签，按 f 过滤",
            "a: add tag, D: delete custom tag, f: filter"
        ),
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
            t.due_at.or(t.scheduled_start_at)
        },
        tags,
        indent,
        done,
        total,
        archive_reason: t.archive_reason.clone(),
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
                                app.help_scroll = app.help_scroll.saturating_add(1).min(20);
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

        // 过去（修复前：5小时前被 rem_euclid 算成 19小时前）
        assert_eq!(
            crate::time::relative_due(zh, Some(now - 5 * h)).as_deref(),
            Some("5小时前")
        );
        assert_eq!(
            crate::time::relative_due(zh, Some(now - 40 * 60 * 1000)).as_deref(),
            Some("40分钟前")
        );
        assert_eq!(
            crate::time::relative_due(zh, Some(now - 2 * d)).as_deref(),
            Some("逾期2天")
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
    fn next_view_skips_today_tomorrow() {
        let mut conn = Connection::open(":memory:").unwrap();
        migrate::run(&mut conn).unwrap();
        let mut app = App::new(&conn).unwrap();
        app.popup = None;

        app.view = View::Inbox;
        app.next_view(1);
        assert_eq!(
            app.view,
            View::Next,
            "Inbox 方向键下一个是 Next，而非 Today"
        );

        app.view = View::Next;
        app.next_view(1);
        assert_eq!(app.view, View::Waiting);

        // 从日视图用方向键也能离开，且不会停在日视图
        app.view = View::Today;
        app.next_view(1);
        assert_eq!(app.view, View::Next);
        app.view = View::Tomorrow;
        app.next_view(-1);
        assert!(app.view != View::Today && app.view != View::Tomorrow);
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
