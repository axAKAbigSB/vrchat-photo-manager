use anyhow::Result;
use chrono::{DateTime, Local};
use image::ImageReader;
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};

const EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];

fn is_photo(path: &Path) -> bool {
    path.extension().and_then(|x| x.to_str()).is_some_and(|x| EXTENSIONS.contains(&x.to_lowercase().as_str()))
}

fn cache_dir() -> Result<PathBuf> {
    let dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("vrchat-photo-manager").join("thumbnails");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn thumbnail(path: &Path) -> Result<Option<String>> {
    let stem = path.to_string_lossy().replace(['\\', '/', ':'], "_");
    let output = cache_dir()?.join(format!("{stem}.jpg"));
    if !output.exists() {
        let image = match ImageReader::open(path)?.with_guessed_format()?.decode() { Ok(image) => image, Err(_) => return Ok(None) };
        image.thumbnail(360, 360).save_with_format(&output, image::ImageFormat::Jpeg)?;
    }
    Ok(Some(output.to_string_lossy().to_string()))
}

fn owner_from_path(path: &Path) -> Option<String> {
    path.parent()?.file_name()?.to_str().filter(|name| name.starts_with("usr_")).map(ToOwned::to_owned)
}

fn index_file(conn: &Connection, path: &Path) -> Result<()> {
    let metadata = std::fs::metadata(path)?;
    let captured = metadata.modified().ok().map(|time| DateTime::<Local>::from(time).to_rfc3339());
    let local_path = path.to_string_lossy().to_string();
    let file_name = path.file_name().and_then(|x| x.to_str()).unwrap_or("photo").to_owned();
    conn.execute(
        "INSERT INTO photos(user_id,source,local_path,thumbnail_path,file_name,captured_at,imported_at)
         VALUES(?1,'local',?2,?3,?4,?5,datetime('now'))
         ON CONFLICT(local_path) DO UPDATE SET thumbnail_path=excluded.thumbnail_path,captured_at=excluded.captured_at",
        params![owner_from_path(path), local_path, thumbnail(path)?, file_name, captured],
    )?;
    Ok(())
}

pub fn scan(conn: &Connection, root: &Path) -> Result<usize> {
    if !root.exists() { return Ok(0) }
    let mut count = 0;
    let mut todo = vec![root.to_path_buf()];
    while let Some(dir) = todo.pop() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() { todo.push(path); }
            else if is_photo(&path) { index_file(conn, &path)?; count += 1; }
        }
    }
    Ok(count)
}
