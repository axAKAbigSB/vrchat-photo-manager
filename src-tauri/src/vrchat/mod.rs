//! Minimal VRChat Web API client. The cookie is an app-owned session, never copied from VRCX.
use anyhow::{bail, Result};
use reqwest::{header, Client, RequestBuilder, Response, StatusCode};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::Duration;
use tokio::time::sleep;

const BASE: &str = "https://api.vrchat.cloud/api/1";
const KEYRING_SERVICE: &str = "com.axaka.vrchat-photo-manager";
const KEYRING_USER: &str = "vrchat-session";
const REQUEST_TIMEOUT_SECS: u64 = 30;
const MAX_RETRIES: usize = 3;
const GALLERY_PAGE_SIZE: usize = 100;
const MAX_GALLERY_PAGES: usize = 100;
const PRINT_PAGE_SIZE: usize = 100;
const MAX_PRINT_PAGES: usize = 100;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub display_name: String,
    pub user_icon: Option<String>,
    pub profile_pic_override: Option<String>,
    pub current_avatar_thumbnail_image_url: Option<String>,
    pub note: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct File {
    pub id: String,
    pub name: String,
    pub versions: Vec<FileVersion>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileVersion {
    pub file: Option<FileBlob>,
    pub created_at: Option<String>,
}
#[derive(Deserialize)]
pub struct FileBlob {
    pub url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Print {
    pub id: String,
    pub files: PrintFiles,
    pub note: Option<String>,
    pub timestamp: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintFiles {
    pub image: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginResult {
    pub authenticated: bool,
    pub requires_two_factor_auth: Vec<String>,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatus {
    pub status: String,
    pub display_name: Option<String>,
    pub user_id: Option<String>,
    pub profile_pic_url: Option<String>,
    pub message: String,
}

fn client(session: &str) -> Result<Client> {
    Ok(Client::builder()
        .user_agent("VRC-Album/0.1 (personal photo manager)")
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .default_headers({
            let mut headers = header::HeaderMap::new();
            headers.insert(header::COOKIE, header::HeaderValue::from_str(session)?);
            headers
        })
        .build()?)
}

fn session(conn: &Connection) -> Result<String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)?;
    if let Ok(value) = entry.get_password() {
        return Ok(value);
    }
    if let Some(legacy) = crate::db::setting(conn, "vrchat_session")? {
        entry.set_password(&legacy)?;
        conn.execute("DELETE FROM settings WHERE key='vrchat_session'", [])?;
        return Ok(legacy);
    }
    bail!("尚未登录 VRChat。请先在设置中登录。")
}

pub(crate) fn session_cookie(conn: &Connection) -> Result<String> {
    session(conn)
}

pub fn has_session(conn: &Connection) -> bool {
    session(conn).is_ok()
}

fn store_session(value: &str) -> Result<()> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)?.set_password(value)?;
    Ok(())
}

fn response_cookies(response: &reqwest::Response) -> Vec<String> {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .map(ToOwned::to_owned)
        .collect()
}

fn merge_cookies(current: &str, updates: &[String]) -> String {
    let mut cookies: Vec<(String, String)> = current
        .split(';')
        .filter_map(|cookie| cookie.trim().split_once('='))
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect();
    for update in updates {
        if let Some((name, value)) = update.split_once('=') {
            if let Some(cookie) = cookies.iter_mut().find(|cookie| cookie.0 == name) {
                cookie.1 = value.to_owned();
            } else {
                cookies.push((name.to_owned(), value.to_owned()));
            }
        }
    }
    cookies
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ")
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    })
}

fn profile_picture(user: &User) -> Option<String> {
    non_empty(user.profile_pic_override.clone())
        .or_else(|| non_empty(user.user_icon.clone()))
        .or_else(|| non_empty(user.current_avatar_thumbnail_image_url.clone()))
}

