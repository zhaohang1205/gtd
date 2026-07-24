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
            let status_enum = r.status.parse::<crate::model::task::Status>().unwrap_or(crate::model::task::Status::Inbox);
            let letter = status_letter(&status_enum);
            let color = status_color(&status_enum);
            let due = crate::time::format_local(r.due);
            let tags = r.tags.join(",");
            let indent = "  ".repeat(r.indent);
            
            let is_selected = app.mode == crate::tui::app::Mode::Visual && app.selected_ids.contains(&r.id);
            let sel_prefix = if is_selected { " [v]" } else { "" };
            
            let line = Line::from(vec![
                Span::styled(format!("{}{}{} ", sel_prefix, indent, letter), Style::default().fg(if is_selected { Color::Yellow } else { color })),
                Span::styled(r.title.clone(), if is_selected { Style::default().add_modifier(ratatui::style::Modifier::BOLD) } else { Style::default() }),
                Span::raw(format!("  {}  [{}]", due, tags)),
            ]);
            
            let mut item = ListItem::new(line);
            if is_selected {
                item = item.style(Style::default().bg(Color::DarkGray));
            }
            item
        })
        .collect()
}
