use anyhow::Result;
use rusqlite::Connection;
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::vrchat;

const PROFILE_CONCURRENCY: usize = 5;

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub phase: String,
    pub current: usize,
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub gallery_count: usize,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub progress: Progress,
}

struct LocalScanResult {
    count: usize,
    error: Option<String>,
}

enum CloudOutcome {
    Skipped,
    Done {
        friends_total: usize,
        newly_marked: usize,
        unmarked: usize,
        gallery: usize,
        profiles_succeeded: usize,
        profiles_failed: usize,
    },
    Failed {
        phase: String,
        message: String,
    },
}

fn save_result(conn: &Connection, message: &str, success: bool) -> Result<()> {
    crate::db::set_setting(conn, "last_sync_at", &chrono::Utc::now().to_rfc3339())?;
    crate::db::set_setting(conn, "last_sync_message", message)?;
    crate::db::set_setting(
        conn,
        "last_sync_success",
        if success { "true" } else { "false" },
    )
}

fn scan_local_folders(album: Option<String>, steam: Option<String>) -> LocalScanResult {
    let database_path = match crate::db::database_path() {
        Ok(path) => path,
        Err(error) => {
            return LocalScanResult {
                count: 0,
                error: Some(error.to_string()),
            };
        }
    };
    let conn = match crate::db::open(&database_path) {
        Ok(conn) => conn,
        Err(error) => {
            return LocalScanResult {
                count: 0,
                error: Some(error.to_string()),
            };
        }
    };

    let mut count = 0;
    let mut error = None;

    if let Some(folder) = album {
        match crate::photos::scan_configured_folder(&conn, Path::new(&folder), "album") {
            Ok(imported) => count += imported,
            Err(scan_error) => {
                error = Some(format!("相册目录扫描失败：{scan_error}"));
            }
        }
    }
    if let Some(folder) = steam {
        match crate::photos::scan_configured_folder(&conn, Path::new(&folder), "screenshot") {
            Ok(imported) => count += imported,
            Err(scan_error) => {
                let message = format!("Steam 目录扫描失败：{scan_error}");
                error = Some(match error {
                    Some(previous) => format!("{previous}；{message}"),
                    None => message,
                });
            }
        }
    }

    LocalScanResult { count, error }
}

fn merge_local_message(base: &str, local: &LocalScanResult) -> String {
    let local_part = match &local.error {
        Some(error) => format!("本地索引 {count} 张（{error}）", count = local.count),
        None => format!("本地相册/截图已索引 {} 张", local.count),
    };
    if base.is_empty() {
        local_part
    } else {
        format!("{local_part}；{base}")
    }
}

fn with_local_hint(message: &str, local_done: &AtomicBool) -> String {
    if local_done.load(Ordering::Acquire) {
        message.to_owned()
    } else {
        format!("{message}（本地扫描进行中）")
    }
}

/// Sync local photo folders in parallel with VRChat friends, own gallery/prints, and curated profiles.
pub async fn run<F>(conn: &Connection, mut report: F) -> Result<Outcome>
where
    F: FnMut(Progress),
{
    let mut progress = Progress {
        phase: "folders".into(),
        message: "正在扫描本地相册目录…".into(),
        ..Default::default()
    };
    report(progress.clone());

    let settings = crate::db::settings(conn)?;
    let album = settings
        .album_folder
        .filter(|path| !path.is_empty());
    let steam = settings
        .steam_screenshot_folder
        .filter(|path| !path.is_empty());
    let local_done = Arc::new(AtomicBool::new(false));
    let local_done_flag = Arc::clone(&local_done);
    let local_handle = tokio::task::spawn_blocking(move || {
        let result = scan_local_folders(album, steam);
        local_done_flag.store(true, Ordering::Release);
        result
    });

    let cloud = if vrchat::has_session(conn) {
        run_cloud(conn, &mut progress, &mut report, &local_done).await?
    } else {
        CloudOutcome::Skipped
    };

    let local = match local_handle.await {
        Ok(result) => result,
        Err(error) => LocalScanResult {
            count: 0,
            error: Some(format!("本地扫描任务失败：{error}")),
        },
    };

    let (phase, base_message, cloud_failed, profiles_failed) = match cloud {
        CloudOutcome::Skipped => (
            "done".to_owned(),
            "尚未登录 VRChat，已跳过云端同步。请先登录后再同步好友与相册。".to_owned(),
            false,
            0usize,
        ),
        CloudOutcome::Done {
            friends_total,
            newly_marked,
            unmarked,
            gallery,
            profiles_succeeded,
            profiles_failed,
        } => {
            progress.gallery_count = gallery;
            progress.succeeded = profiles_succeeded;
            progress.failed = profiles_failed;
            (
                "done".to_owned(),
                format!(
                    "同步完成：好友 {} 人（新增 {}，解除标记 {}），刷新 {} 位精选资料，相册与拍立得 {} 张。",
                    friends_total, newly_marked, unmarked, profiles_succeeded, gallery
                ),
                false,
                profiles_failed,
            )
        }
        CloudOutcome::Failed { phase, message } => (phase, message, true, 0usize),
    };

    progress.phase = phase;
    if local.error.is_some() {
        progress.failed = progress.failed.saturating_add(1);
    }
    progress.message = merge_local_message(&base_message, &local);
    let success = !cloud_failed && profiles_failed == 0 && local.error.is_none();
    save_result(conn, &progress.message, success)?;
    report(progress.clone());
    Ok(Outcome { progress })
}

