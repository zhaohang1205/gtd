use std::path::PathBuf;
use std::time::Duration;

use rusqlite::Connection;

use crate::db::migrate;

/// Open (creating if needed) the gtp SQLite database under the XDG config dir
/// (`~/.config/gtp/gtp.db`) and run migrations.
pub fn open() -> anyhow::Result<Connection> {
    let dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gtp");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("gtp.db");
    let mut conn = Connection::open(path)?;

    // 先设 busy_timeout，让随后的 journal_mode 切换（需要写锁）也能等待而不是
    // 立即报 `database is locked`；WAL 让 CLI/TUI/pomo 多进程并发读不互相阻塞。
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;

    migrate::run(&mut conn)?;
    Ok(conn)
}
