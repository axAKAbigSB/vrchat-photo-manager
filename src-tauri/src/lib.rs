mod db;
mod photos;
mod sync;
mod vrchat;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncStatus {
    running: bool,
    phase: String,
    current: usize,
    total: usize,
    succeeded: usize,
    failed: usize,
    gallery_count: usize,
    message: String,
    started_at: Option<String>,
    finished_at: Option<String>,
}

impl Default for SyncStatus {
    fn default() -> Self {
        Self {
            running: false,
            phase: "idle".into(),
            current: 0,
            total: 0,
            succeeded: 0,
            failed: 0,
            gallery_count: 0,
            message: "尚未开始同步".into(),
            started_at: None,
            finished_at: None,
        }
    }
}

#[derive(Clone, Default)]
struct SyncManager(Arc<Mutex<SyncStatus>>);

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LastSync {
    at: Option<String>,
    message: Option<String>,
    success: Option<bool>,
}

fn connection() -> Result<rusqlite::Connection, String> {
    let path = db::database_path().map_err(|error| error.to_string())?;
    db::open(&path).map_err(|error| error.to_string())
}

#[tauri::command]
fn list_players() -> Result<Vec<db::Player>, String> {
    db::list_players(&connection()?).map_err(|error| error.to_string())
}

#[tauri::command]
fn list_photos(user_id: Option<String>, kind: Option<String>) -> Result<Vec<db::Photo>, String> {
    db::list_photos(&connection()?, user_id.as_deref(), kind.as_deref())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn scan_photo_folder(path: String, kind: String) -> Result<usize, String> {
    let conn = connection()?;
    let imported = photos::scan_configured_folder(&conn, &PathBuf::from(&path), &kind)
        .map_err(|error| error.to_string())?;
    photos::watch_configured_folder(PathBuf::from(path), kind)
        .map_err(|error| error.to_string())?;
    Ok(imported)
}

#[tauri::command]
fn get_settings() -> Result<db::AppSettings, String> {
    let mut settings = db::settings(&connection()?).map_err(|error| error.to_string())?;
    if let Some(steam) = settings.steam_screenshot_folder.take() {
        settings.steam_screenshot_folder = Some(
            photos::normalize_steam_folder(Path::new(&steam))
                .to_string_lossy()
                .into_owned(),
        );
    }
    Ok(settings)
}

#[tauri::command]
fn save_settings(mut settings: db::AppSettings) -> Result<(), String> {
    if let Some(steam) = settings.steam_screenshot_folder.take() {
        settings.steam_screenshot_folder = Some(
            photos::normalize_steam_folder(Path::new(&steam))
                .to_string_lossy()
                .into_owned(),
        );
    }
    let conn = connection()?;
    db::save_settings(&conn, &settings).map_err(|error| error.to_string())?;
    for (path, kind) in [
        (settings.album_folder, "album"),
        (settings.steam_screenshot_folder, "screenshot"),
    ] {
        if let Some(path) = path.filter(|path| !path.is_empty()) {
            photos::watch_configured_folder(PathBuf::from(path), kind.into())
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
fn assign_photos(photo_ids: Vec<i64>, user_id: String) -> Result<usize, String> {
    db::assign_photos(&mut connection()?, &photo_ids, &user_id).map_err(|error| error.to_string())
}

#[tauri::command]
fn assign_photos_to_friends(photo_ids: Vec<i64>, user_ids: Vec<String>) -> Result<usize, String> {
    db::assign_photos_to_friends(&mut connection()?, &photo_ids, &user_ids)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn unassign_photo(photo_id: i64, user_id: String) -> Result<(), String> {
    db::unassign_photo(&connection()?, photo_id, &user_id).map_err(|error| error.to_string())
}

#[tauri::command]
fn set_friend(user_id: String, selected: bool) -> Result<(), String> {
    db::set_friend(&connection()?, &user_id, selected).map_err(|error| error.to_string())
}

#[tauri::command]
fn reorder_friends(user_ids: Vec<String>) -> Result<(), String> {
    db::reorder_friends(&connection()?, &user_ids).map_err(|error| error.to_string())
}

#[tauri::command]
fn start_sync(
    app: tauri::AppHandle,
    manager: tauri::State<'_, SyncManager>,
) -> Result<SyncStatus, String> {
    start_sync_task(app, manager.inner().clone())
}

#[tauri::command]
fn get_sync_status(manager: tauri::State<'_, SyncManager>) -> Result<SyncStatus, String> {
    manager
        .0
        .lock()
        .map(|status| status.clone())
        .map_err(|_| "同步状态不可用".into())
}

#[tauri::command]
fn get_last_sync() -> Result<LastSync, String> {
    let conn = connection()?;
    Ok(LastSync {
        at: db::setting(&conn, "last_sync_at").map_err(|error| error.to_string())?,
        message: db::setting(&conn, "last_sync_message").map_err(|error| error.to_string())?,
        success: db::setting(&conn, "last_sync_success")
            .map_err(|error| error.to_string())?
            .map(|value| value == "true"),
    })
}

#[tauri::command]
fn vrchat_session_status() -> Result<vrchat::SessionStatus, String> {
    let conn = connection()?;
    Ok(tauri::async_runtime::block_on(vrchat::session_status(
        &conn,
    )))
}

#[tauri::command]
fn login_vrchat(username: String, password: String) -> Result<vrchat::LoginResult, String> {
    let conn = connection()?;
    tauri::async_runtime::block_on(vrchat::login(&conn, &username, &password))
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn verify_two_factor(method: String, code: String) -> Result<vrchat::LoginResult, String> {
    let conn = connection()?;
    tauri::async_runtime::block_on(vrchat::verify_two_factor(&conn, &method, &code))
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn logout_vrchat() -> Result<(), String> {
    vrchat::logout().map_err(|error| error.to_string())
}

fn start_sync_task(app: tauri::AppHandle, manager: SyncManager) -> Result<SyncStatus, String> {
    {
        let mut status = manager.0.lock().map_err(|_| "同步状态不可用".to_string())?;
        if status.running {
            return Ok(status.clone());
        }
        *status = SyncStatus {
            running: true,
            phase: "starting".into(),
            message: "正在启动同步…".into(),
            started_at: Some(chrono::Utc::now().to_rfc3339()),
            ..Default::default()
        };
    }
    let initial = manager.0.lock().map_err(|_| "同步状态不可用")?.clone();
    let _ = app.emit("sync-progress", &initial);
    std::thread::spawn(move || {
        let result = connection().and_then(|conn| {
            tauri::async_runtime::block_on(sync::run(&conn, |progress| {
                if let Ok(mut status) = manager.0.lock() {
                    status.phase = progress.phase;
                    status.current = progress.current;
                    status.total = progress.total;
                    status.succeeded = progress.succeeded;
                    status.failed = progress.failed;
                    status.gallery_count = progress.gallery_count;
                    status.message = progress.message;
                    let _ = app.emit("sync-progress", &*status);
                }
            }))
            .map_err(|error| error.to_string())
        });
        if let Ok(mut status) = manager.0.lock() {
            status.running = false;
            status.finished_at = Some(chrono::Utc::now().to_rfc3339());
            match result {
                Ok(outcome) => {
                    status.phase = outcome.progress.phase;
                    status.message = outcome.progress.message;
                }
                Err(error) => {
                    status.phase = "failed".into();
                    status.message = format!("同步失败：{error}");
                    if let Ok(conn) = connection() {
                        let _ = db::set_setting(
                            &conn,
                            "last_sync_at",
                            &chrono::Utc::now().to_rfc3339(),
                        );
                        let _ = db::set_setting(&conn, "last_sync_message", &status.message);
                        let _ = db::set_setting(&conn, "last_sync_success", "false");
                    }
                }
            }
            let _ = app.emit("sync-progress", &*status);
        }
    });
    Ok(initial)
}

pub fn run() {
    tauri::Builder::default()
        .manage(SyncManager::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            list_players,
            list_photos,
            scan_photo_folder,
            get_settings,
            save_settings,
            assign_photos,
            assign_photos_to_friends,
            unassign_photo,
            set_friend,
            reorder_friends,
            start_sync,
            get_sync_status,
            get_last_sync,
            vrchat_session_status,
            login_vrchat,
            verify_two_factor,
            logout_vrchat
        ])
        .setup(|app| {
            // Wait for the configured interval before the first automatic sync.
            // Startup remains local-only; users can still trigger a manual sync.
            let app_handle = app.handle().clone();
            let manager = app.state::<SyncManager>().inner().clone();
            std::thread::spawn(move || loop {
                let interval = if let Ok(conn) = connection() {
                    db::settings(&conn)
                        .map(|settings| settings.sync_interval_minutes.max(5))
                        .unwrap_or(15)
                } else {
                    15
                };
                std::thread::sleep(std::time::Duration::from_secs(interval as u64 * 60));
                let _ = start_sync_task(app_handle.clone(), manager.clone());
            });
            if let Ok(conn) = connection() {
                if let Ok(settings) = db::settings(&conn) {
                    for (path, kind) in [
                        (settings.album_folder, "album"),
                        (settings.steam_screenshot_folder, "screenshot"),
                    ] {
                        if let Some(path) = path.filter(|path| !path.is_empty()) {
                            let _ =
                                photos::watch_configured_folder(PathBuf::from(path), kind.into());
                        }
                    }
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running VRC Album");
}
