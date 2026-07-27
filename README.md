# VRChat Photo Manager

把 VRChat 里拍下的瞬间，重新挂回真正在场的那些人身上。

相册里堆着成千上万张截图时，最难的往往不是「存在哪」，而是「这张到底是和谁」。本应用帮你把本地照片、Steam 截图，以及自己的 Gallery / Prints，和好友一一对上号——改名了也认得出人，解除好友也不会把回忆一起删掉。照片始终留在你的电脑上；关联靠的是每个人稳定的 VRChat ID，不是转瞬即逝的显示名。

Windows 桌面端。打开就能整理本地相册；想同步云端好友和相册时，再登录自己的 VRChat 账号即可。

## 功能

### 本地照片
- 默认读取「图片」文件夹下的 `VRChat` 相册
- 可选添加 Steam 截图目录
- 文件夹有变动时自动更新；放在好友 ID 文件夹里的照片会自动关联
- 可按来源筛选：本地、Gallery、Prints

### 好友与关联
- 同步后可在「管理好友」里挑选要固定显示在左栏的人
- 解除 VRChat 好友不会取消你已精选的人，也不会删掉照片关联
- 可把自己置顶显示
- 一张照片可以关联多位好友

### 同步
点「立即同步」会依次：

1. 更新你的 VRChat 好友列表
2. 同步自己的 Gallery 和 Prints
3. 刷新精选好友的资料与头像（不会拉取别人的 Gallery）

启动时不会自动同步。可在设置里配置定时同步；登录支持两步验证，会话保存在系统钥匙串里。

## 系统要求

- Windows 10 / 11
- 多数电脑已自带 [WebView2](https://developer.microsoft.com/microsoft-edge/webview2/)；若打不开窗口，按提示安装即可
- 浏览本地照片无需登录；同步云端内容需要 VRChat 账号

## 下载与安装

到 [GitHub Releases](https://github.com/axAKAbigSB/vrchat-photo-manager/releases) 下载安装包。

安装包尚未代码签名，首次运行若被 SmartScreen 拦住，选择「仍要运行」即可。

## 你的数据在哪

| 内容 | 位置 |
|---|---|
| 照片索引数据库 | `%AppData%\vrchat-photo-manager\photos.db` |
| 登录会话 | Windows 系统钥匙串 |

照片文件本身仍在你原来的文件夹里，本应用不会把它们搬走。

## 隐私

- 使用本应用自己的 VRChat 登录，不借用其它软件的会话
- 登录信息保存在系统钥匙串，不写进普通配置明文

## 已知限制

- 同步过程暂不可取消
- 不会根据照片内容自动猜测该关联谁
- 不同步他人的 Gallery

## 开发与发版

```powershell
npm install
npm run tauri dev
```

发版：对齐版本号后合并到 `master`，再推送标签（例如 `v0.1.0`），Actions 的 Release 工作流会自动上传安装包。
