use anyhow::Result;
use rusqlite::Connection;
use serde::Serialize;
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

fn save_result(conn: &Connection, message: &str, success: bool) -> Result<()> {
    crate::db::set_setting(conn, "last_sync_at", &chrono::Utc::now().to_rfc3339())?;
    crate::db::set_setting(conn, "last_sync_message", message)?;
    crate::db::set_setting(
        conn,
        "last_sync_success",
        if success { "true" } else { "false" },
    )
}

/// Sync VRChat friends, own gallery/prints, then refresh curated friend profiles.
pub async fn run<F>(conn: &Connection, mut report: F) -> Result<Outcome>
where
    F: FnMut(Progress),
{
    if !vrchat::has_session(conn) {
        let progress = Progress {
            phase: "done".into(),
            message: "尚未登录 VRChat，已跳过云端同步。请先登录后再同步好友与相册。".into(),
            ..Default::default()
        };
        save_result(conn, &progress.message, true)?;
        report(progress.clone());
        return Ok(Outcome { progress });
    }

    let mut progress = Progress {
        phase: "friends".into(),
        message: "正在同步 VRChat 好友列表…".into(),
        ..Default::default()
    };
    report(progress.clone());
    let friends = match vrchat::sync_friends(conn).await {
        Ok(result) => result,
        Err(error) => {
            progress.phase = if error.to_string().contains("登录已过期") {
                "expired"
            } else {
                "failed"
            }
            .into();
            progress.message = format!("VRChat 好友同步失败：{error}");
            save_result(conn, &progress.message, false)?;
            report(progress.clone());
            return Ok(Outcome { progress });
        }
    };
    progress.message = format!(
        "好友列表已更新：共 {} 人（新增标记 {}，解除标记 {}）",
        friends.total, friends.newly_marked, friends.unmarked
    );
    report(progress.clone());

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
            progress.message = format!(
                "好友 {} 人；VRChat 相册与拍立得同步失败：{error}",
                friends.total
            );
            save_result(conn, &progress.message, false)?;
            report(progress.clone());
            return Ok(Outcome { progress });
        }
    };
    progress.gallery_count = gallery;

    let ids = crate::db::friend_ids(conn)?;
    progress.phase = "profiles".into();
    progress.current = 0;
    progress.succeeded = 0;
    progress.failed = 0;
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
        progress.message = format!("好友 {} 人；VRChat 登录已过期，请重新登录", friends.total);
        save_result(conn, &progress.message, false)?;
        report(progress.clone());
        return Ok(Outcome { progress });
    }
    progress.phase = "done".into();
    progress.message = format!(
        "同步完成：好友 {} 人（新增 {}，解除标记 {}），刷新 {} 位精选资料，相册与拍立得 {} 张。",
        friends.total, friends.newly_marked, friends.unmarked, progress.succeeded, gallery
    );
    save_result(conn, &progress.message, progress.failed == 0)?;
    report(progress.clone());
    Ok(Outcome { progress })
}
