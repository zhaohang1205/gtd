use std::path::PathBuf;

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
    migrate::run(&mut conn)?;
    Ok(conn)
}
