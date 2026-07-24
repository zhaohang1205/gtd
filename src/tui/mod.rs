pub mod app;
pub mod handlers;
pub mod render;
pub mod ui;
pub mod calendar;

pub(crate) use app::{App, Pane, View};
pub(crate) use handlers::AppHandlers;
pub(crate) use render::AppRender;

use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};
use crossterm::terminal::{enable_raw_mode, disable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::execute;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, Stdout};
use std::time::Duration;
use crate::model::task::{self, Task};
use crate::repo::tags;
use rusqlite::Connection;
use app::Row;

/// 状态的中文含义，用于引导栏的“状态地图”。
pub(crate) fn status_cn(s: task::Status) -> &'static str {
    match s {
        task::Status::Inbox => "收件箱",
        task::Status::Next => "下一步",
        task::Status::Waiting => "等待中",
        task::Status::Scheduled => "已排程",
        task::Status::Someday => "将来/也许",
        task::Status::Reference => "参考资料",
        task::Status::Done => "已完成",
    }
}

/// 根据当前视图，给出“下一步该做什么”的提示。
pub(crate) fn next_hint(v: View) -> &'static str {
    match v {
        View::Inbox => "按 Enter 理清，决定它的去向",
        View::Next => "选一条开始行动（做）",
        View::Waiting => "跟进被阻塞的事项",
        View::Scheduled => "按排程时间执行",
        View::Someday => "定期回顾是否激活",
        View::Reference => "需要时检索查阅",
        View::Done => "可归档已完成事项",
        View::Projects => "把收件箱行动归入项目",
        View::Review => "清空各类积压",
    }
}

pub(crate) fn row_from(t: &Task, indent: usize, conn: &Connection) -> Result<Row> {
    let tags = tags::get_task_tags(conn, &t.id)?
        .iter()
        .map(|x| x.name.clone())
        .collect();
    Ok(Row {
        id: t.id.clone(),
        title: t.title.clone(),
        status: t.status.to_string(),
        due: t.due_at.or(t.scheduled_start_at),
        tags,
        indent,
    })
}

