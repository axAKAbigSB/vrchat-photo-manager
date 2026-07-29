import { useEffect, useState } from 'react'
import { Cloud, FolderOpen, LoaderCircle, Search, Users } from 'lucide-react'
import type {
  AppSettings, LastSync, Player, SyncStatus, VrchatSessionStatus,
} from './lib/api'
import { api, normalizeSteamFolderPath } from './lib/api'

const avatarFallback = 'https://api.dicebear.com/9.x/shapes/svg?seed='

type Step = 'welcome' | 'login' | 'folders' | 'sync' | 'friends' | 'finish'

const steps: Step[] = ['welcome', 'login', 'folders', 'sync', 'friends', 'finish']
const stepLabels: Record<Step, string> = {
  welcome: '欢迎',
  login: '登录',
  folders: '目录',
  sync: '同步',
  friends: '精选',
  finish: '完成',
}

const displayPlayer = (player: Player) =>
  player.note ? `${player.note}（${player.displayName}）` : player.displayName

const trustClass = (level?: string) => {
  const normalized = level?.trim().toLowerCase().replaceAll('_', ' ')
  if (normalized === 'visitor') return 'trust-visitor'
  if (normalized === 'new user') return 'trust-new'
  if (normalized === 'user') return 'trust-user'
  if (normalized === 'known user') return 'trust-known'
  if (normalized === 'trusted user') return 'trust-trusted'
  return ''
}

const formatTime = (value?: string) => {
  if (!value) return '尚未同步'
  const normalized = /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/.test(value)
    ? `${value.replace(' ', 'T')}Z`
    : value
  const date = new Date(normalized)
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString()
}

export interface OnboardingWizardProps {
  settings: AppSettings
  onSettingsChange: (settings: AppSettings) => void
  sessionStatus?: VrchatSessionStatus
  username: string
  password: string
  twoFactorCode: string
  twoFactorMethods: string[]
  loggingIn: boolean
  authFeedback?: { message: string, error: boolean }
  onUsernameChange: (value: string) => void
  onPasswordChange: (value: string) => void
  onTwoFactorCodeChange: (value: string) => void
  onLogin: () => void
  onVerifyTwoFactor: () => void
  syncStatus?: SyncStatus
  lastSync?: LastSync
  onSync: () => void
  managedPlayers: Player[]
  friendQuery: string
  onFriendQueryChange: (value: string) => void
  onToggleFriend: (player: Player) => void
  friendStatusLabel: (player: Player) => string
  onComplete: () => void
  onSkip: () => void
}

