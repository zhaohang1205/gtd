use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
    widgets::ListItem,
};

use crate::tui::App;

pub fn status_letter(s: &str) -> &'static str {
    match s {
        "inbox" => ".",
        "next" => ">",
        "waiting" => "W",
        "scheduled" => "#",
        "someday" => "?",
        "reference" => "*",
        "done" => "x",
        _ => " ",
    }
}

pub fn status_color(s: &str) -> Color {
    match s {
        "inbox" => Color::Gray,
        "next" => Color::Yellow,
        "waiting" => Color::Blue,
        "scheduled" => Color::Cyan,
        "someday" => Color::Magenta,
        "reference" => Color::White,
        "done" => Color::Green,
        _ => Color::Gray,
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
