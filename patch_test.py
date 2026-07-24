import re

with open("src/tui/mod.rs", "r") as f:
    content = f.read()

# Update "Newtask:" to "Newtask"
content = content.replace('assert!(s.contains("Newtask:"), "收集提示");', 'assert!(s.contains("Newtask"), "收集提示");')

# Update "Project?" to "Project?" -> it's already "Project?", wait, let's check what the modal title is.
# Mode::PlanningProject => " Project? "
# So it's "Project?" which is same.

# What else could break? Let's fix unused warnings:
content = content.replace('const LONG_HELP: &str =', '#[allow(dead_code)]\nconst LONG_HELP: &str =')
content = content.replace('fn all_status_views', '#[allow(dead_code)]\n    fn all_status_views')

with open("src/tui/mod.rs", "w") as f:
    f.write(content)

