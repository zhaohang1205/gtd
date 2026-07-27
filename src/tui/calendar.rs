use chrono::{Datelike, Days, NaiveDate, Local};
use ratatui::{
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Clear, Paragraph, Row, Table},
    Frame,
};

#[derive(Clone, PartialEq, Eq)]
pub struct CalendarState {
    pub cursor: NaiveDate,
    pub start_date: Option<NaiveDate>,
}

impl CalendarState {
    pub fn new() -> Self {
        Self {
            cursor: Local::now().date_naive(),
            start_date: None,
        }
    }

    pub fn handle_key(&mut self, code: crossterm::event::KeyCode) -> Option<Option<(NaiveDate, NaiveDate)>> {
        use crossterm::event::KeyCode;
        match code {
            KeyCode::Char('h') | KeyCode::Left => {
                if let Some(prev) = self.cursor.checked_sub_days(Days::new(1)) {
                    self.cursor = prev;
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if let Some(next) = self.cursor.checked_add_days(Days::new(1)) {
                    self.cursor = next;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(prev) = self.cursor.checked_sub_days(Days::new(7)) {
                    self.cursor = prev;
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(next) = self.cursor.checked_add_days(Days::new(7)) {
                    self.cursor = next;
                }
            }
            KeyCode::Char('K') | KeyCode::PageUp => {
                let m = self.cursor.month();
                let y = self.cursor.year();
                let (nm, ny) = if m == 1 { (12, y - 1) } else { (m - 1, y) };
                if let Some(next) = NaiveDate::from_ymd_opt(ny, nm, self.cursor.day().min(28)) {
                    self.cursor = next;
                }
            }
            KeyCode::Char('J') | KeyCode::PageDown => {
                let m = self.cursor.month();
                let y = self.cursor.year();
                let (nm, ny) = if m == 12 { (1, y + 1) } else { (m + 1, y) };
                if let Some(next) = NaiveDate::from_ymd_opt(ny, nm, self.cursor.day().min(28)) {
                    self.cursor = next;
                }
            }
            KeyCode::Enter => {
                if let Some(start) = self.start_date {
                    let end = self.cursor;
                    let (s, e) = if start <= end { (start, end) } else { (end, start) };
                    return Some(Some((s, e)));
                } else {
                    self.start_date = Some(self.cursor);
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                if self.start_date.is_some() {
                    self.start_date = None;
                } else {
                    return Some(None);
                }
            }
            _ => {}
        }
        None
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(format!(" {}年 {}月 ", self.cursor.year(), self.cursor.month()));

        f.render_widget(Clear, area);

        let mut rows = vec![];
        rows.push(Row::new(vec![
            "日", "一", "二", "三", "四", "五", "六",
        ]).style(Style::default().fg(Color::DarkGray)));

        let first_day = NaiveDate::from_ymd_opt(self.cursor.year(), self.cursor.month(), 1).unwrap();
        let weekday = first_day.weekday().num_days_from_sunday();

        let days_in_month = if self.cursor.month() == 12 {
            31
        } else {
            NaiveDate::from_ymd_opt(self.cursor.year(), self.cursor.month() + 1, 1)
                .unwrap()
                .signed_duration_since(first_day)
                .num_days()
        };

        let mut current_row = vec![Span::raw(""); 7];
        let mut day_of_week = weekday as usize;

        for d in 1..=days_in_month {
            let date = NaiveDate::from_ymd_opt(self.cursor.year(), self.cursor.month(), d as u32).unwrap();
            let mut style = Style::default();
            
            let is_cursor = date == self.cursor;
            let mut is_in_range = false;
            
            if let Some(start) = self.start_date {
                let end = self.cursor;
                let (min, max) = if start <= end { (start, end) } else { (end, start) };
                if date >= min && date <= max {
                    is_in_range = true;
                }
            }

            if is_cursor {
                style = style.bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD);
            } else if is_in_range {
                style = style.bg(Color::DarkGray).fg(Color::White);
            }

            current_row[day_of_week] = Span::styled(format!("{:>2}", d), style);

            day_of_week += 1;
            if day_of_week == 7 {
                rows.push(Row::new(current_row.clone()));
                current_row = vec![Span::raw(""); 7];
                day_of_week = 0;
            }
        }
        
        if day_of_week > 0 {
            rows.push(Row::new(current_row));
        }

        let widths = [
            Constraint::Length(3), Constraint::Length(3), Constraint::Length(3),
            Constraint::Length(3), Constraint::Length(3), Constraint::Length(3), Constraint::Length(3)
        ];
        
        let table = Table::new(rows, widths).block(block).column_spacing(1);
        f.render_widget(table, area);

        let hint = if self.start_date.is_none() {
            " 回车:选择起始日期 | Esc:退出 "
        } else {
            " 回车:选择结束日期 | Esc:重选 "
        };
        let hint_area = Rect::new(area.x + 2, area.y + area.height.saturating_sub(1), area.width.saturating_sub(4), 1);
        f.render_widget(Paragraph::new(Span::styled(hint, Style::default().fg(Color::Cyan))), hint_area);
    }
}