export function OnboardingWizard({
  settings,
  onSettingsChange,
  sessionStatus,
  username,
  password,
  twoFactorCode,
  twoFactorMethods,
  loggingIn,
  authFeedback,
  onUsernameChange,
  onPasswordChange,
  onTwoFactorCodeChange,
  onLogin,
  onVerifyTwoFactor,
  syncStatus,
  lastSync,
  onSync,
  managedPlayers,
  friendQuery,
  onFriendQueryChange,
  onToggleFriend,
  friendStatusLabel,
  onComplete,
  onSkip,
}: OnboardingWizardProps) {
  const [step, setStep] = useState<Step>('welcome')
  const [savingFolders, setSavingFolders] = useState(false)
  const [folderNotice, setFolderNotice] = useState('')
  const [syncFinished, setSyncFinished] = useState(false)
  const stepIndex = steps.indexOf(step)
  const loggedIn = sessionStatus?.status === 'active' && !twoFactorMethods.length
  const syncing = syncStatus?.running ?? false
  const canAdvanceAfterSync = syncFinished || (!syncing && syncStatus?.phase === 'done')

  useEffect(() => {
    if (!syncing && syncStatus?.phase === 'done') setSyncFinished(true)
  }, [syncing, syncStatus?.phase])

  const goNext = () => {
    const next = steps[stepIndex + 1]
    if (next) setStep(next)
  }
  const goBack = () => {
    const prev = steps[stepIndex - 1]
    if (prev) setStep(prev)
  }

  const saveFolders = async () => {
    setSavingFolders(true)
    setFolderNotice('')
    try {
      await api.saveSettings(settings)
      setFolderNotice('目录已保存。下一步将同步并索引照片。')
      setStep('sync')
    } catch (error) {
      setFolderNotice(error instanceof Error ? error.message : '保存目录失败')
    } finally {
      setSavingFolders(false)
    }
  }

  return (
    <div className="modal-backdrop onboarding-backdrop">
      <section className="settings-modal onboarding-modal" onMouseDown={(event) => event.stopPropagation()}>
        <div className="modal-heading">
          <div>
            <h2>新手设置</h2>
            <small>{stepLabels[step]} · {stepIndex + 1} / {steps.length}</small>
          </div>
        </div>

        <div className="onboarding-steps" aria-hidden>
          {steps.map((item, index) => (
            <span
              key={item}
              className={`onboarding-step-dot ${index === stepIndex ? 'active' : ''} ${index < stepIndex ? 'done' : ''}`}
            />
          ))}
        </div>

        {step === 'welcome' && (
          <div className="onboarding-body onboarding-welcome">
            <img className="onboarding-app-icon" src="/app-icon.png" width={72} height={72} alt="" />
            <h3>欢迎使用 VRC Album</h3>
            <p>把本地相册、Steam 截图和 VRChat Gallery / Prints，重新挂回真正在场的那些人身上。</p>
            <p className="help">接下来会引导你登录、确认照片目录、同步，并精选左栏好友。</p>
          </div>
        )}

        {step === 'login' && (
          <div className="onboarding-body">
            <h3>登录 VRChat</h3>
            <p className="help">登录后才能同步好友列表，并精选显示在左栏的玩家。</p>
            {loggedIn ? (
              <div className="auth-card">
                {sessionStatus?.profilePicUrl && <img src={sessionStatus.profilePicUrl} alt="" />}
                <div><b>{sessionStatus?.displayName}</b><small>{sessionStatus?.userId}</small></div>
                <span>已登录</span>
              </div>
            ) : (
              <p className={`help auth-state ${sessionStatus?.status === 'expired' ? 'error' : ''}`}>
                {sessionStatus?.message ?? '请登录你的 VRChat 账号。'}
              </p>
            )}
            {authFeedback && (
              <p className={`auth-feedback ${authFeedback.error ? 'error' : 'ok'}`} role="status">
                {authFeedback.message}
              </p>
            )}
            {!loggedIn && !twoFactorMethods.length && (
              <>
                <label>用户名
                  <input value={username} onChange={(event) => onUsernameChange(event.target.value)} autoComplete="username" autoFocus />
                </label>
                <label>密码
                  <input type="password" value={password} onChange={(event) => onPasswordChange(event.target.value)} autoComplete="current-password" />
                </label>
                <button
                  className="secondary wide"
                  disabled={!username || !password || loggingIn}
                  onClick={onLogin}
                >
                  <Cloud size={16} />{loggingIn ? '登录中…' : '登录 VRChat'}
                </button>
              </>
            )}
            {twoFactorMethods.length > 0 && (
              <>
                <label>两步验证码（{twoFactorMethods[0]}）
                  <input value={twoFactorCode} onChange={(event) => onTwoFactorCodeChange(event.target.value)} inputMode="numeric" autoFocus />
                </label>
                <button
                  className="primary wide"
                  disabled={!twoFactorCode || loggingIn}
                  onClick={onVerifyTwoFactor}
                >
                  {loggingIn ? '验证中…' : '验证'}
                </button>
              </>
            )}
            <p className="help">会话保存在 Windows Credential Manager。若现在不想登录，可稍后再说。</p>
          </div>
        )}

        {step === 'folders' && (
          <div className="onboarding-body">
            <h3>照片目录</h3>
            <p className="help">确认本地相册路径；Steam 截图目录可选。</p>
            <label>相册目录
              <span className="directory-field">
                <input
                  value={settings.albumFolder ?? ''}
                  onChange={(event) => onSettingsChange({ ...settings, albumFolder: event.target.value })}
                  placeholder="C:\\Users\\你\\Pictures\\VRChat"
                />
                <button
                  type="button"
                  onClick={async () => {
                    const path = await api.chooseDirectory(settings.albumFolder)
                    if (path) onSettingsChange({ ...settings, albumFolder: path })
                  }}
                >
                  选择…
                </button>
              </span>
            </label>
            <label>Steam 安装目录
              <span className="directory-field">
                <input
                  value={settings.steamScreenshotFolder ?? ''}
                  onChange={(event) => onSettingsChange({ ...settings, steamScreenshotFolder: event.target.value })}
                  placeholder="例如 D:\Steam 或 C:\Program Files (x86)\Steam"
                />
                <button
                  type="button"
                  onClick={async () => {
                    const path = await api.chooseDirectory(settings.steamScreenshotFolder)
                    if (path) onSettingsChange({ ...settings, steamScreenshotFolder: normalizeSteamFolderPath(path) })
                  }}
                >
                  选择…
                </button>
              </span>
            </label>
            <p className="help">选择 Steam 安装根目录即可；程序会自动在 userdata 下查找 VRChat 截图文件夹。</p>
            {folderNotice && <p className="help ok">{folderNotice}</p>}
          </div>
        )}

        {step === 'sync' && (
          <div className="onboarding-body">
            <h3>同步照片与好友</h3>
            <p className="help">同步会索引本地目录，并拉取 VRChat 好友与自己的 Gallery / Prints。精选好友前请先完成同步。</p>
            <div className="sync-details">
              <b>{syncStatus?.running ? syncStatus.message : lastSync?.message ?? '尚未同步'}</b>
              <small>
                {syncStatus?.running && syncStatus.total
                  ? `进度 ${syncStatus.current}/${syncStatus.total} · 成功 ${syncStatus.succeeded} · 失败 ${syncStatus.failed}`
                  : `最后同步：${formatTime(lastSync?.at)}`}
              </small>
            </div>
            <button className="primary wide" disabled={syncing} onClick={() => { setSyncFinished(false); onSync() }}>
              {syncing ? <><LoaderCircle className="spin" size={16} />同步中…</> : <><FolderOpen size={16} />立即同步</>}
            </button>
            {canAdvanceAfterSync && (
              <p className="help ok">同步完成，可以继续精选好友。</p>
            )}
          </div>
        )}

        {step === 'friends' && (
          <div className="onboarding-body onboarding-friends">
            <h3>精选好友</h3>
            <p className="help">勾选要固定显示在左栏的玩家；之后可在「管理好友」里修改。</p>
            <label className="friend-manager-search">
              <Search size={15} />
              <input
                value={friendQuery}
                onChange={(event) => onFriendQueryChange(event.target.value)}
                placeholder="搜索备注、昵称、曾用名或 ID"
              />
            </label>
            <div className="friend-manager-list onboarding-friend-list">
              {managedPlayers.map((player) => (
                <label className="friend-manager-item" key={player.userId}>
                  <input type="checkbox" checked={player.isFriend} onChange={() => void onToggleFriend(player)} />
                  <img src={player.profilePicUrl || `${avatarFallback}${encodeURIComponent(player.userId)}`} alt="" />
                  <span>
                    <b className={trustClass(player.trustLevel)}>{displayPlayer(player)}</b>
                    <small>{friendStatusLabel(player)} · {player.userId}</small>
                  </span>
                </label>
              ))}
              {!managedPlayers.length && (
                <div className="empty">
                  {sessionStatus?.status === 'active'
                    ? '暂无候选玩家。请返回上一步重新同步。'
                    : '请先登录并同步后再精选好友。'}
                </div>
              )}
            </div>
          </div>
        )}

        {step === 'finish' && (
          <div className="onboarding-body">
            <Users size={28} />
            <h3>可以开始整理了</h3>
            <p>在照片上勾选后点「关联好友」，把瞬间挂回真正在场的人身上。解除 VRChat 好友不会删掉你的精选与关联。</p>
            <p className="help">需要改目录或重新引导时，可在设置里打开。</p>
          </div>
        )}

        <footer className="onboarding-footer">
          {step === 'welcome' && (
            <>
              <button className="text-button" type="button" onClick={onSkip}>跳过</button>
              <button className="primary" type="button" onClick={goNext}>开始设置</button>
            </>
          )}
          {step === 'login' && (
            <>
              <button className="text-button" type="button" onClick={() => setStep('folders')}>稍后再说</button>
              <div className="onboarding-footer-actions">
                <button className="secondary" type="button" onClick={goBack}>上一步</button>
                <button className="primary" type="button" disabled={!loggedIn} onClick={goNext}>下一步</button>
              </div>
            </>
          )}
          {step === 'folders' && (
            <>
              <button className="secondary" type="button" onClick={goBack}>上一步</button>
              <button className="primary" type="button" disabled={savingFolders} onClick={() => void saveFolders()}>
                {savingFolders ? '保存中…' : '保存并继续'}
              </button>
            </>
          )}
          {step === 'sync' && (
            <>
              <button className="secondary" type="button" onClick={goBack} disabled={syncing}>上一步</button>
              <button className="primary" type="button" disabled={syncing || !canAdvanceAfterSync} onClick={goNext}>
                下一步
              </button>
            </>
          )}
          {step === 'friends' && (
            <>
              <button className="text-button" type="button" onClick={() => setStep('finish')}>跳过精选</button>
              <div className="onboarding-footer-actions">
                <button className="secondary" type="button" onClick={goBack}>上一步</button>
                <button className="primary" type="button" onClick={goNext}>下一步</button>
              </div>
            </>
          )}
          {step === 'finish' && (
            <button className="primary wide" type="button" onClick={onComplete}>开始使用</button>
          )}
        </footer>
      </section>
    </div>
  )
}