fn trust_level(user: &User) -> String {
    if user.tags.iter().any(|tag| tag == "system_trust_veteran") {
        "Trusted User"
    } else if user.tags.iter().any(|tag| tag == "system_trust_trusted") {
        "Known User"
    } else if user.tags.iter().any(|tag| tag == "system_trust_known") {
        "User"
    } else if user.tags.iter().any(|tag| tag == "system_trust_basic") {
        "New User"
    } else {
        "Visitor"
    }
    .into()
}

fn should_retry(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn retry_delay(response: &Response, attempt: usize) -> Duration {
    response
        .headers()
        .get(header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(1 << attempt.min(3)))
}

async fn send_with_retry(request: RequestBuilder) -> Result<Response> {
    for attempt in 0..=MAX_RETRIES {
        let result = request
            .try_clone()
            .ok_or_else(|| anyhow::anyhow!("无法重试 VRChat API 请求"))?
            .send()
            .await;
        let response = match result {
            Ok(response) => response,
            Err(_) if attempt < MAX_RETRIES => {
                sleep(Duration::from_secs(1 << attempt.min(3))).await;
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        if !should_retry(response.status()) || attempt == MAX_RETRIES {
            return Ok(response);
        }
        sleep(retry_delay(&response, attempt)).await;
    }
    unreachable!()
}

pub async fn login(_conn: &Connection, username: &str, password: &str) -> Result<LoginResult> {
    let response = Client::builder()
        .user_agent("VRC-Album/0.1 (personal photo manager)")
        .build()?
        .get(format!("{BASE}/auth/user"))
        .basic_auth(username, Some(password))
        .send()
        .await?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        bail!("VRChat 用户名或密码不正确")
    }
    let cookies = response_cookies(&response);
    if cookies.is_empty() {
        bail!("VRChat 未返回登录会话。")
    }
    store_session(&cookies.join("; "))?;
    let body: serde_json::Value = response.json().await?;
    let methods: Vec<String> = body
        .get("requiresTwoFactorAuth")
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Ok(LoginResult {
        authenticated: methods.is_empty(),
        message: if methods.is_empty() {
            "VRChat 登录成功".into()
        } else {
            "请输入两步验证码".into()
        },
        requires_two_factor_auth: methods,
    })
}

pub async fn verify_two_factor(conn: &Connection, method: &str, code: &str) -> Result<LoginResult> {
    let endpoint = match method {
        "totp" => "totp",
        "emailOtp" | "emailotp" => "emailotp",
        "otp" => "otp",
        _ => bail!("不支持的两步验证方式：{method}"),
    };
    let current_session = session(conn)?;
    let response = client(&current_session)?
        .post(format!("{BASE}/auth/twofactorauth/{endpoint}/verify"))
        .json(&serde_json::json!({ "code": code }))
        .send()
        .await?;
    if !response.status().is_success() {
        bail!("验证码无效或已过期")
    }
    let cookies = response_cookies(&response);
    if !cookies.is_empty() {
        store_session(&merge_cookies(&current_session, &cookies))?;
    }
    Ok(LoginResult {
        authenticated: true,
        requires_two_factor_auth: Vec::new(),
        message: "两步验证成功".into(),
    })
}

pub fn logout() -> Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)?;
    let _ = entry.delete_credential();
    Ok(())
}

pub async fn user_with_session(session: &str, user_id: &str) -> Result<User> {
    let response = send_with_retry(client(session)?.get(format!("{BASE}/users/{user_id}"))).await?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        bail!("VRChat 登录已过期，请重新登录")
    }
    let response = response.error_for_status()?;
    Ok(response.json().await?)
}

async fn current_user(conn: &Connection) -> Result<User> {
    let response =
        send_with_retry(client(&session(conn)?)?.get(format!("{BASE}/auth/user"))).await?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        bail!("VRChat 登录已过期，请重新登录")
    }
    let response = response.error_for_status()?;
    Ok(response.json().await?)
}

