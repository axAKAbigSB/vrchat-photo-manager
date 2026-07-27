mod db;
mod photos;
mod sync;
mod vrchat;
mod vrcx;

use std::path::PathBuf;

fn connection() -> Result<rusqlite::Connection, String> {
    let path = db::database_path().map_err(|error| error.to_string())?;
    db::open(&path).map_err(|error| error.to_string())
}

#[tauri::command]
fn list_players() -> Result<Vec<db::Player>, String> {
    db::list_players(&connection()?).map_err(|error| error.to_string())
}

#[tauri::command]
fn list_photos(user_id: String) -> Result<Vec<db::Photo>, String> {
    db::list_photos(&connection()?, &user_id).map_err(|error| error.to_string())
}

#[tauri::command]
fn scan_photo_folder(path: String) -> Result<usize, String> {
    let conn = connection()?;
    let imported = photos::scan(&conn, &PathBuf::from(&path)).map_err(|error| error.to_string())?;
    conn.execute(
        "INSERT INTO settings(key,value) VALUES('photo_folder',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [&path],
    ).map_err(|error| error.to_string())?;
    Ok(imported)
}

#[tauri::command]
fn sync_now() -> Result<String, String> {
    let conn = connection()?;
    tauri::async_runtime::block_on(sync::run(&conn)).map_err(|error| error.to_string())
}

#[tauri::command]
fn login_vrchat(username: String, password: String) -> Result<String, String> {
    let conn = connection()?;
    tauri::async_runtime::block_on(vrchat::login(&conn, &username, &password))
        .map_err(|error| error.to_string())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            list_players,
            list_photos,
            scan_photo_folder,
            sync_now,
            login_vrchat
        ])
        .setup(|_| {
            // Keep profile names and public avatars current. Individual errors
            // (e.g. no VRChat session yet) are intentionally non-fatal.
            std::thread::spawn(|| loop {
                if let Ok(conn) = connection() {
                    let _ = tauri::async_runtime::block_on(sync::run(&conn));
                }
                std::thread::sleep(std::time::Duration::from_secs(15 * 60));
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running VRC Album");
}
