//! Read-only importer for VRCX. This never writes to VRCX's SQLite database.
use anyhow::{bail, Result};
use rusqlite::{params, Connection};
use std::path::PathBuf;

fn vrcx_database() -> Option<PathBuf> {
    let roaming = dirs::config_dir()?.join("VRCX");
    let config = roaming.join("VRCX.json");
    if let Ok(text) = std::fs::read_to_string(config) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(path) = value
                .get("VRCX_DatabaseLocation")
                .and_then(|v| v.as_str())
                .filter(|p| !p.is_empty())
            {
                return Some(PathBuf::from(path));
            }
        }
    }
    let default = roaming.join("VRCX.sqlite3");
    default.exists().then_some(default)
}

pub fn import(app: &Connection) -> Result<usize> {
    let Some(path) = vrcx_database() else {
        return Ok(0);
    };
    let source = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let table = source.query_row(
        "SELECT name FROM sqlite_master WHERE type='table' AND name LIKE '%_friend_log_current' LIMIT 1", [], |r| r.get::<_, String>(0)
    ).map_err(|_| anyhow::anyhow!("VRCX 尚未生成好友资料表；请在 VRCX 登录并启用好友日志。"))?;
    let prefix = table.trim_end_matches("_friend_log_current");
    let mut count = 0;
    let players_query = format!("SELECT user_id,display_name,trust_level FROM {table}");
    let mut players_statement = source.prepare(&players_query)?;
    let mut rows = players_statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })?;
    for row in rows.by_ref() {
        let (id, name, trust) = row?;
        app.execute(
            "INSERT INTO players(user_id,display_name,trust_level,source,last_synced_at) VALUES(?1,?2,?3,'vrcx',datetime('now'))
             ON CONFLICT(user_id) DO UPDATE SET display_name=excluded.display_name,trust_level=excluded.trust_level,source='vrcx',last_synced_at=datetime('now')",
            params![id, name, trust],
        )?;
        count += 1;
    }
    let history = format!("{prefix}_friend_log_history");
    let exists: bool = source.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [history.as_str()],
        |r| r.get(0),
    )?;
    if exists {
        let sql = format!("SELECT user_id,display_name,previous_display_name,created_at FROM {history} WHERE display_name IS NOT NULL");
        let mut history_statement = source.prepare(&sql)?;
        let mut history_rows = history_statement.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        for row in history_rows.by_ref() {
            let (id, name, previous, at) = row?;
            app.execute("INSERT OR IGNORE INTO display_name_history(user_id,display_name,previous_display_name,changed_at) VALUES(?1,?2,?3,?4)", params![id, name, previous, at])?;
        }
    }
    Ok(count)
}

pub fn ensure_available() -> Result<()> {
    if vrcx_database().is_none() {
        bail!("找不到 VRCX.sqlite3；请在设置中配置 VRCX 数据库路径。")
    }
    Ok(())
}