pub async fn session_status(conn: &Connection) -> SessionStatus {
    if let Err(error) = session(conn) {
        let message = error.to_string();
        return SessionStatus {
            status: if message.contains("尚未登录") {
                "loggedOut"
            } else {
                "error"
            }
            .into(),
            display_name: None,
            user_id: None,
            profile_pic_url: None,
            message,
        };
    }
    match current_user(conn).await {
        Ok(user) => {
            let picture = profile_picture(&user);
            let display_name = user.display_name.clone();
            let user_id = user.id.clone();
            let _ = save_player(conn, user);
            SessionStatus {
                status: "active".into(),
                display_name: Some(display_name),
                user_id: Some(user_id),
                profile_pic_url: picture,
                message: "VRChat 会话有效".into(),
            }
        }
        Err(error) => {
            let message = error.to_string();
            let expired = message.contains("登录已过期");
            SessionStatus {
                status: if expired { "expired" } else { "error" }.into(),
                display_name: None,
                user_id: None,
                profile_pic_url: None,
                message,
            }
        }
    }
}

pub fn save_player(conn: &Connection, player: User) -> Result<()> {
    let picture = profile_picture(&player);
    let thumbnail = non_empty(player.current_avatar_thumbnail_image_url.clone());
    let trust = trust_level(&player);
    conn.execute(
        "INSERT INTO players(user_id,display_name,profile_pic_url,avatar_thumbnail_url,trust_level,note,source,last_synced_at)
         VALUES(?1,?2,?3,?4,?5,?6,'api',datetime('now'))
         ON CONFLICT(user_id) DO UPDATE SET
           display_name=excluded.display_name,
           profile_pic_url=COALESCE(excluded.profile_pic_url,players.profile_pic_url),
           avatar_thumbnail_url=COALESCE(excluded.avatar_thumbnail_url,players.avatar_thumbnail_url),
           trust_level=excluded.trust_level,
           note=excluded.note,
           source='api',last_synced_at=datetime('now')",
        params![
            player.id,
            player.display_name,
            picture,
            thumbnail,
            trust,
            player.note
        ],
    )?;
    Ok(())
}

