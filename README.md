# VRChat Photo Manager

Windows 桌面端 VRChat 照片管理器（Tauri 2 + React + SQLite）。照片留在本地；玩家关联使用 VRChat `userId`，改名不会打散相册。

## 功能

### 本地照片
- 默认扫描 `%USERPROFILE%\Pictures\VRChat`（递归）
- 可选 Steam 截图目录：选中 Steam `userdata` 后，会展开其下 App `438100` 的 screenshots
- 文件夹监视；位于 `usr_xxx` 目录下的照片会自动关联对应用户
- 来源筛选：本地 / VRChat Gallery / VRChat Prints（拍立得）

### 好友与关联
- 同步时通过 VRChat Friends API 拉取在线/离线好友，写入候选池（`is_vrchat_friend`）
- 左栏「好友」是**精选列表**（`is_friend=1`），通过「管理好友」挑选；默认同步不会自动精选
- VRChat 解除好友只会清除 `is_vrchat_friend`，**不会**自动取消精选或删除玩家/照片关联
- 可把「自己」置顶显示（设置项，默认开启）
- 一张照片可关联多位精选好友

### VRChat 同步
「立即同步」按顺序执行：

1. 同步 VRChat 好友列表（分页；标记新增/解除）
2. 同步**当前登录账号**的 Gallery + Prints（分页拉取）
3. 仅刷新精选好友资料 / 头像（最多 5 并发）；**不拉取他人 Gallery**

其它行为：

- **启动不会自动同步**；定时同步会先等满一个间隔再跑（最短 5 分钟）
- 登录 / 2FA / 登出；会话状态：`loggedOut` / `active` / `expired`
- Cookie 存在系统钥匙串（keyring），不落在普通配置明文里
- 429 / 5xx 会重试；401 会停止同步并标记会话过期

## 系统要求

- Windows 10/11 x64
- [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/)（多数系统已自带；没有则需安装）
- VRChat 账号：仅云端好友 / Gallery / Prints 同步需要登录

## 下载与安装

预编译安装包见 [GitHub Releases](https://github.com/axAKAbigSB/vrchat-photo-manager/releases)（NSIS `.exe` 或 `.msi`）。

安装包目前未代码签名，首次运行可能被 Windows SmartScreen 拦截，选择「仍要运行」即可。

## 运行（开发）

```powershell
npm install
npm run tauri dev
```

仅预览前端：`npm run dev`。

## 发版（维护者）

1. 将 `package.json` 与 `src-tauri/tauri.conf.json` 的 `version` 对齐（必要时同步 `src-tauri/Cargo.toml`）
2. 合并到 `master` 并推送
3. 打标签并推送：
   ```powershell
   git tag v0.1.0
   git push origin v0.1.0
   ```
4. 等待 Actions 的 `Release` 工作流完成；产物会出现在 Releases 页

## 数据位置

| 用途 | 位置 |
|---|---|
| 应用数据库 | `%AppData%\vrchat-photo-manager\photos.db` |
| VRChat 会话 | OS keyring：`com.axaka.vrchat-photo-manager` / `vrchat-session` |

## 隐私与认证

- VRChat API 使用本应用自己的登录会话
- 旧版若把会话写在本地 `settings` 表，启动时会迁移到 keyring 并清除库内明文

## 已知限制

- 无同步取消、无增量同步
- 不会自动根据照片元数据建议关联
- 设计上不同步他人 Gallery
- 真实登录与 2FA 需人工验收
