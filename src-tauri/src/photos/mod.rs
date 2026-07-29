use anyhow::Result;
use chrono::{DateTime, Local};
use image::ImageReader;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::{params, Connection};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];
const VRCHAT_STEAM_APP_ID: &str = "438100";
static WATCHED_FOLDERS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

fn is_photo(path: &Path) -> bool {
    path.extension()
        .and_then(|x| x.to_str())
        .is_some_and(|x| EXTENSIONS.contains(&x.to_lowercase().as_str()))
}

fn cache_dir() -> Result<PathBuf> {
    let dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("vrchat-photo-manager")
        .join("thumbnails");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn thumbnail(path: &Path) -> Result<Option<String>> {
    let stem = path.to_string_lossy().replace(['\\', '/', ':'], "_");
    let output = cache_dir()?.join(format!("{stem}.jpg"));
    if !output.exists() {
        let image = match ImageReader::open(path)?.with_guessed_format()?.decode() {
            Ok(image) => image,
            Err(_) => return Ok(None),
        };
        image
            .thumbnail(360, 360)
            .save_with_format(&output, image::ImageFormat::Jpeg)?;
    }
    Ok(Some(output.to_string_lossy().to_string()))
}

fn owner_from_path(path: &Path) -> Option<String> {
    path.parent()?
        .file_name()?
        .to_str()
        .filter(|name| name.starts_with("usr_"))
        .map(ToOwned::to_owned)
}

fn index_file(conn: &Connection, path: &Path, kind: &str) -> Result<()> {
    let metadata = std::fs::metadata(path)?;
    let captured = metadata
        .modified()
        .ok()
        .map(|time| DateTime::<Local>::from(time).to_rfc3339());
    let local_path = path.to_string_lossy().to_string();
    let file_name = path
        .file_name()
        .and_then(|x| x.to_str())
        .unwrap_or("photo")
        .to_owned();
    conn.execute(
        "INSERT INTO photos(user_id,source,kind,local_path,thumbnail_path,file_name,captured_at,imported_at)
         VALUES(?1,'local',?2,?3,?4,?5,?6,datetime('now'))
         ON CONFLICT(local_path) DO UPDATE SET kind=excluded.kind,
         thumbnail_path=excluded.thumbnail_path,captured_at=excluded.captured_at",
        params![owner_from_path(path), kind, local_path, thumbnail(path)?, file_name, captured],
    )?;
    if let Some(user_id) = owner_from_path(path) {
        let player_exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM players WHERE user_id=?1)",
            [&user_id],
            |row| row.get(0),
        )?;
        if player_exists {
            let photo_id: i64 = conn.query_row(
                "SELECT id FROM photos WHERE local_path=?1",
                [path.to_string_lossy().as_ref()],
                |row| row.get(0),
            )?;
            conn.execute(
                "INSERT OR IGNORE INTO photo_people(photo_id,user_id,source,confirmed)
                 VALUES(?1,?2,'folder',1)",
                params![photo_id, user_id],
            )?;
        }
    }
    Ok(())
}

fn scan(conn: &Connection, root: &Path, kind: &str, recursive: bool) -> Result<HashSet<String>> {
    let mut seen = HashSet::new();
    if !root.exists() {
        return Ok(seen);
    }
    let mut todo = vec![root.to_path_buf()];
    while let Some(dir) = todo.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                if recursive {
                    todo.push(path);
                }
            } else if is_photo(&path) {
                index_file(conn, &path, kind)?;
                seen.insert(path.to_string_lossy().to_string());
            }
        }
    }
    Ok(seen)
}

fn delete_local_photo(conn: &Connection, photo_id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM photo_people WHERE photo_id=?1",
        params![photo_id],
    )?;
    conn.execute("DELETE FROM photos WHERE id=?1", params![photo_id])?;
    Ok(())
}

