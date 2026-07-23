use rusqlite::Connection;

/// Run schema + seed migrations. Both are idempotent (IF NOT EXISTS /
/// INSERT OR IGNORE) so this is safe to call on every startup.
pub fn run(conn: &mut Connection) -> anyhow::Result<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    let sql1 = include_str!("../../migrations/0001_init.sql");
    conn.execute_batch(sql1)?;
    let sql2 = include_str!("../../migrations/0002_seed_tags.sql");
    conn.execute_batch(sql2)?;
    Ok(())
}
