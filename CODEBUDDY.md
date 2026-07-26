# CODEBUDDY.md

This file provides guidance to CodeBuddy Code when working with code in this repository.

## Project Overview

`gtp` is a GTD (Getting Things Done) terminal task manager written in Rust. It is a Cargo binary (package name `gtp`, edition 2021) combining a data layer (SQLite) with both a CLI and an interactive TUI. Its distinguishing design is **time-datafication**: nearly every state of a task has an associated UTC-millisecond timestamp, and every transition is written to an append-only event timeline.

## Build, Run, and Test Commands

Standard Cargo workflow — no custom build scripts, Makefile, or CI config exist.

- Build: `cargo build` (debug) / `cargo build --release`
- Run: `cargo run -- <subcommand> ...` (e.g. `cargo run -- capture "buy milk" --tag home`)
- Lint: `cargo clippy`
- Format: `cargo fmt`
- Check (no build): `cargo check`
- Tests: `cargo test` — note there are currently **no** `#[test]`/`#[cfg(test)]` modules in `src/`, so this only runs integration tests if any are added. To run a single future test: `cargo test <test_name>`.

There is no `tests/` directory. If you add tests, `tempfile` is already a dependency for temp DBs.

## Architecture

Entry flow: `src/main.rs` parses CLI (`clap`), opens the SQLite connection, and dispatches to `commands::run`. The default command (when none is given) is `Tui`, launching the interactive UI.

Layered design (each layer depends only inward):

- **`cli.rs`** — `clap` `Cli`/`Command` definitions. One `Command` variant per user action.
- **`commands/`** — thin handlers that translate a `Command` into repo/model calls and print output. `mod.rs::run` is the central dispatch; `effective_due()` (mod.rs:90) computes the sortable due time for recurring tasks.
- **`repo/`** — data-access layer over `rusqlite`. `tasks.rs` holds the bulk of domain logic (`create_capture`, `transition`, `schedule`, `assign_project`, `list`, id resolution). `tags.rs` manages the tag catalog and task↔tag links. `pomodoro.rs` persists Pomodoro timer state to a JSON file (not the DB). `mod.rs::log_event` is the shared audit-log writer.
- **`model/`** — plain data structures: `task` (Status/TaskKind/ProjectType enums + `Task`), `tag`, `event` (event-type string consts), `pomodoro` (Phase + PomoState).
- **`db/`** — `conn.rs::open` resolves `~/.config/gtp/gtp.db` via `dirs::config_dir()`, creates it, and runs migrations. `migrate.rs` applies migrations idempotently keyed off SQLite `user_version` (v1 and v2).
- **`time.rs`** — time utilities. `parse_time` parses human input (`now`, `+2h`, `today`, `2026-07-24 14:30`, etc.) into UTC ms. `rrule_occurrences` is a self-contained RRULE expander (DAILY/WEEKLY/MONTHLY with INTERVAL/COUNT/UNTIL/BYDAY) — no external crate. All timestamps are stored as UTC ms; display uses `format_local` (local timezone).
- **`parser.rs`** — `parse_quick_add`: splits input into title/tags (`@tag`) and time (`~time`).
- **`tui/`** — ratatui/crossterm interactive UI. `mod.rs::run` owns the terminal loop; `app.rs` (App/Pane/View state), `handlers.rs` (key handling), `render.rs` (drawing), `ui.rs`, `calendar.rs`. UI strings are in Chinese.

### Key domain concepts (require reading multiple files)

- **Status lifecycle** (`model/task.rs`): `Inbox → Next / Scheduled / Waiting / Someday / Reference → Done`, with `Archive` as soft-delete (`archived_at` set; list queries filter `archived_at IS NULL`). Transition rules live in `repo/tasks.rs::transition`, which also sets the corresponding `*_at` timestamp (clarified/organized/started/completed) — this is the "time-datafication."
- **Tasks as a tree**: actions have `parent_id` → a project (a `Task` with `kind='project'`, `project_type` Parallel vs Sequential). `resolve_project` accepts an id, unique id-prefix, or exact title.
- **Recurring tasks / habits**: a task with `rrule` set, when marked `Done`, is rescheduled to its next RRULE occurrence instead of staying done (`transition`, tasks.rs:176). `effective_due` is what sorting/filtering uses for these.
- **Tags**: a preset system set is seeded (migrations/0002): context tags (`home`,`work`,`learning`,`errands`,`calls`,`computer`) and priority tags (`p1`/`p2`/`p3`). Custom tags are auto-created on first use (`find_or_create_tag`). Tagging writes `tag_added`/`tag_removed` events.
- **Event timeline**: `task_events` is append-only; `show <id>` renders it. Event-type string consts are in `model/event.rs` and must stay in sync with the schema comments in `migrations/0001_init.sql`.
- **Pomodoro**: state lives in `~/.config/gtp/pomo.json`. `pomo start` spawns a background `gtp pomo daemon` process that ticks every second, logs `pomodoro` events, and sends desktop notifications. `pomo waybar` emits JSON for a waybar module.

## Conventions to respect when editing

- Add new DB columns via a **new migration file** + bump in `migrate.rs`, never by editing existing migration SQL. `migrate.rs` is idempotent via `user_version`.
- New event types must be added both as a const in `model/event.rs` and documented in the `task_events` comment in `migrations/0001_init.sql`.
- All time math should go through `time.rs`; store UTC ms (INTEGER), never formatted strings.
- The DB path and Pomodoro JSON path both live under `~/.config/gtp/` via `dirs::config_dir()` — don't hardcode paths elsewhere.
- Error type is `crate::error::Error` (thiserror) for domain errors; commands propagate `anyhow::Result`.
