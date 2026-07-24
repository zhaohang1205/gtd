import sys

def main():
    # 1. Update src/repo/tasks.rs for rrule
    with open("src/repo/tasks.rs", "r") as f:
        repo_code = f.read()
        
    repo_code = repo_code.replace("""pub struct CaptureInput {
    pub title: String,
    pub kind: task::TaskKind, // 'action' | 'project'
    pub parent_id: Option<String>,
    pub status: task::Status,
    pub due_at: Option<i64>,
    pub tag_names: Vec<String>,
    pub delegated_to: Option<String>,
    pub project_type: Option<task::ProjectType>,
    pub checklist: Vec<task::ChecklistItem>,
}""", """pub struct CaptureInput {
    pub title: String,
    pub kind: task::TaskKind, // 'action' | 'project'
    pub parent_id: Option<String>,
    pub status: task::Status,
    pub due_at: Option<i64>,
    pub tag_names: Vec<String>,
    pub delegated_to: Option<String>,
    pub project_type: Option<task::ProjectType>,
    pub checklist: Vec<task::ChecklistItem>,
    pub rrule: Option<String>,
}""")

    repo_code = repo_code.replace("""            delegated_to: None,
            project_type: None,
            checklist: Vec::new(),
        }""", """            delegated_to: None,
            project_type: None,
            checklist: Vec::new(),
            rrule: None,
        }""")

    repo_code = repo_code.replace("""    tx.execute(
        "INSERT INTO tasks \\
         (id,title,notes,kind,parent_id,status,rrule,created_at,clarified_at,organized_at,due_at,updated_at,delegated_to,project_type,checklist) \\
         VALUES (?1,?2,'',?3,?4,?5,NULL,?6,?7,?8,?9,?10,?11,?12,?13)",""", """    tx.execute(
        "INSERT INTO tasks \\
         (id,title,notes,kind,parent_id,status,rrule,created_at,clarified_at,organized_at,due_at,updated_at,delegated_to,project_type,checklist) \\
         VALUES (?1,?2,'',?3,?4,?5,?14,?6,?7,?8,?9,?10,?11,?12,?13),""")

    repo_code = repo_code.replace("""            input.delegated_to,
            pt_str,
            cl_str
        ],""", """            input.delegated_to,
            pt_str,
            cl_str,
            input.rrule,
        ],""")

    with open("src/repo/tasks.rs", "w") as f:
        f.write(repo_code)

    # 2. Update src/tui/mod.rs for rrule in parse_nlp_capture, and fix 'R' vs 'r'
    with open("src/tui/mod.rs", "r") as f:
        mod_code = f.read()

    nlp_orig = """fn parse_nlp_capture(input: &str) -> (String, Vec<String>, Option<String>, Option<i64>) {
    let mut title_parts = Vec::new();
    let mut tags = Vec::new();
    let mut project = None;
    let mut time_str = Vec::new();

    for token in input.split_whitespace() {
        if token.starts_with('@') && token.len() > 1 {
            tags.push(token[1..].to_string());
        } else if token.starts_with('+') && token.len() > 1 {
            project = Some(token[1..].to_string());
        } else if token.starts_with('~') && token.len() > 1 {
            time_str.push(token[1..].to_string());
        } else {
            title_parts.push(token);
        }
    }

    let title = title_parts.join(" ");
    let time = if !time_str.is_empty() {
        crate::time::parse_time(&time_str.join(" ")).ok()
    } else {
        None
    };

    (title, tags, project, time)
}"""

    nlp_new = """fn parse_nlp_capture(input: &str) -> (String, Vec<String>, Option<String>, Option<i64>, Option<String>) {
    let mut title_parts = Vec::new();
    let mut tags = Vec::new();
    let mut project = None;
    let mut time_str = Vec::new();
    let mut rrule = None;

    for token in input.split_whitespace() {
        if token.starts_with('@') && token.len() > 1 {
            tags.push(token[1..].to_string());
        } else if token.starts_with('+') && token.len() > 1 {
            project = Some(token[1..].to_string());
        } else if token.starts_with('~') && token.len() > 1 {
            time_str.push(token[1..].to_string());
        } else if token.starts_with(";FREQ=") || token.starts_with("rrule:") {
            let rr = if token.starts_with("rrule:") { &token[6..] } else { token };
            rrule = Some(rr.to_string());
        } else {
            title_parts.push(token);
        }
    }

    let title = title_parts.join(" ");
    let time = if !time_str.is_empty() {
        crate::time::parse_time(&time_str.join(" ")).ok()
    } else {
        None
    };

    (title, tags, project, time, rrule)
}"""
    mod_code = mod_code.replace(nlp_orig, nlp_new)

    mod_code = mod_code.replace("""let (title, tags, project, due_at) = parse_nlp_capture(trimmed);""", """let (title, tags, project, due_at, rrule) = parse_nlp_capture(trimmed);""")
    
    mod_code = mod_code.replace("""tag_names: tags,
                                ..Default::default()""", """tag_names: tags,
                                rrule,
                                ..Default::default()""")

    mod_code = mod_code.replace("""KeyCode::Char('r') | KeyCode::Char('R') => {
                if self.review_step.is_none() {""", """KeyCode::Char('r') => self.set_view(View::Review),
            KeyCode::Char('R') => {
                if self.review_step.is_none() {""")
    
    with open("src/tui/mod.rs", "w") as f:
        f.write(mod_code)

if __name__ == '__main__':
    main()