async fn run_cloud<F>(
    conn: &Connection,
    progress: &mut Progress,
    report: &mut F,
    local_done: &AtomicBool,
) -> Result<CloudOutcome>
where
    F: FnMut(Progress),
{
    progress.phase = "friends".into();
    progress.message = with_local_hint("正在同步 VRChat 好友列表…", local_done);
    report(progress.clone());
    let friends = match vrchat::sync_friends(conn).await {
        Ok(result) => result,
        Err(error) => {
            let phase = if error.to_string().contains("登录已过期") {
                "expired"
            } else {
                "failed"
            }
            .into();
            return Ok(CloudOutcome::Failed {
                phase,
                message: format!("VRChat 好友同步失败：{error}"),
            });
        }
    };
    progress.message = with_local_hint(
        &format!(
            "好友列表已更新：共 {} 人（新增标记 {}，解除标记 {}）",
            friends.total, friends.newly_marked, friends.unmarked
        ),
        local_done,
    );
    report(progress.clone());

    progress.phase = "gallery".into();
    progress.message = with_local_hint("正在同步 VRChat 相册与拍立得…", local_done);
    report(progress.clone());
    let gallery = match vrchat::sync_own_gallery(conn).await {
        Ok(count) => count,
        Err(error) => {
            let phase = if error.to_string().contains("登录已过期") {
                "expired"
            } else {
                "failed"
            }
            .into();
            return Ok(CloudOutcome::Failed {
                phase,
                message: format!(
                    "好友 {} 人；VRChat 相册与拍立得同步失败：{error}",
                    friends.total
                ),
            });
        }
    };
    progress.gallery_count = gallery;

    let ids = crate::db::friend_ids(conn)?;
    progress.phase = "profiles".into();
    progress.current = 0;
    progress.succeeded = 0;
    progress.failed = 0;
    progress.total = ids.len();
    progress.message = with_local_hint(
        &format!("正在刷新精选好友资料 0/{}", progress.total),
        local_done,
    );
    report(progress.clone());
    let mut session_expired = false;
    let session = vrchat::session_cookie(conn)?;
    for chunk in ids.chunks(PROFILE_CONCURRENCY) {
        let mut tasks = tokio::task::JoinSet::new();
        for id in chunk {
            let id = id.clone();
            let session = session.clone();
            tasks.spawn(async move { vrchat::user_with_session(&session, &id).await });
        }
        while let Some(result) = tasks.join_next().await {
            progress.current += 1;
            match result {
                Ok(Ok(player)) => match vrchat::save_player(conn, player) {
                    Ok(()) => progress.succeeded += 1,
                    Err(error) => {
                        progress.failed += 1;
                        progress.message = error.to_string();
                    }
                },
                Ok(Err(error)) => {
                    progress.failed += 1;
                    let error = error.to_string();
                    if error.contains("登录已过期") {
                        session_expired = true;
                    }
                    progress.message = error;
                }
                Err(error) => {
                    progress.failed += 1;
                    progress.message = format!("玩家同步任务失败：{error}");
                }
            }
            if !session_expired {
                progress.message = with_local_hint(
                    &format!(
                        "正在刷新精选好友资料 {}/{}（5 个并发 · 成功 {}，失败 {}）",
                        progress.current, progress.total, progress.succeeded, progress.failed
                    ),
                    local_done,
                );
            }
            report(progress.clone());
        }
        if session_expired {
            break;
        }
        tokio::time::sleep(Duration::from_millis(350)).await;
    }
    if session_expired {
        return Ok(CloudOutcome::Failed {
            phase: "expired".into(),
            message: format!("好友 {} 人；VRChat 登录已过期，请重新登录", friends.total),
        });
    }

    Ok(CloudOutcome::Done {
        friends_total: friends.total,
        newly_marked: friends.newly_marked,
        unmarked: friends.unmarked,
        gallery,
        profiles_succeeded: progress.succeeded,
        profiles_failed: progress.failed,
    })
}
