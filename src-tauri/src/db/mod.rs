use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    pub user_id: String,
    pub display_name: String,
    pub profile_pic_url: Option<String>,
    pub avatar_thumbnail_url: Option<String>,
    pub trust_level: Option<String>,
    pub source: String,
    pub previous_names: Vec<String>,
    pub photo_count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Photo {
    pub id: i64,
    pub user_id: Option<String>,
    pub source: String,
    pub local_path: Option<String>,
    pub remote_url: Option<String>,
    pub thumbnail_path: Option<String>,
    pub file_name: String,
    pub captured_at: Option<String>,
}

pub fn database_path() -> Result<PathBuf> {
    let dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("vrchat-photo-manager");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("photos.db"))
}

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
        CREATE TABLE IF NOT EXISTS players (
            user_id TEXT PRIMARY KEY, display_name TEXT NOT NULL,
            profile_pic_url TEXT, avatar_thumbnail_url TEXT, trust_level TEXT,
            source TEXT NOT NULL DEFAULT 'local', last_synced_at TEXT
        );
        CREATE TABLE IF NOT EXISTS display_name_history (
            id INTEGER PRIMARY KEY, user_id TEXT NOT NULL, display_name TEXT NOT NULL,
            previous_display_name TEXT, changed_at TEXT NOT NULL,
            UNIQUE(user_id, display_name, changed_at)
        );
        CREATE TABLE IF NOT EXISTS photos (
            id INTEGER PRIMARY KEY, user_id TEXT, source TEXT NOT NULL,
            local_path TEXT UNIQUE, vrchat_file_id TEXT,
            remote_url TEXT, thumbnail_path TEXT, file_name TEXT NOT NULL,
            captured_at TEXT, imported_at TEXT NOT NULL,
            UNIQUE(vrchat_file_id, user_id)
        );
        CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )?;
    Ok(conn)
}

pub fn list_players(conn: &Connection) -> Result<Vec<Player>> {
    let mut stmt = conn.prepare(
        "SELECT p.user_id,p.display_name,p.profile_pic_url,p.avatar_thumbnail_url,p.trust_level,p.source,
        (SELECT COUNT(*) FROM photos ph WHERE ph.user_id=p.user_id)
        FROM players p ORDER BY p.display_name COLLATE NOCASE",
    )?;
    let players = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get::<_, String>(5)?, row.get(6)?))
    })?.collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter().map(|(id, name, profile, avatar, trust, source, count)| {
            let mut names = conn.prepare("SELECT display_name FROM display_name_history WHERE user_id=?1 AND display_name != ?2 ORDER BY changed_at DESC LIMIT 10")?;
            let previous_names = names.query_map(params![id, name], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
            Ok(Player { user_id: id, display_name: name, profile_pic_url: profile, avatar_thumbnail_url: avatar, trust_level: trust, source, previous_names, photo_count: count })
        }).collect()
}

pub fn list_photos(conn: &Connection, user_id: &str) -> Result<Vec<Photo>> {
    let mut stmt = conn.prepare("SELECT id,user_id,source,local_path,remote_url,thumbnail_path,file_name,captured_at FROM photos WHERE user_id=?1 ORDER BY captured_at DESC, imported_at DESC")?;
    Ok(stmt.query_map([user_id], |r| Ok(Photo {
        id: r.get(0)?, user_id: r.get(1)?, source: r.get(2)?, local_path: r.get(3)?,
        remote_url: r.get(4)?, thumbnail_path: r.get(5)?, file_name: r.get(6)?, captured_at: r.get(7)?,
    }))?.collect::<rusqlite::Result<_>>()?)
}

pub fn setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn.query_row("SELECT value FROM settings WHERE key=?1", [key], |r| r.get(0)).optional()?)
}
