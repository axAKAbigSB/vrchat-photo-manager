use anyhow::Result;
use rusqlite::Connection;

use crate::{vrcx, vrchat};

/// VRCX is the first source because it carries nickname history. API fills gaps
/// and refreshes the current profile image when an app-owned session is present.
pub async fn run(conn: &Connection) -> Result<String> {
    let imported = vrcx::import(conn).unwrap_or(0);
    let ids: Vec<String> = {
        let mut statement = conn.prepare("SELECT user_id FROM players")?;
        statement.query_map([], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    let mut refreshed = 0;
    for id in ids {
        if vrchat::refresh_player(conn, &id).await.is_ok() { refreshed += 1; }
    }
    let gallery = vrchat::sync_own_gallery(conn).await.unwrap_or(0);
    Ok(format!("同步完成：VRCX 导入 {imported} 位玩家，刷新 {refreshed} 个资料，自己的 Gallery 同步 {gallery} 张。"))
}
