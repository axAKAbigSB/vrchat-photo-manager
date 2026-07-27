use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    pub user_id: String,
    pub display_name: String,
    pub profile_pic_url: Option<String>,
    pub avatar_thumbnail_url: Option<String>,
    pub trust_level: Option<String>,
    pub note: Option<String>,
    pub vrcx_memo: Option<String>,
    pub source: String,
    pub previous_names: Vec<String>,
    pub photo_count: i64,
    pub last_synced_at: Option<String>,
    pub is_friend: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Photo {
    pub id: i64,
    pub user_id: Option<String>,
    pub source: String,
    pub kind: String,
    pub local_path: Option<String>,
    pub remote_url: Option<String>,
    pub thumbnail_path: Option<String>,
    pub file_name: String,
    pub captured_at: Option<String>,
    pub people: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub album_folder: Option<String>,
    pub steam_screenshot_folder: Option<String>,
    pub vrcx_database_path: Option<String>,
    pub sync_interval_minutes: i64,
    #[serde(default = "default_true")]
    pub show_self_in_friends: bool,
}

fn default_true() -> bool {
    true
}

pub fn database_path() -> Result<PathBuf> {
    let dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("vrchat-photo-manager");
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
            note TEXT, vrcx_memo TEXT,
            source TEXT NOT NULL DEFAULT 'local', last_synced_at TEXT
        );
        CREATE TABLE IF NOT EXISTS display_name_history (
            id INTEGER PRIMARY KEY, user_id TEXT NOT NULL, display_name TEXT NOT NULL,
            previous_display_name TEXT, changed_at TEXT NOT NULL,
            UNIQUE(user_id, display_name, changed_at)
        );
        CREATE TABLE IF NOT EXISTS photos (
            id INTEGER PRIMARY KEY, user_id TEXT, source TEXT NOT NULL,
            kind TEXT NOT NULL DEFAULT 'album',
            local_path TEXT UNIQUE, vrchat_file_id TEXT,
            remote_url TEXT, thumbnail_path TEXT, file_name TEXT NOT NULL,
            captured_at TEXT, imported_at TEXT NOT NULL,
            UNIQUE(vrchat_file_id, user_id)
        );
        CREATE TABLE IF NOT EXISTS photo_people (
            photo_id INTEGER NOT NULL REFERENCES photos(id) ON DELETE CASCADE,
            user_id TEXT NOT NULL REFERENCES players(user_id) ON DELETE CASCADE,
            source TEXT NOT NULL DEFAULT 'manual',
            confirmed INTEGER NOT NULL DEFAULT 1,
            PRIMARY KEY(photo_id, user_id)
        );
        CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )?;
    add_column_if_missing(&conn, "players", "note", "TEXT")?;
    add_column_if_missing(&conn, "players", "vrcx_memo", "TEXT")?;
    add_column_if_missing(&conn, "players", "is_friend", "INTEGER NOT NULL DEFAULT 0")?;
    add_column_if_missing(&conn, "photos", "kind", "TEXT NOT NULL DEFAULT 'album'")?;
    conn.execute(
        "INSERT OR IGNORE INTO photo_people(photo_id,user_id,source,confirmed)
         SELECT id,user_id,'legacy',1 FROM photos WHERE user_id IS NOT NULL",
        [],
    )?;
    Ok(conn)
}

fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<()> {
    let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !columns.iter().any(|name| name == column) {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )?;
    }
    Ok(())
}

pub fn list_players(conn: &Connection) -> Result<Vec<Player>> {
    let mut stmt = conn.prepare(
        "SELECT p.user_id,p.display_name,p.profile_pic_url,p.avatar_thumbnail_url,p.trust_level,
        p.note,p.vrcx_memo,p.source,p.last_synced_at,p.is_friend,
        (SELECT COUNT(*) FROM photo_people pp WHERE pp.user_id=p.user_id)
        FROM players p ORDER BY p.display_name COLLATE NOCASE",
    )?;
    let players = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get(2)?,
            row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?,
            row.get::<_, String>(7)?, row.get(8)?, row.get::<_, i64>(9)? != 0, row.get(10)?,
        ))
    })?.collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter().map(|(id, name, profile, avatar, trust, note, memo, source, synced, is_friend, count)| {
            let mut names = conn.prepare("SELECT display_name FROM display_name_history WHERE user_id=?1 AND display_name != ?2 ORDER BY changed_at DESC LIMIT 10")?;
            let previous_names = names.query_map(params![id, name], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
            Ok(Player {
                user_id: id, display_name: name, profile_pic_url: profile,
                avatar_thumbnail_url: avatar, trust_level: trust, note,
                vrcx_memo: memo, source, previous_names, photo_count: count,
                last_synced_at: synced, is_friend,
            })
        }).collect::<Result<Vec<_>>>()?;
    Ok(players)
}