/// Drop local rows under `root` that are not in `seen` (missing files or excluded subfolders).
fn reconcile_local_under_root(
    conn: &Connection,
    root: &Path,
    kind: &str,
    seen: &HashSet<String>,
) -> Result<usize> {
    let mut statement = conn.prepare(
        "SELECT id, local_path FROM photos
         WHERE source='local' AND kind=?1 AND local_path IS NOT NULL",
    )?;
    let rows: Vec<(i64, String)> = statement
        .query_map(params![kind], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut removed = 0;
    for (photo_id, local_path) in rows {
        let path = PathBuf::from(&local_path);
        if !path.starts_with(root) {
            continue;
        }
        if seen.contains(&local_path) {
            continue;
        }
        delete_local_photo(conn, photo_id)?;
        removed += 1;
    }
    Ok(removed)
}

/// Remove any local photo whose file is gone from disk.
fn prune_missing_local_photos(conn: &Connection) -> Result<usize> {
    let mut statement = conn.prepare(
        "SELECT id, local_path FROM photos WHERE source='local' AND local_path IS NOT NULL",
    )?;
    let rows: Vec<(i64, String)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut removed = 0;
    for (photo_id, local_path) in rows {
        if Path::new(&local_path).is_file() {
            continue;
        }
        delete_local_photo(conn, photo_id)?;
        removed += 1;
    }
    Ok(removed)
}

pub fn normalize_steam_folder(configured: &Path) -> PathBuf {
    // Prefer the Steam install root when the user picked userdata or a deeper path.
    if configured.join("userdata").is_dir() {
        return configured.to_path_buf();
    }
    if configured.file_name().is_some_and(|name| name == "userdata") {
        if let Some(parent) = configured.parent() {
            return parent.to_path_buf();
        }
    }
    if let Some(userdata) = configured
        .ancestors()
        .find(|path| path.file_name().is_some_and(|name| name == "userdata"))
    {
        if let Some(parent) = userdata.parent() {
            return parent.to_path_buf();
        }
    }
    configured.to_path_buf()
}

pub fn steam_screenshot_folders(configured: &Path) -> Vec<PathBuf> {
    let configured = normalize_steam_folder(configured);
    let userdata = if configured
        .file_name()
        .is_some_and(|name| name == "userdata")
    {
        Some(configured.clone())
    } else if configured.join("userdata").is_dir() {
        Some(configured.join("userdata"))
    } else {
        configured
            .ancestors()
            .find(|path| path.file_name().is_some_and(|name| name == "userdata"))
            .map(Path::to_path_buf)
    };
    let Some(userdata) = userdata else {
        return configured
            .exists()
            .then_some(configured)
            .into_iter()
            .collect();
    };
    let Ok(users) = std::fs::read_dir(userdata) else {
        return Vec::new();
    };
    users
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| {
            entry
                .path()
                .join("760")
                .join("remote")
                .join(VRCHAT_STEAM_APP_ID)
                .join("screenshots")
        })
        .filter(|path| path.is_dir())
        .collect()
}

pub fn scan_configured_folder(conn: &Connection, root: &Path, kind: &str) -> Result<usize> {
    let recursive = kind != "screenshot";
    let roots = if kind == "screenshot" {
        steam_screenshot_folders(root)
    } else {
        vec![root.to_path_buf()]
    };
    let mut count = 0;
    for scan_root in roots {
        if !scan_root.exists() {
            continue;
        }
        let seen = scan(conn, &scan_root, kind, recursive)?;
        count += seen.len();
        reconcile_local_under_root(conn, &scan_root, kind, &seen)?;
    }
    prune_missing_local_photos(conn)?;
    Ok(count)
}

pub fn watch_configured_folder(root: PathBuf, kind: String) -> Result<()> {
    let recursive = kind != "screenshot";
    let roots = if kind == "screenshot" {
        steam_screenshot_folders(&root)
    } else {
        vec![root]
    };
    for root in roots {
        watch_folder(root, kind.clone(), recursive)?;
    }
    Ok(())
}

