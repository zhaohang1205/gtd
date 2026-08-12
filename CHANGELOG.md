# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Open-source release: MIT `LICENSE`, `README.md`, `CHANGELOG.md`, Cargo
  metadata (`license`, `repository`, `rust-version`), and GitHub Actions CI +
  release workflows.
- Count caching for the guide sidebar (`App::counts`) so rendering performs
  zero database queries per frame; one-pass today/tomorrow list computation
  (`day_lists`) with a single RRULE expansion per recurring task; batched tag
  fetch (`get_tags_for_tasks`) replacing per-row queries.

### Changed

- `README.md` is now a bilingual (中文/English) user manual.
- Removed the stale `CODEBUDDY.md`; `AGENTS.md` is the single authoritative
  contributor guide, with test locations and CI usage corrected.

### Fixed

- Checklist-adding keybinding was unreachable (`Shift+K` shadowed by the
  Tomorrow view). `Shift+C` now adds checklist items, and the pomodoro-length
  configuration moved to `[`; help/syntax panels updated.
- Due-notification checks mixed milliseconds and seconds, so 1h/10m/due-now
  desktop notifications never fired. The check now uses a consistent seconds
  scale and queries only tasks within the relevant window (`due_in_range`).
- TUI tests read the live `pomo.json`, so a running `gtp pomo daemon` made the
  rendering tests fail. Added a test-only idle override
  (`set_pomo_idle_for_tests`).
- `relative_due` / `relative_past` built strings via repeated `replace`;
  switched to a single `format!`-style substitution.

### Performance

- `check_notifications` no longer scans the full task table on every tick;
  it selects only tasks with `due_at` in the ±1h window.
- `refresh()` loads the visible list and all its tag names in two queries
  instead of one query per row.

## [0.1.0] - unreleased

Initial development build of the GTD terminal task manager (see git history for
the full list of `feat`/`fix`/`refactor` commits).