pub fn list_photos(
    conn: &Connection,
    user_id: Option<&str>,
    kind: Option<&str>,
) -> Result<Vec<Photo>> {
    let mut sql = String::from(
        "SELECT DISTINCT p.id,p.user_id,p.source,p.kind,p.local_path,p.remote_url,
         p.thumbnail_path,p.file_name,p.captured_at FROM photos p",
    );
    if user_id.is_some() {
        sql.push_str(" JOIN photo_people pp ON pp.photo_id=p.id");
    }
    sql.push_str(
        " WHERE (?1 IS NULL OR pp.user_id=?1) AND (?2 IS NULL OR p.kind=?2)
                   ORDER BY p.captured_at DESC,p.imported_at DESC",
    );
    if user_id.is_none() {
        sql = sql.replace(" WHERE (?1 IS NULL OR pp.user_id=?1)", " WHERE ?1 IS NULL");
    }
    let mut stmt = conn.prepare(&sql)?;
    let photos = stmt
        .query_map(params![user_id, kind], |r| {
            let id: i64 = r.get(0)?;
            Ok(Photo {
                id,
                user_id: r.get(1)?,
                source: r.get(2)?,
                kind: r.get(3)?,
                local_path: r.get(4)?,
                remote_url: r.get(5)?,
                thumbnail_path: r.get(6)?,
                file_name: r.get(7)?,
                captured_at: r.get(8)?,
                people: Vec::new(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    photos
        .into_iter()
        .map(|mut photo| {
            let mut people = conn
                .prepare("SELECT user_id FROM photo_people WHERE photo_id=?1 ORDER BY user_id")?;
            photo.people = people
                .query_map([photo.id], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(photo)
        })
        .collect()
}

pub fn setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
            r.get(0)
        })
        .optional()?)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings(key,value) VALUES(?1,?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn default_album_folder() -> Option<String> {
    dirs::picture_dir().map(|path| path.join("VRChat").to_string_lossy().to_string())
}

pub fn settings(conn: &Connection) -> Result<AppSettings> {
    Ok(AppSettings {
        album_folder: setting(conn, "album_folder")?
            .filter(|path| !path.trim().is_empty())
            .or_else(default_album_folder),
        steam_screenshot_folder: setting(conn, "steam_screenshot_folder")?
            .filter(|path| !path.trim().is_empty()),
        vrcx_database_path: setting(conn, "vrcx_database_path")?,
        sync_interval_minutes: setting(conn, "sync_interval_minutes")?
            .and_then(|value| value.parse().ok())
            .unwrap_or(15),
        show_self_in_friends: setting(conn, "show_self_in_friends")?
            .map(|value| value != "false")
            .unwrap_or(true),
    })
}

pub fn save_settings(conn: &Connection, settings: &AppSettings) -> Result<()> {
    for (key, value) in [
        ("album_folder", settings.album_folder.as_deref()),
        (
            "steam_screenshot_folder",
            settings.steam_screenshot_folder.as_deref(),
        ),
        ("vrcx_database_path", settings.vrcx_database_path.as_deref()),
    ] {
        if let Some(value) = value {
            set_setting(conn, key, value)?;
        }
    }
    set_setting(
        conn,
        "sync_interval_minutes",
        &settings.sync_interval_minutes.to_string(),
    )?;
    set_setting(
        conn,
        "show_self_in_friends",
        if settings.show_self_in_friends {
            "true"
        } else {
            "false"
        },
    )
}

pub fn assign_photos(conn: &mut Connection, photo_ids: &[i64], user_id: &str) -> Result<usize> {
    assign_photos_to_friends(conn, photo_ids, &[user_id.to_owned()])
}

pub fn assign_photos_to_friends(
    conn: &mut Connection,
    photo_ids: &[i64],
    user_ids: &[String],
) -> Result<usize> {
    let transaction = conn.transaction()?;
    let mut changed = 0;
    for photo_id in photo_ids {
        for user_id in user_ids {
            changed += transaction.execute(
                "INSERT OR IGNORE INTO photo_people(photo_id,user_id,source,confirmed)
                 VALUES(?1,?2,'manual',1)",
                params![photo_id, user_id],
            )?;
        }
    }
    transaction.commit()?;
    Ok(changed)
}

pub fn unassign_photo(conn: &Connection, photo_id: i64, user_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM photo_people WHERE photo_id=?1 AND user_id=?2",
        params![photo_id, user_id],
    )?;
    Ok(())
}

pub fn set_friend(conn: &Connection, user_id: &str, selected: bool) -> Result<()> {
    conn.execute(
        "UPDATE players SET is_friend=?2 WHERE user_id=?1",
        params![user_id, selected as i64],
    )?;
    Ok(())
}

pub fn friend_ids(conn: &Connection) -> Result<Vec<String>> {
    let mut statement =
        conn.prepare("SELECT user_id FROM players WHERE is_friend=1 ORDER BY user_id")?;
    let ids = statement
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_and_filters_photos() {
        let directory = tempfile::tempdir().unwrap();
        let mut conn = open(&directory.path().join("test.db")).unwrap();
        conn.execute(
            "INSERT INTO photos(source,kind,local_path,file_name,imported_at)
             VALUES('local','screenshot','x.png','x.png',datetime('now'))",
            [],
        )
        .unwrap();
        assert_eq!(
            list_photos(&conn, None, Some("screenshot")).unwrap().len(),
            1
        );
        assert!(list_photos(&conn, None, Some("album")).unwrap().is_empty());
        conn.execute(
            "INSERT INTO players(user_id,display_name) VALUES('usr_test','Test')",
            [],
        )
        .unwrap();
        assert!(!list_players(&conn).unwrap()[0].is_friend);
        set_friend(&conn, "usr_test", true).unwrap();
        assert!(list_players(&conn).unwrap()[0].is_friend);
        conn.execute(
            "INSERT INTO players(user_id,display_name) VALUES('usr_other','Other')",
            [],
        )
        .unwrap();
        assert_eq!(friend_ids(&conn).unwrap(), vec!["usr_test"]);
        let photo_id = conn
            .query_row("SELECT id FROM photos LIMIT 1", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap();
        let changed = assign_photos_to_friends(
            &mut conn,
            &[photo_id],
            &["usr_test".into(), "usr_other".into()],
        )
        .unwrap();
        assert_eq!(changed, 2);
    }
}
