//! Read-only importer for VRCX. This never writes to VRCX's SQLite database.
use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VrcxStatus {
    pub detected: bool,
    pub path: Option<String>,
    pub message: String,
}

fn vrcx_database(app: &Connection) -> Option<PathBuf> {
    if let Ok(Some(path)) = crate::db::setting(app, "vrcx_database_path") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Some(path);
        }
    }
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
    let Some(path) = vrcx_database(app) else {
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
    let memo_table_exists: bool = source.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='memos')",
        [],
        |row| row.get(0),
    )?;
    if memo_table_exists {
        let mut statement = source.prepare("SELECT user_id,memo FROM memos")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (user_id, memo) = row?;
            app.execute(
                "UPDATE players SET vrcx_memo=?2 WHERE user_id=?1",
                params![user_id, memo],
            )?;
        }
    }
    Ok(count)
}

pub fn status(app: &Connection) -> VrcxStatus {
    match vrcx_database(app) {
        Some(path) => VrcxStatus {
            detected: true,
            path: Some(path.to_string_lossy().to_string()),
            message: "已找到 VRCX 数据库".into(),
        },
        None => VrcxStatus {
            detected: false,
            path: None,
            message: "找不到 VRCX.sqlite3；请手动配置路径".into(),
        },
    }
}
