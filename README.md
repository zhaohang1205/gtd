# gtp — GTD Terminal Task Manager

[![CI](https://github.com/zhaohang1205/gtd/actions/workflows/ci.yml/badge.svg)](https://github.com/zhaohang1205/gtd/actions/workflows/ci.yml)
[![Release](https://github.com/zhaohang1205/gtd/actions/workflows/release.yml/badge.svg)](https://github.com/zhaohang1205/gtd/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A GTD (Getting Things Done) task manager for the terminal, written in Rust.
It combines a **SQLite data layer**, a **CLI**, and a **ratatui TUI** into one
binary.

The core design idea is **time-datafication**: every task state change is
stamped with a UTC-ms timestamp and appended to an append-only `task_events`
timeline, so every task carries a full audit trail of its life.

## Features

- **Full GTD workflow** — Inbox → Next / Scheduled / Waiting / Someday /
  Reference → Done, with weekly review wizard and today/tomorrow views.
- **Time-datafication** — append-only event timeline per task (`created`,
  `status_change`, `completed`, `archived`, pomodoro counts, habit reschedules).
- **Recurring tasks / habits** — RRULE support (`FREQ=DAILY|WEEKLY|MONTHLY`,
  `INTERVAL`, `COUNT`, `UNTIL`, `BYDAY`) plus a quick shorthand like
  `*2w[1,3]` (every 2 weeks Mon/Wed). Marking one `Done` auto-reschedules it.
- **Tag system** — context tags (`@home`, `@work` …) with preset priorities
  (`p1`/`p2`/`p3`); custom tags auto-create on first use.
- **Checklists** — sub-items with one-key tick, auto-reset when all done.
- **Pomodoro focus mode** — full-screen focus timer with a braille progress
  ring, streaks, desktop notifications, a background daemon and a waybar module.
- **Alarm / waybar reminders** — upcoming-task reminders with 5-minute lead.
- **i18n** — Chinese by default, English toggle with `F6` (persisted).
- **Themes** — Catppuccin Mocha (dark) / Latte (light) toggled with `F5`.

## Installation

Requires Rust 1.89+. SQLite is bundled, so there are no system dependencies.

```sh
cargo install --git https://github.com/zhaohang1205/gtd
# or build from source
git clone https://github.com/zhaohang1205/gtd.git && cd gtd
cargo build --release
```

Data lives in `~/.config/gtp/` (`gtp.db` + `pomo.json`).

## Quick start

```sh
gtp                                # launch the TUI
gtp capture "buy milk" --tag home  # capture into the inbox
gtp list --status next             # list next actions
gtp show <task-id>                 # full event timeline
```

Task references accept a full id, a unique id-prefix (like git), or an exact title.

## CLI

| Command | Description |
| --- | --- |
| `gtp` | Launch the interactive TUI |
| `gtp capture <title> [--tag T]... [--due TIME] [--status S] [--p1\|--p2\|--p3] [--json]` | Capture a new item |
| `gtp list [--status S] [--tag T]... [--due-before TIME] [--json]` | List tasks |
| `gtp show <id> [--json]` | Show a task with its event timeline |
| `gtp next <id>` / `wait` / `someday` / `done` | Move between statuses |
| `gtp schedule <id> [--start TIME] [--end TIME] [--rrule R]` | Schedule (with optional recurrence) |
| `gtp archive <id>` / `restore <id>` | Soft delete / restore |
| `gtp tag <id> <name>` / `untag <id> <name>` | Manage tags |
| `gtp review` | Weekly review helper |
| `gtp tags` | List all tags grouped by category |
| `gtp pomo start <id> \| stop \| daemon \| waybar` | Pomodoro |
| `gtp alarm waybar [slot] \| next [slot]` | Upcoming-task reminders |

## TUI keybindings

| Key | Action |
| --- | --- |
| `h` / `l` | Switch pane (guide / list / detail) |
| `j` / `k` | Move up / down the list |
| `1`-`9` | Switch view (8 = archive, 9 = tags) |
| `⇧J` / `⇧K` | Today / Tomorrow view |
| `/` | Global search (title & notes) |
| `f` | Context / tag filter |
| `a` | Quick capture |
| `Enter` | Clarify / mark next action |
| `x` / `w` / `s` | Done / Waiting / Someday |
| `c` | Calendar schedule |
| `C` | Add checklist item |
| `Space` | Tick checklist / continue pomodoro |
| `e` / `d` / `L` / `W` | Edit title / due / rrule / delegated |
| `n` | Edit long notes (`$EDITOR`) |
| `P` / `S` | Start / stop pomodoro |
| `[` | Customize pomodoro lengths (work;short;long) |
| `A` / `D` | Archive (y confirm / n cancel) |
| `u` | Restore from archive |
| `r` / `R` | Weekly review (start / next step) |
| `F5` / `F6` | Toggle theme / language |
| `F1` or `?` | Toggle shortcut help |
| `q` | Quit |

## Time & recurrence syntax

Set a due/scheduled time with human-friendly strings:

```
now                    now
+2h  +30m  +1d  +1w    relative offsets
today / tomorrow [HH:MM]
HH:MM                  same-day time (or tomorrow if already past)
2026-07-24 [HH:MM]     absolute date & time
```

Recurrence (RRULE) — used in the schedule prompt, after `;`:

```
FREQ=DAILY|WEEKLY|MONTHLY
INTERVAL=2            every 2 weeks
BYDAY=SA,SU           days of week
COUNT=10 / UNTIL=YYYY-MM-DD
```

Quick-capture shorthand: `*2w[1,3]` = every 2 weeks Mon & Wed (days 1–7,
0 = Sunday), or `*mo,we`. Priority via `!a` / `!b` / `!c`.

## Development

```sh
cargo test                 # run tests
cargo clippy -- -D warnings # lint (must stay clean)
cargo fmt --check          # formatting
```

## Architecture

- `cli.rs` — clap command definitions.
- `commands/` — thin handlers for each CLI action (`pomo.rs` runs the daemon).
- `repo/` — rusqlite data access; `tasks.rs` holds most domain logic; every
  change is appended to the `task_events` timeline (`mod.rs::log_event`).
- `model/` — plain structs + enums.
- `db/` — connection + versioned, idempotent migrations.
- `time.rs` — human time parsing, RRULE expansion, local formatting.
- `tui/` — ratatui app (guide / list / detail / calendar / pomodoro focus).

## License

MIT. See [LICENSE](LICENSE).
