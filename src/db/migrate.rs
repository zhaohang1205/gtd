use rusqlite::Connection;

/// Run schema + seed migrations. Both are idempotent (IF NOT EXISTS /
/// INSERT OR IGNORE) so this is safe to call on every startup.
pub fn run(conn: &mut Connection) -> anyhow::Result<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?;

    let current_version: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;

    if current_version < 1 {
        let sql1 = include_str!("../../migrations/0001_init.sql");
        conn.execute_batch(sql1)?;
        let sql2 = include_str!("../../migrations/0002_seed_tags.sql");
        conn.execute_batch(sql2)?;
        conn.pragma_update(None, "user_version", 1)?;
    }

    if current_version < 2 {
        let sql3 = include_str!("../../migrations/0003_add_gtd_advanced.sql");
        conn.execute_batch(sql3)?;
        conn.pragma_update(None, "user_version", 2)?;
    }

    Ok(())
}