pub async fn sync_own_gallery(conn: &Connection) -> Result<usize> {
    let own = current_user(conn).await?;
    let own_picture = profile_picture(&own);
    let own_thumbnail = non_empty(own.current_avatar_thumbnail_image_url.clone());
    let own_trust = trust_level(&own);
    conn.execute(
        "INSERT INTO players(user_id,display_name,profile_pic_url,avatar_thumbnail_url,trust_level,note,source,last_synced_at)
         VALUES(?1,?2,?3,?4,?5,?6,'api',datetime('now'))
         ON CONFLICT(user_id) DO UPDATE SET display_name=excluded.display_name,
         profile_pic_url=COALESCE(excluded.profile_pic_url,players.profile_pic_url),
         avatar_thumbnail_url=COALESCE(excluded.avatar_thumbnail_url,players.avatar_thumbnail_url),
         trust_level=excluded.trust_level,note=excluded.note,last_synced_at=datetime('now')",
        params![
            own.id,
            own.display_name,
            own_picture,
            own_thumbnail,
            own_trust,
            own.note
        ],
    )?;
    let mut count = 0;
    for page in 0..MAX_GALLERY_PAGES {
        let offset = page * GALLERY_PAGE_SIZE;
        let response = send_with_retry(client(&session(conn)?)?.get(format!(
            "{BASE}/files?tag=gallery&n={GALLERY_PAGE_SIZE}&offset={offset}"
        )))
        .await?;
        if response.status() == StatusCode::UNAUTHORIZED {
            bail!("VRChat 登录已过期，请重新登录")
        }
        let files: Vec<File> = response.error_for_status()?.json().await?;
        let batch_len = files.len();
        for file in files {
            let Some((url, captured_at)) = latest_file_url(&file) else {
                continue;
            };
            conn.execute(
                "INSERT INTO photos(user_id,source,kind,vrchat_file_id,remote_url,file_name,captured_at,imported_at)
                 VALUES(?1,'vrchat_gallery','album',?2,?3,?4,?5,datetime('now'))
                 ON CONFLICT(vrchat_file_id,user_id) DO UPDATE SET
                   remote_url=excluded.remote_url,file_name=excluded.file_name,
                   captured_at=COALESCE(excluded.captured_at,photos.captured_at)",
                params![own.id, file.id, url, file.name, captured_at],
            )?;
            let photo_id: i64 = conn.query_row(
                "SELECT id FROM photos WHERE vrchat_file_id=?1 AND user_id=?2",
                params![file.id, own.id],
                |row| row.get(0),
            )?;
            conn.execute(
                "INSERT OR IGNORE INTO photo_people(photo_id,user_id,source,confirmed)
             VALUES(?1,?2,'gallery-owner',1)",
                params![photo_id, own.id],
            )?;
            count += 1;
        }
        if gallery_page_is_last(batch_len) {
            break;
        }
        sleep(Duration::from_millis(350)).await;
    }

    let mut print_offset = 0;
    let mut seen_prints = HashSet::new();
    for _ in 0..MAX_PRINT_PAGES {
        let response = send_with_retry(client(&session(conn)?)?.get(format!(
            "{BASE}/prints/user/{}?n={PRINT_PAGE_SIZE}&offset={print_offset}",
            own.id
        )))
        .await?;
        if response.status() == StatusCode::UNAUTHORIZED {
            bail!("VRChat 登录已过期，请重新登录")
        }
        let prints: Vec<Print> = response.error_for_status()?.json().await?;
        let batch_len = prints.len();
        if batch_len == 0 {
            break;
        }
        let mut new_prints = 0;
        for print in prints {
            if !seen_prints.insert(print.id.clone()) {
                continue;
            }
            new_prints += 1;
            let Some(image) = non_empty(print.files.image) else {
                continue;
            };
            let name = non_empty(print.note).unwrap_or_else(|| "VRChat Print".into());
            let captured_at = print.timestamp.or(print.created_at);
            conn.execute(
                "INSERT INTO photos(user_id,source,kind,vrchat_file_id,remote_url,file_name,captured_at,imported_at)
                 VALUES(?1,'vrchat_print','album',?2,?3,?4,?5,datetime('now'))
                 ON CONFLICT(vrchat_file_id,user_id) DO UPDATE SET
                   source='vrchat_print',remote_url=excluded.remote_url,file_name=excluded.file_name,
                   captured_at=COALESCE(excluded.captured_at,photos.captured_at)",
                params![own.id, print.id, image, name, captured_at],
            )?;
            let photo_id: i64 = conn.query_row(
                "SELECT id FROM photos WHERE vrchat_file_id=?1 AND user_id=?2",
                params![print.id, own.id],
                |row| row.get(0),
            )?;
            conn.execute(
                "INSERT OR IGNORE INTO photo_people(photo_id,user_id,source,confirmed)
                 VALUES(?1,?2,'print-owner',1)",
                params![photo_id, own.id],
            )?;
            count += 1;
        }
        if new_prints == 0 {
            break;
        }
        print_offset += batch_len;
        sleep(Duration::from_millis(350)).await;
    }

    Ok(count)
}

fn gallery_page_is_last(batch_len: usize) -> bool {
    batch_len < GALLERY_PAGE_SIZE
}

