pub struct QuickAdd {
    pub title: String,
    pub tags: Vec<String>,
    pub time_str: Option<String>,
    pub rrule: Option<String>,
    /// 优先级, 归一化为系统标签名: `!a`→p1 (最高), `!b`→p2, `!c`→p3.
    pub priority: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickAddKind {
    Title,
    Tag,
    Time,
    Rrule,
    Priority,
}

#[derive(Debug)]
pub struct QuickAddToken {
    pub kind: QuickAddKind,
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// Map a priority letter to its tag name: `!a`→p1 (最高), `!b`→p2, `!c`→p3.
pub fn priority_tag(letter: &str) -> Option<&'static str> {
    match letter {
        "a" | "A" => Some("p1"),
        "b" | "B" => Some("p2"),
        "c" | "C" => Some("p3"),
        _ => None,
    }
}

/// Reverse of [`priority_tag`]: `p1`→'a' (最高), `p2`→'b', `p3`→'c'.
pub fn priority_letter(tag: &str) -> Option<char> {
    match tag {
        "p1" => Some('a'),
        "p2" => Some('b'),
        "p3" => Some('c'),
        _ => None,
    }
}

/// Walk words in input order using `split_whitespace` semantics.
/// Each word: `@x`→Tag, `~x`→Time, `*x`→Rrule, `!x`→Priority (each only when
/// `word.len() > 1`), else Title. `start`/`end` are byte offsets of the word
/// INCLUDING its prefix.
pub fn tokenize_quick_add(input: &str) -> Vec<QuickAddToken> {
    let mut tokens = Vec::new();
    let mut chars = input.char_indices().peekable();
    while matches!(chars.peek(), Some((_, c)) if c.is_whitespace()) {
        chars.next();
    }
    while let Some((start, _)) = chars.peek().copied() {
        let mut end = start;
        for (idx, c) in chars.by_ref() {
            if c.is_whitespace() {
                end = idx;
                break;
            }
            end = idx + c.len_utf8();
        }
        let word = &input[start..end];
        let kind = if word.starts_with('@') && word.len() > 1 {
            QuickAddKind::Tag
        } else if word.starts_with('~') && word.len() > 1 {
            QuickAddKind::Time
        } else if word.starts_with('*') && word.len() > 1 {
            QuickAddKind::Rrule
        } else if word.starts_with('!') && word.len() > 1 {
            QuickAddKind::Priority
        } else {
            QuickAddKind::Title
        };
        tokens.push(QuickAddToken {
            kind,
            start,
            end,
            text: word.to_string(),
        });
        while matches!(chars.peek(), Some((_, c)) if c.is_whitespace()) {
            chars.next();
        }
    }
    tokens
}

pub fn parse_quick_add(input: &str) -> QuickAdd {
    let mut title_parts = Vec::new();
    let mut tags = Vec::new();
    let mut time_str = None;
    let mut rrule = None;
    let mut priority = None;

    for tok in tokenize_quick_add(input) {
        match tok.kind {
            QuickAddKind::Title => title_parts.push(tok.text),
            QuickAddKind::Tag => tags.push(tok.text[1..].to_string()),
            QuickAddKind::Time => time_str = Some(tok.text[1..].to_string()),
            QuickAddKind::Rrule => rrule = Some(parse_rrule_shorthand(&tok.text[1..])),
            QuickAddKind::Priority => {
                let letter = &tok.text[1..];
                if let Some(tag) = priority_tag(letter) {
                    priority = Some(tag.to_string());
                } else {
                    // 无法识别的 !x 词按普通标题处理, 不静默丢弃
                    title_parts.push(tok.text);
                }
            }
        }
    }

    QuickAdd {
        title: title_parts.join(" "),
        tags,
        time_str,
        rrule,
        priority,
    }
}

pub fn parse_rrule_shorthand(s: &str) -> String {
    let lower = s.to_lowercase();
    if lower.starts_with("freq=") {
        return s.to_string();
    }
    match lower.as_str() {
        "daily" => return "FREQ=DAILY".to_string(),
        "weekly" => return "FREQ=WEEKLY".to_string(),
        "monthly" => return "FREQ=MONTHLY".to_string(),
        "yearly" => return "FREQ=YEARLY".to_string(),
        "weekday" | "workday" => return "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR".to_string(),
        "weekend" => return "FREQ=WEEKLY;BYDAY=SA,SU".to_string(),
        _ => {}
    }

    // Try to match patterns like "1d", "2w", "3m", "4y"
    if let Some(pos) = lower.find(|c: char| !c.is_ascii_digit()) {
        if pos > 0 && pos == lower.len() - 1 {
            let num_str = &lower[..pos];
            if let Ok(num) = num_str.parse::<u32>() {
                let unit = &lower[pos..];
                let freq = match unit {
                    "d" => "DAILY",
                    "w" => "WEEKLY",
                    "m" => "MONTHLY",
                    "y" => "YEARLY",
                    _ => "",
                };
                if !freq.is_empty() {
                    return format!("FREQ={};INTERVAL={}", freq, num);
                }
            }
        }
    }

    // Try to match comma separated days like "mon,we,fri"
    let mut days = Vec::new();
    let mut valid = true;
    for part in lower.split(',') {
        let day = match part.trim() {
            "mo" | "mon" | "monday" => "MO",
            "tu" | "tue" | "tuesday" => "TU",
            "we" | "wed" | "wednesday" => "WE",
            "th" | "thu" | "thursday" => "TH",
            "fr" | "fri" | "friday" => "FR",
            "sa" | "sat" | "saturday" => "SA",
            "su" | "sun" | "sunday" => "SU",
            _ => {
                valid = false;
                break;
            }
        };
        days.push(day);
    }
    if valid && !days.is_empty() {
        return format!("FREQ=WEEKLY;BYDAY={}", days.join(","));
    }

    // fallback
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_letters_map_to_tags() {
        assert_eq!(priority_tag("a"), Some("p1"));
        assert_eq!(priority_tag("b"), Some("p2"));
        assert_eq!(priority_tag("c"), Some("p3"));
        assert_eq!(priority_tag("A"), Some("p1"));
        assert_eq!(priority_tag("x"), None);
        assert_eq!(priority_letter("p1"), Some('a'));
        assert_eq!(priority_letter("p3"), Some('c'));
    }

    #[test]
    fn quick_add_parses_priority() {
        let q = parse_quick_add("写周报 @work !a ~+3d");
        assert_eq!(q.title, "写周报");
        assert_eq!(q.tags, vec!["work"]);
        assert_eq!(q.priority.as_deref(), Some("p1"));
        assert_eq!(q.time_str.as_deref(), Some("+3d"));
    }

    #[test]
    fn last_priority_wins() {
        let q = parse_quick_add("任务 !a !c");
        assert_eq!(q.title, "任务");
        assert_eq!(q.priority.as_deref(), Some("p3"));
    }

    #[test]
    fn unknown_priority_stays_in_title() {
        let q = parse_quick_add("无效 !z 保留");
        assert_eq!(q.title, "无效 !z 保留");
        assert_eq!(q.priority, None);
    }
}
