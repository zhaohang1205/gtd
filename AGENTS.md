# AGENTS.md

GTD terminal task manager (`gtp`) — a Rust binary (edition 2021, **no lib target**) combining a SQLite data layer, a CLI, and a ratatui TUI. Core design idea: **time-datafication** — every task state change is stamped with a UTC-ms timestamp and appended to an append-only `task_events` timeline.

## Commands

- Build/run: `cargo run -- capture "buy milk" --tag home` — note the `--` before subcommand args.
- Test: `cargo test` — unit tests live only in `src/tui/mod.rs`; run one with `cargo test <name>`.
- Lint/format: `cargo clippy`, `cargo fmt`.
- No CI, Makefile, or build scripts. `tempfile` is available for test DBs.

## Architecture (layers depend only inward)

- `cli.rs` — clap `Command` enum, one variant per CLI action. Default command (no args) is `Tui`.
- `commands/` — thin handlers; `mod.rs::run` dispatches. `pomo.rs` handles the daemon/waybar logic.
- `repo/` — rusqlite data access; `tasks.rs` holds most domain logic (`create_capture`, `transition`, `schedule`, `resolve_project`, `list`). `mod.rs::log_event` writes the audit timeline.
- `model/` — plain structs + enums; `event.rs` holds event-type string consts.
- `db/` — `conn.rs::open` resolves `~/.config/gtp/gtp.db` via `dirs::config_dir()`, then runs migrations keyed off SQLite `user_version`.
- `time.rs` — `parse_time` (human input: `now`, `+2h`, `today`, `2026-07-24 14:30` → UTC ms), self-contained `rrule_occurrences` (no external crate), `format_local`.
- `parser.rs` — `parse_quick_add`: splits input into `@tag` words and `~time` words.
- `tui/` — `app.rs`, `handlers.rs` (key handling), `render.rs`, `ui.rs`, `calendar.rs`, `theme.rs` (Catppuccin), `i18n.rs`. **UI strings are Chinese by default and localized via `crate::tr!` / `Lang` (F6 toggles to English, F5 toggles theme); never hardcode UI text.**

## Non-obvious rules (violating these breaks things)

- **Timestamps**: store UTC ms INTEGER, never formatted strings. All time math goes through `time.rs`; display via `format_local`.
- **Migrations**: never edit existing `migrations/*.sql`. Add a new file plus a new version block in `migrate.rs` (currently v1 = 0001+0002, v2 = +0003, v3 = +0004, v4 = +0005, v5 = +0006). Migration SQL is idempotent (IF NOT EXISTS / INSERT OR IGNORE).
- **New event types**: add a const in `model/event.rs` AND keep the `task_events` comment in `migrations/0001_init.sql` in sync.
- **DB paths**: `~/.config/gtp/gtp.db` and `~/.config/gtp/pomo.json` both derive from `dirs::config_dir()` — never hardcode.
- **ID resolution**: commands accept a task id, a unique id-prefix, or an exact title (`resolve_project`).
- **Archive is soft-delete**: sets `archived_at` and `archive_reason` (`completed` when the task was Done at archive time, else `deleted`); list queries filter `archived_at IS NULL`. `Restore` clears both. The Archived view shows reason + archive time, never the old status or overdue.
- **Recurring tasks**: a task with `rrule` reschedules to its next occurrence on `Done` instead of completing. Sorting/filtering uses `effective_due` (`commands/mod.rs`), not the raw due column.
- **Status lifecycle**: `Inbox → Next / Scheduled / Waiting / Someday / Reference → Done`; `transition` in `repo/tasks.rs` sets the matching `*_at` timestamp.
- **Pomodoro**: `pomo start` spawns a background `gtp pomo daemon` (ticks every second, writes `pomo.json`, sends `notify-send`). `pomo waybar` emits JSON for a waybar module. `kill_daemon` (pomo.rs) deliberately waits for the old process to exit — concurrent daemons corrupt `pomo.json`; don't "optimize" that away.
- **Tags**: system presets seeded in migrations (`home`, `work`, `learning`, `errands`, `calls`, `computer`, `quick`, `focus`; priorities `p1`/`p2`/`p3`). Custom tags auto-create on first use (`find_or_create_tag`).

## Errors

Domain errors use `crate::error::Error` (thiserror); command handlers return `anyhow::Result`.

## Existing docs

`CODEBUDDY.md` has per-file detail but is partially stale (migration version, test locations). Treat this file as authoritative.
