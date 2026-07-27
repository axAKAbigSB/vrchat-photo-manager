use anyhow::Result;
use rusqlite::Connection;
use serde::Serialize;
use std::time::Duration;

use crate::{vrchat, vrcx};

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

fn save_result(conn: &Connection, message: &str, success: bool) -> Result<()> {
    crate::db::set_setting(conn, "last_sync_at", &chrono::Utc::now().to_rfc3339())?;
    crate::db::set_setting(conn, "last_sync_message", message)?;
    crate::db::set_setting(
        conn,
        "last_sync_success",
        if success { "true" } else { "false" },
    )
}

/// Import local metadata, prioritize cloud photos, then refresh profiles with
/// bounded concurrency so a large player list does not delay the gallery.
pub async fn run<F>(conn: &Connection, mut report: F) -> Result<Outcome>
where
    F: FnMut(Progress),
{
    let mut progress = Progress {
        phase: "vrcx".into(),
        message: "正在导入 VRCX 玩家…".into(),
        ..Default::default()
    };
    report(progress.clone());
    let imported = vrcx::import(conn).unwrap_or(0);
    if !vrchat::has_session(conn) {
        progress.phase = "done".into();
        progress.message =
            format!("VRCX 导入 {imported} 位玩家；尚未登录 VRChat，已跳过云端同步。");
        save_result(conn, &progress.message, true)?;
        report(progress.clone());
        return Ok(Outcome { progress });
    }
    progress.phase = "gallery".into();
    progress.message = "正在同步 VRChat 相册与拍立得…".into();
    report(progress.clone());
    let gallery = match vrchat::sync_own_gallery(conn).await {
        Ok(count) => count,
        Err(error) => {
            progress.phase = if error.to_string().contains("登录已过期") {
                "expired"
            } else {
                "failed"
            }
            .into();
            progress.message =
                format!("VRCX 导入 {imported} 位玩家；VRChat 相册与拍立得同步失败：{error}");
            save_result(conn, &progress.message, false)?;
            report(progress.clone());
            return Ok(Outcome { progress });
        }
    };
    progress.gallery_count = gallery;
    let ids = crate::db::friend_ids(conn)?;
    progress.phase = "profiles".into();
    progress.total = ids.len();
    progress.message = format!("正在刷新精选好友资料 0/{}", progress.total);
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
                progress.message = format!(
                    "正在刷新精选好友资料 {}/{}（5 个并发 · 成功 {}，失败 {}）",
                    progress.current, progress.total, progress.succeeded, progress.failed
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
        progress.phase = "expired".into();
        progress.message = format!("VRCX 导入 {imported} 位玩家；VRChat 登录已过期，请重新登录");
        save_result(conn, &progress.message, false)?;
        report(progress.clone());
        return Ok(Outcome { progress });
    }
    progress.phase = "done".into();
    progress.message = format!(
        "同步完成：VRCX 导入 {imported} 位玩家，刷新 {} 位精选好友资料，VRChat 相册与拍立得同步 {gallery} 张。",
        progress.succeeded
    );
    save_result(conn, &progress.message, progress.failed == 0)?;
    report(progress.clone());
    Ok(Outcome { progress })
}
