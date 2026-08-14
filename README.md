# gtp — GTD 终端任务管理器 / GTD Terminal Task Manager

[![CI](https://github.com/zhaohang1205/gtd/actions/workflows/ci.yml/badge.svg)](https://github.com/zhaohang1205/gtd/actions/workflows/ci.yml)
[![Release](https://github.com/zhaohang1205/gtd/actions/workflows/release.yml/badge.svg)](https://github.com/zhaohang1205/gtd/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

用 Rust 写成的 GTD 终端任务管理器：SQLite 数据层 + CLI + ratatui TUI 三合一。
A GTD terminal task manager in Rust: SQLite data layer + CLI + ratatui TUI in one binary.

核心设计是 **时间数据化（time-datafication）**：每次任务状态变更都打上 UTC 毫秒时间戳，
追加到只写(append-only)的 `task_events` 时间线，每个任务都带完整的履历。
Core idea: **time-datafication** — every state change is stamped with UTC-ms and appended to
an append-only `task_events` timeline, giving every task a full audit trail.

## 功能特性 / Features

- 完整 GTD 流程：收件箱 → 下一步/已排程/等待中/将来也许/参考资料 → 已完成，含周回顾向导与今日/明日视图 · Full GTD workflow with weekly review and today/tomorrow views
- 循环任务：RRULE 支持 + 快捷简写（`*2w[1,3]`），完成后自动重排 · Recurring tasks auto-reschedule on Done
- 标签系统：情境标签 + `p1/p2/p3` 优先级，自定义标签自动创建 · Tag system with auto-created custom tags
- 检查单：一键勾选，全部完成自动重置 · Checklists with one-key tick and auto-reset
- 番茄钟专注模式：全屏倒计时环、连击、桌面通知、waybar 模块 · Pomodoro focus mode with progress ring, streaks and a waybar module
- 中英双语界面（`F6` 切换）、Catppuccin 深/浅主题（`F5` 切换） · Bilingual UI (F6) and Catppuccin themes (F5)

## 安装 / Installation

需要 Rust 1.89+，SQLite 已内置，无系统依赖。Requires Rust 1.89+; SQLite is bundled.

```sh
cargo install --git https://github.com/zhaohang1205/gtd
# 或本地构建 / or build from source:
git clone https://github.com/zhaohang1205/gtd.git && cd gtd
cargo build --release
```

数据目录：`~/.config/gtp/`（`gtp.db` + `pomo.json`）。Data lives in `~/.config/gtp/`.

## 快速开始 / Quick start

```sh
gtp                                # 启动 TUI / launch the TUI
gtp capture "买牛奶" --tag home     # 捕获进收件箱 / capture into the inbox
gtp list --status next             # 列出下一步 / list next actions
gtp show <task-id>                 # 查看完整时间线 / full event timeline
```

任务引用支持完整 id、唯一 id 前缀（类似 git）、或精确标题。Task refs accept a full id, a unique
id-prefix, or an exact title.

## CLI

| 命令 / Command | 说明 / Description |
| --- | --- |
| `gtp` | 启动 TUI / Launch the TUI |
| `gtp capture <title> [--tag T]... [--due TIME] [--status S] [--p1\|--p2\|--p3] [--json]` | 捕获新任务 / Capture |
| `gtp list [--status S] [--tag T]... [--due-before TIME] [--json]` | 列出任务 / List tasks |
| `gtp show <id> [--json]` | 任务详情 + 时间线 / Show with timeline |
| `gtp next\|wait\|someday\|done <id>` | 流转状态 / Move between statuses |
| `gtp schedule <id> [--start TIME] [--end TIME] [--rrule R]` | 排期（可加循环）/ Schedule (+recurrence) |
| `gtp archive <id>` / `gtp restore <id>` | 软删除 / 恢复 / Soft delete / restore |
| `gtp tag <id> <name>` / `gtp untag <id> <name>` | 增删标签 / Manage tags |
| `gtp review` | 周回顾 / Weekly review |
| `gtp tags` | 标签库 / List tags |
| `gtp pomo start <id> \| stop \| daemon \| waybar` | 番茄钟 / Pomodoro |
| `gtp alarm waybar [slot] \| next [slot]` | 到期提醒 / Upcoming-task reminders |

## TUI 快捷键 / Keybindings

| 键 / Key | 说明 / Action |
| --- | --- |
| `h` / `l` | 切换面板（引导/列表/详情）· switch pane |
| `j` / `k` | 上下移动 · move up/down |
| `1`-`9` | 切换视图（8=归档，9=标签库）· switch view |
| `⇧J` / `⇧K` | 今日 / 明日 · today / tomorrow |
| `/` | 全局搜索 · global search |
| `f` | 情境过滤 · tag filter |
| `a` | 快速捕获 · quick capture |
| `Enter` | 组织/编辑：一句话补全 @标签 ~时间 *周期 · organize/edit (@tags ~time *rrule) |
| `x` / `w` / `s` | 已完成 / 等待中 / 将来也许 · done / waiting / someday |
| `C` | 新增检查单 · add checklist item |
| `Space` | 勾选检查单 / 继续番茄 · tick / continue pomodoro |
| `e` / `d` / `L` / `W` | 编辑标题 / 截止 / 循环 / 委派 · edit title/due/rrule/delegated |
| `n` | 编辑长备注（`$EDITOR`）· edit notes |
| `P` / `S` / `[` | 开始 / 停止番茄 / 番茄时长配置 · pomodoro start/stop/config |
| `A` / `D` | 归档（y 确认 / n 取消）· archive (y/n) |
| `u` | 恢复归档 · restore from archive |
| `r` / `R` | 周回顾（开始 / 下一步）· weekly review (start/next) |
| `F5` / `F6` | 主题 / 语言 · theme / language |
| `F1` 或 `?` | 快捷键帮助 · shortcut help |
| `q` | 退出 · quit |

## 时间与循环语法 / Time & recurrence syntax

```
now  +2h  +30m  +1d  +1w          相对偏移 / relative offsets
+3d 15:30                         相对偏移 + 时刻 / offset + clock
今天 / 明天 / 后天 [HH:MM]         中文天词 / Chinese day words
周三 / 下周五 [HH:MM]              星期几（可带"下周"）/ weekday (+next week)
8/20 15:30 · 2026.8.20            斜杠/点日期 / slash & dot dates
HH:MM                             当日时刻（已过则视为明日）/ same-day time
2026-07-24 [HH:MM]                绝对日期时间 / absolute date & time
```

一句话里的 `~time` 设**排程起点**（`scheduled_start_at`，状态进入已排程，只设起点不设终点）；`d` 键/`--due` 设软截止（`due_at`）。

循环 RRULE（一句话里 `*` 简写）：`FREQ=DAILY|WEEKLY|MONTHLY`、`INTERVAL=2`、
`BYDAY=SA,SU`、`COUNT=10` / `UNTIL=YYYY-MM-DD`。
快速简写：`*d`/`*w`/`*m`/`*y`（每天/周/月/年）、`*2w[1,3]`（每两周周一、周三，1-7=周一至周日，0=周日），优先级 `!a`/`!b`/`!c`。

## 开发 / Development

```sh
cargo test                     # 测试 / tests
cargo clippy -- -D warnings    # 静态检查（须零警告）/ lint (must stay clean)
cargo fmt --check              # 格式 / formatting
```

架构说明见 [AGENTS.md](AGENTS.md)。See AGENTS.md for architecture and contributor rules.

## 许可证 / License

MIT，见 [LICENSE](LICENSE)。
