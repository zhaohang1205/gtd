pub struct QuickAdd {
    pub title: String,
    pub tags: Vec<String>,
    pub time_str: Option<String>,
}

pub fn parse_quick_add(input: &str) -> QuickAdd {
    let mut title_parts = Vec::new();
    let mut tags = Vec::new();
    let mut time_str = None;

    for word in input.split_whitespace() {
        if word.starts_with('@') && word.len() > 1 {
            tags.push(word[1..].to_string());
        } else if word.starts_with('~') && word.len() > 1 {
            time_str = Some(word[1..].to_string());
        } else {
            title_parts.push(word);
        }
    }

    QuickAdd {
        title: title_parts.join(" "),
        tags,
        time_str,
    }
}