/// 启动交互式 TUI。
pub fn run(conn: &Connection) -> Result<()> {
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
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<Stdout>>, conn: &Connection) -> Result<()> {
    let mut app = App::new(conn)?;
    loop {
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
    use crate::repo::tasks::{self, CaptureInput};
    use ratatui::backend::TestBackend;
    use std::io::Write;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use crate::tui::app::Mode;
    use crate::repo::tasks::ListFilter;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty())
    }
    fn kc(k: KeyCode) -> KeyEvent {
        KeyEvent::new(k, KeyModifiers::empty())
    }
    fn ctr(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn seed(conn: &Connection) {
        let proj = tasks::create_capture(
            conn,
            &CaptureInput {
                title: "Website Redesign".into(),
                kind: task::TaskKind::Project,
                parent_id: None,
                status: task::Status::Next,
                due_at: None,
                tag_names: vec![],
                ..Default::default()
            },
        )
        .unwrap();
        let mk = |title: &str, kind: task::TaskKind, parent: Option<&str>, status: task::Status, tags: &[&str]| {
            tasks::create_capture(
                conn,
                &CaptureInput {
                    title: title.into(),
                    kind,
                    parent_id: parent.map(|s| s.to_string()),
                    status,
                    due_at: None,
                    tag_names: tags.iter().map(|s| s.to_string()).collect(),
                    ..Default::default()
                },
            )
            .unwrap();
        };
        mk("Write homepage copy", task::TaskKind::Action, Some(&proj.id), task::Status::Inbox, &["work", "p1"]);
        mk("Buy groceries", task::TaskKind::Action, None, task::Status::Inbox, &["home", "errands"]);
        mk("Read Rust book", task::TaskKind::Action, None, task::Status::Next, &["learning"]);
        mk("Pay taxes", task::TaskKind::Action, None, task::Status::Waiting, &["work", "p2"]);
        mk("Plan vacation", task::TaskKind::Action, None, task::Status::Someday, &["home"]);
        mk("Finish report", task::TaskKind::Action, None, task::Status::Done, &[]);
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
    pub(crate) fn visual_len(s: &str) -> usize {
        s.chars().filter(|c| *c != ' ').collect::<String>().len()
    }
    
    fn norm(s: &str) -> String {
        s.chars().filter(|c| *c != ' ').collect()
    }

    #[test]
    fn drive_tui() {
        let mut conn = Connection::open(":memory:").unwrap();
        migrate::run(&mut conn).unwrap();
        seed(&conn);
        let mut app = App::new(&conn).unwrap();
        let mut term = Terminal::new(TestBackend::new(110, 30)).unwrap();
        let mut out = std::fs::File::create("/tmp/gtp_tui_frames.txt").unwrap();
        let frame = |label: &str, term: &mut Terminal<TestBackend>, app: &mut App, out: &mut std::fs::File| -> String {
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
        assert!(s.contains("Tasks·Inbox"), "中栏列表标题");
        assert!(s.contains("任务详情"), "右侧详情栏");
        assert!(s.contains("Buygroceries"), "inbox 列出已灌入的任务");
        assert!(s.contains("等待中") && s.contains("将来/也许"), "上下文分组已列出");

        // 2) vim 导航：下、上
        app.handle_key(key('j')).unwrap();
        frame("2-nav-down", &mut term, &mut app, &mut out);
        app.handle_key(key('k')).unwrap();
        frame("3-nav-up", &mut term, &mut app, &mut out);

        // 3) h/l 把焦点在 Left, Center, Right 之间切换
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
        assert!(s.contains("Newtask"), "收集提示");
        for c in "Buy milk".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let s = norm(&frame("7-after-capture", &mut term, &mut app, &mut out));
        assert!(s.contains("Buymilk"), "新收集的任务出现");
        assert!(s.contains("·Inbox"), "收集后跳到 Inbox");

        // 5) 回车 -> next 触发计划钩子（先问项目，再问时间）
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let s = norm(&frame("8-plan-project", &mut term, &mut app, &mut out));
        assert!(s.contains("Project?"), "计划钩子询问项目");
        // 跳过项目
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let s = norm(&frame("9-plan-time", &mut term, &mut app, &mut out));
        assert!(s.contains("Time?"), "计划钩子询问时间");
        // 跳过时间 -> 回到正常；被计划的任务已是 next（已离开 inbox）
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        frame("10-after-plan", &mut term, &mut app, &mut out);
        assert!(app.mode == Mode::Normal, "计划钩子结束");
        let in_next = tasks::list(
            &conn,
            &ListFilter {
                status: Some(task::Status::Next),
                project: None,
                tags: vec![],
                query: None,
            },
        )
        .unwrap()
        .iter()
        .any(|t| t.title == "Write homepage copy");
        assert!(in_next, "被计划的任务已进入 next");

        // 6) 用数字键切换视图
        for (d, lbl, expect) in [
            ('3', "11-waiting", "Waiting"),
            ('4', "12-scheduled", "Scheduled"),
            ('5', "13-someday", "Someday"),
            ('6', "14-reference", "Reference"),
            ('7', "15-done", "Done"),
            ('1', "16-back-inbox", "Inbox"),
        ] {
            app.handle_key(key(d)).unwrap();
            let s = norm(&frame(lbl, &mut term, &mut app, &mut out));
            assert!(s.contains(expect), "视图 {lbl} 应显示 {expect}");
        }

        // 7) 项目树 + 周回顾
        app.handle_key(key('p')).unwrap();
        let s = norm(&frame("17-projects", &mut term, &mut app, &mut out));
        assert!(s.contains("WebsiteRedesign"), "项目视图");
        app.handle_key(key('r')).unwrap();
        let s = norm(&frame("18-review", &mut term, &mut app, &mut out));
        assert!(s.contains("WeeklyReview"), "回顾视图");

        // 8) 在非 inbox 视图收集后自动跳回 Inbox
        app.handle_key(key('3')).unwrap();
        app.handle_key(key('a')).unwrap();
        for c in "Captured from waiting".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(kc(KeyCode::Enter)).unwrap();
        let s = norm(&frame("19-capture-jump", &mut term, &mut app, &mut out));
        assert!(s.contains("·Inbox"), "从 waiting 视图收集后跳到 Inbox");
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
        let s = norm(&frame("21-after-schedule", &mut term, &mut app, &mut out));
        assert!(s.contains("sched"), "显示排程时间");

        // 10) 归档 + 帮助切换 + 退出
        app.handle_key(key('4')).unwrap();
        app.handle_key(key('A')).unwrap();
        frame("22-after-archive", &mut term, &mut app, &mut out);
        app.handle_key(key('?')).unwrap();
        let s = norm(&frame("23-help", &mut term, &mut app, &mut out));
        assert!(s.contains("快捷键"), "help text");
        app.handle_key(key('?')).unwrap();
        frame("24-help-off", &mut term, &mut app, &mut out);
        app.handle_key(key('q')).unwrap();
        assert!(app.should_quit, "q quits");
    }

    #[test]
    fn empty_db_shows_guide() {
        let mut conn = Connection::open(":memory:").unwrap();
        migrate::run(&mut conn).unwrap();
        let mut app = App::new(&conn).unwrap();
        let mut term = Terminal::new(TestBackend::new(110, 30)).unwrap();
        term.draw(|f| app.render(f)).unwrap();
        let raw = snap(&term);
        let mut out = std::fs::File::create("/tmp/gtp_empty_guide.txt").unwrap();
        out.write_all(raw.as_bytes()).unwrap();
        let s = norm(&raw);
        assert!(s.contains("欢迎使用gtp"), "empty db should show welcome guide");
        assert!(s.contains("Active"), "guide shows groups");
    }
}
