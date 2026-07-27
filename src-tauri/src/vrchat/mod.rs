//! Minimal VRChat Web API client. The cookie is an app-owned session, never copied from VRCX.
use anyhow::{bail, Context, Result};
use reqwest::{header, Client};
use rusqlite::{params, Connection};
use serde::Deserialize;

const BASE: &str = "https://api.vrchat.cloud/api/1";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub display_name: String,
    pub user_icon: Option<String>,
    pub profile_pic_override: Option<String>,
    pub current_avatar_thumbnail_image_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct File {
    pub id: String,
    pub name: String,
    pub mime_type: Option<String>,
    pub versions: Vec<FileVersion>,
}
#[derive(Deserialize)]
pub struct FileVersion {
    pub version: i64,
    pub file: FileBlob,
}
#[derive(Deserialize)]
pub struct FileBlob {
    pub url: String,
}

fn client(session: &str) -> Result<Client> {
    Ok(Client::builder()
        .user_agent("VRC-Album/0.1 (personal photo manager)")
        .default_headers({
            let mut headers = header::HeaderMap::new();
            headers.insert(header::COOKIE, header::HeaderValue::from_str(session)?);
            headers
        })
        .build()?)
}

fn session(conn: &Connection) -> Result<String> {
    conn.query_row(
        "SELECT value FROM settings WHERE key='vrchat_session'",
        [],
        |r| r.get(0),
    )
    .context("尚未登录 VRChat。请先在设置中登录。")
}

pub async fn login(conn: &Connection, username: &str, password: &str) -> Result<String> {
    let response = Client::builder()
        .user_agent("VRC-Album/0.1 (personal photo manager)")
        .build()?
        .get(format!("{BASE}/auth/user"))
        .basic_auth(username, Some(password))
        .send()
        .await?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        bail!("账号、密码或 2FA 验证失败。请在设置中完成 2FA。")
    }
    let cookies: Vec<String> = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .map(ToOwned::to_owned)
        .collect();
    if cookies.is_empty() {
        bail!("VRChat 未返回登录会话。")
    }
    conn.execute("INSERT INTO settings(key,value) VALUES('vrchat_session',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [cookies.join("; ")])?;
    Ok("VRChat 登录成功；若账号启用了 2FA，请在下一步验证。".into())
}

pub async fn user(conn: &Connection, user_id: &str) -> Result<User> {
    let response = client(&session(conn)?)?
        .get(format!("{BASE}/users/{user_id}"))
        .send()
        .await?
        .error_for_status()?;
    Ok(response.json().await?)
}

async fn current_user(conn: &Connection) -> Result<User> {
    let response = client(&session(conn)?)?
        .get(format!("{BASE}/auth/user"))
        .send()
        .await?
        .error_for_status()?;
    Ok(response.json().await?)
}

pub async fn refresh_player(conn: &Connection, user_id: &str) -> Result<()> {
    let player = user(conn, user_id).await?;
    let picture = player.user_icon.or(player.profile_pic_override);
    conn.execute(
        "INSERT INTO players(user_id,display_name,profile_pic_url,avatar_thumbnail_url,source,last_synced_at)
         VALUES(?1,?2,?3,?4,'api',datetime('now'))
         ON CONFLICT(user_id) DO UPDATE SET
           display_name=excluded.display_name, profile_pic_url=excluded.profile_pic_url,
           avatar_thumbnail_url=excluded.avatar_thumbnail_url, source='api', last_synced_at=datetime('now')",
        params![player.id, player.display_name, picture, player.current_avatar_thumbnail_image_url],
    )?;
    Ok(())
}

pub async fn sync_own_gallery(conn: &Connection) -> Result<usize> {
    let response = client(&session(conn)?)?
        .get(format!("{BASE}/files?tag=gallery&n=100"))
        .send()
        .await?
        .error_for_status()?;
    let files: Vec<File> = response.json().await?;
    let own = current_user(conn).await?;
    let mut count = 0;
    for file in files {
        let Some(version) = file.versions.last() else {
            continue;
        };
        if !file
            .mime_type
            .as_deref()
            .is_some_and(|mime| mime.starts_with("image/"))
        {
            continue;
        }
        conn.execute(
            "INSERT INTO photos(user_id,source,vrchat_file_id,remote_url,file_name,imported_at)
             VALUES(?1,'vrchat_gallery',?2,?3,?4,datetime('now'))
             ON CONFLICT(vrchat_file_id,user_id) DO UPDATE SET remote_url=excluded.remote_url,file_name=excluded.file_name",
            params![own.id, file.id, version.file.url, file.name],
        )?;
        count += 1;
    }
    Ok(count)
}
