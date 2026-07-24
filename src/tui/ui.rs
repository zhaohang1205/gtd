use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
    widgets::ListItem,
};

use crate::tui::App;

use crate::model::task::Status;

pub fn status_letter(s: &Status) -> &'static str {
    match s {
        Status::Inbox => ".",
        Status::Next => ">",
        Status::Waiting => "W",
        Status::Scheduled => "#",
        Status::Someday => "?",
        Status::Reference => "*",
        Status::Done => "x",
    }
}

pub fn status_color(s: &Status) -> Color {
    match s {
        Status::Inbox => Color::Gray,
        Status::Next => Color::Yellow,
        Status::Waiting => Color::Blue,
        Status::Scheduled => Color::Cyan,
        Status::Someday => Color::Magenta,
        Status::Reference => Color::White,
        Status::Done => Color::Green,
    }
}

pub fn build_list_items(app: &App) -> Vec<ListItem<'static>> {
    app.items
        .iter()
        .map(|r| {
            let letter = status_letter(&r.status);
            let color = status_color(&r.status);
            let due = crate::time::format_local(r.due);
            let tags = r.tags.join(",");
            let indent = "  ".repeat(r.indent);
            let line = Line::from(vec![
                Span::styled(format!("{}{} ", indent, letter), Style::default().fg(color)),
                Span::raw(r.title.clone()),
                Span::raw(format!("  {}  [{}]", due, tags)),
            ]);
            ListItem::new(line)
        })
        .collect()
}
