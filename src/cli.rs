use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "gtp", version, about = "GTD terminal task manager", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Capture a new item into the inbox
    Capture {
        title: String,
        #[arg(long)]
        tag: Vec<String>,
        #[arg(long)]
        p1: bool,
        #[arg(long)]
        p2: bool,
        #[arg(long)]
        p3: bool,
        #[arg(long)]
        due: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List tasks (optional filters)
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        tag: Vec<String>,
        #[arg(long)]
        due_before: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show a task with its full event timeline
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Mark actionable (next)
    Next { id: String },
    /// Mark waiting-for
    Wait { id: String },
    /// Schedule with a planned start (and optional --rrule)
    Schedule {
        id: String,
        #[arg(long)]
        start: Option<String>,
        #[arg(long)]
        end: Option<String>,
        #[arg(long)]
        rrule: Option<String>,
    },
    /// Move to someday/maybe
    Someday { id: String },
    /// Mark done
    Done { id: String },
    /// Archive (soft delete)
    Archive { id: String },
    /// Restore a previously archived (soft-deleted) task
    Restore { id: String },
    /// Add a tag to a task (preset or custom)
    Tag { id: String, name: String },
    /// Remove a tag from a task
    Untag { id: String, name: String },
    /// Weekly review helper
    Review,
    /// List all tags grouped by category
    Tags,
    /// Pomodoro commands (start, stop, daemon, waybar)
    Pomo {
        action: String,
        task_id: Option<String>,
    },
    /// Launch the interactive TUI
    Tui,
}