pub fn watch_folder(root: PathBuf, kind: String, recursive: bool) -> Result<()> {
    let folders = WATCHED_FOLDERS.get_or_init(|| Mutex::new(HashSet::new()));
    if !folders.lock().expect("watch lock").insert(root.clone()) {
        return Ok(());
    }
    let mode = if recursive {
        RecursiveMode::Recursive
    } else {
        RecursiveMode::NonRecursive
    };
    std::thread::spawn(move || {
        let (sender, receiver) = std::sync::mpsc::channel();
        let Ok(mut watcher) = RecommendedWatcher::new(sender, notify::Config::default()) else {
            return;
        };
        if watcher.watch(&root, mode).is_err() {
            return;
        }
        while let Ok(result) = receiver.recv() {
            let Ok(event) = result else { continue };
            let Ok(database_path) = crate::db::database_path() else {
                continue;
            };
            let Ok(conn) = crate::db::open(&database_path) else {
                continue;
            };
            for path in event.paths {
                match event.kind {
                    EventKind::Create(_) | EventKind::Modify(_)
                        if path.is_file() && is_photo(&path) =>
                    {
                        let _ = index_file(&conn, &path, &kind);
                    }
                    EventKind::Remove(_) => {
                        let _ = conn.execute(
                            "DELETE FROM photos WHERE local_path=?1",
                            [path.to_string_lossy().as_ref()],
                        );
                    }
                    _ => {}
                }
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_supported_extensions_case_insensitively() {
        assert!(is_photo(Path::new("photo.PNG")));
        assert!(is_photo(Path::new("photo.webp")));
        assert!(!is_photo(Path::new("photo.txt")));
    }

    #[test]
    fn discovers_vrchat_screenshots_for_every_steam_user() {
        let root = tempfile::tempdir().unwrap();
        for user in ["111", "222"] {
            std::fs::create_dir_all(
                root.path()
                    .join("userdata")
                    .join(user)
                    .join("760")
                    .join("remote")
                    .join(VRCHAT_STEAM_APP_ID)
                    .join("screenshots"),
            )
            .unwrap();
        }
        let folders = steam_screenshot_folders(&root.path().join("userdata"));
        assert_eq!(folders.len(), 2);
    }

    #[test]
    fn discovers_vrchat_screenshots_from_steam_root() {
        let root = tempfile::tempdir().unwrap();
        let screenshots = root
            .path()
            .join("userdata")
            .join("111")
            .join("760")
            .join("remote")
            .join(VRCHAT_STEAM_APP_ID)
            .join("screenshots");
        std::fs::create_dir_all(&screenshots).unwrap();

        let folders = steam_screenshot_folders(root.path());
        assert_eq!(folders, vec![screenshots]);
    }

    #[test]
    fn normalizes_deep_steam_paths_to_install_root() {
        let root = tempfile::tempdir().unwrap();
        let deep = root
            .path()
            .join("userdata")
            .join("111")
            .join("760")
            .join("remote")
            .join(VRCHAT_STEAM_APP_ID);
        std::fs::create_dir_all(&deep).unwrap();

        assert_eq!(normalize_steam_folder(root.path()), root.path());
        assert_eq!(
            normalize_steam_folder(&root.path().join("userdata")),
            root.path()
        );
        assert_eq!(normalize_steam_folder(&deep), root.path());
    }

    #[test]
    fn screenshot_scan_skips_thumbnail_subfolders() {
        let root = tempfile::tempdir().unwrap();
        let screenshots = root
            .path()
            .join("userdata")
            .join("111")
            .join("760")
            .join("remote")
            .join(VRCHAT_STEAM_APP_ID)
            .join("screenshots");
        let thumbnails = screenshots.join("thumbnails");
        std::fs::create_dir_all(&thumbnails).unwrap();
        std::fs::write(screenshots.join("shot.png"), b"png").unwrap();
        std::fs::write(thumbnails.join("shot.png"), b"png").unwrap();

        let directory = tempfile::tempdir().unwrap();
        let conn = crate::db::open(&directory.path().join("scan.db")).unwrap();
        let count =
            scan_configured_folder(&conn, &root.path().join("userdata"), "screenshot").unwrap();
        assert_eq!(count, 1);
        let paths: Vec<String> = conn
            .prepare("SELECT local_path FROM photos WHERE kind='screenshot'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(paths.len(), 1);
        assert!(paths[0].ends_with("shot.png"));
        assert!(!paths[0].contains("thumbnails"));
    }

    #[test]
    fn screenshot_scan_removes_stale_thumbnail_rows_and_missing_files() {
        let root = tempfile::tempdir().unwrap();
        let screenshots = root
            .path()
            .join("userdata")
            .join("111")
            .join("760")
            .join("remote")
            .join(VRCHAT_STEAM_APP_ID)
            .join("screenshots");
        let thumbnails = screenshots.join("thumbnails");
        std::fs::create_dir_all(&thumbnails).unwrap();
        let keep = screenshots.join("keep.png");
        let stale_thumb = thumbnails.join("old.png");
        let missing = screenshots.join("missing.png");
        std::fs::write(&keep, b"png").unwrap();
        std::fs::write(&stale_thumb, b"png").unwrap();

        let directory = tempfile::tempdir().unwrap();
        let conn = crate::db::open(&directory.path().join("scan.db")).unwrap();
        for path in [&keep, &stale_thumb, &missing] {
            conn.execute(
                "INSERT INTO photos(source,kind,local_path,file_name,imported_at)
                 VALUES('local','screenshot',?1,?2,datetime('now'))",
                params![
                    path.to_string_lossy().as_ref(),
                    path.file_name().unwrap().to_string_lossy().as_ref()
                ],
            )
            .unwrap();
        }

        let count =
            scan_configured_folder(&conn, &root.path().join("userdata"), "screenshot").unwrap();
        assert_eq!(count, 1);
        let paths: Vec<String> = conn
            .prepare("SELECT local_path FROM photos WHERE kind='screenshot' ORDER BY local_path")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(paths, vec![keep.to_string_lossy().to_string()]);
    }
}