fn latest_file_url(file: &File) -> Option<(String, Option<String>)> {
    file.versions.iter().rev().find_map(|version| {
        non_empty(version.file.as_ref()?.url.clone()).map(|url| (url, version.created_at.clone()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn mock_responses(
        responses: Vec<(u16, &'static str)>,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            for (status, headers) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 1024];
                let _ = stream.read(&mut request);
                let reason = if status == 200 { "OK" } else { "Error" };
                write!(
                    stream,
                    "HTTP/1.1 {status} {reason}\r\n{headers}Content-Length: 2\r\nConnection: close\r\n\r\n{{}}"
                )
                .unwrap();
            }
        });
        (format!("http://{address}"), handle)
    }

    fn user(profile: Option<&str>, icon: Option<&str>, avatar: Option<&str>) -> User {
        User {
            id: "usr_test".into(),
            display_name: "Test".into(),
            profile_pic_override: profile.map(str::to_owned),
            user_icon: icon.map(str::to_owned),
            current_avatar_thumbnail_image_url: avatar.map(str::to_owned),
            note: None,
            tags: Vec::new(),
        }
    }

    #[test]
    fn picture_uses_priority_and_ignores_blank_values() {
        assert_eq!(
            profile_picture(&user(Some(" override "), Some("icon"), Some("avatar"))).as_deref(),
            Some("override")
        );
        assert_eq!(
            profile_picture(&user(Some(" "), Some("icon"), Some("avatar"))).as_deref(),
            Some("icon")
        );
        assert_eq!(
            profile_picture(&user(None, Some(""), Some("avatar"))).as_deref(),
            Some("avatar")
        );
        assert!(profile_picture(&user(Some(""), Some(" "), None)).is_none());
    }

    #[test]
    fn trust_tags_follow_vrchat_name_color_ranks() {
        let mut player = user(None, None, None);
        assert_eq!(trust_level(&player), "Visitor");
        player.tags = vec!["system_trust_basic".into()];
        assert_eq!(trust_level(&player), "New User");
        player.tags = vec!["system_trust_known".into()];
        assert_eq!(trust_level(&player), "User");
        player.tags = vec!["system_trust_trusted".into()];
        assert_eq!(trust_level(&player), "Known User");
        player.tags = vec!["system_trust_veteran".into()];
        assert_eq!(trust_level(&player), "Trusted User");
    }

    #[test]
    fn retry_policy_only_retries_rate_limits_and_server_errors() {
        assert!(should_retry(StatusCode::TOO_MANY_REQUESTS));
        assert!(should_retry(StatusCode::BAD_GATEWAY));
        assert!(!should_retry(StatusCode::UNAUTHORIZED));
        assert!(!should_retry(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn gallery_stops_on_short_page() {
        assert!(!gallery_page_is_last(GALLERY_PAGE_SIZE));
        assert!(gallery_page_is_last(GALLERY_PAGE_SIZE - 1));
    }

    #[test]
    fn gallery_uses_latest_version_with_a_real_file_url() {
        let file: File = serde_json::from_str(
            r#"{
                "id":"file_test",
                "name":"Gallery image",
                "versions":[
                    {"file":{"url":"https://example/old.png"},"createdAt":"2026-01-01T00:00:00Z"},
                    {"file":null},
                    {"file":{"url":"https://example/new.png"},"createdAt":"2026-02-01T00:00:00Z"}
                ]
            }"#,
        )
        .unwrap();
        let (url, captured_at) = latest_file_url(&file).unwrap();
        assert_eq!(url, "https://example/new.png");
        assert_eq!(captured_at.as_deref(), Some("2026-02-01T00:00:00Z"));
    }

    #[test]
    fn print_response_accepts_optional_image_metadata() {
        let prints: Vec<Print> = serde_json::from_str(
            r#"[{
                "id":"prnt_test",
                "files":{"image":"https://example/print.png"},
                "note":"A print",
                "timestamp":"2026-03-01T00:00:00Z"
            }]"#,
        )
        .unwrap();
        assert_eq!(
            prints[0].files.image.as_deref(),
            Some("https://example/print.png")
        );
    }

    #[test]
    fn two_factor_cookie_updates_preserve_auth_cookie() {
        let updates = vec!["twoFactorAuth=new".to_owned()];
        assert_eq!(
            merge_cookies("auth=secret; twoFactorAuth=old", &updates),
            "auth=secret; twoFactorAuth=new"
        );
    }

    #[tokio::test]
    async fn retries_mocked_rate_limit_and_server_error_responses() {
        let (url, server) = mock_responses(vec![
            (429, "Retry-After: 0\r\n"),
            (500, "Retry-After: 0\r\n"),
            (200, ""),
        ]);
        let response = send_with_retry(Client::new().get(url)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn does_not_retry_mocked_unauthorized_response() {
        let (url, server) = mock_responses(vec![(401, "")]);
        let response = send_with_retry(Client::new().get(url)).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        server.join().unwrap();
    }
}
