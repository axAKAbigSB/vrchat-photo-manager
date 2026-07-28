import { useCallback, useEffect, useMemo, useState } from 'react'
import {
  Camera, ChevronDown, ChevronLeft, ChevronRight, Cloud, FolderOpen,
  Image, Images, LoaderCircle, LogOut, Maximize2, Search, Settings,
  Unlink, Users, X,
} from 'lucide-react'
import {
  api, type AppSettings, type LastSync, type Photo, type Player, type SyncStatus,
  type VrchatSessionStatus,
} from './lib/api'
import './App.css'

const avatarFallback = 'https://api.dicebear.com/9.x/shapes/svg?seed='
type View = 'all' | 'album' | 'screenshot' | 'player'

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
  // SQLite datetime('now') is UTC without a zone; treat it as UTC for local display.
  const normalized = /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/.test(value)
    ? `${value.replace(' ', 'T')}Z`
    : value
  const date = new Date(normalized)
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString()
}
const errorMessage = (error: unknown, fallback: string) => {
  if (error instanceof Error) return error.message
  if (typeof error === 'string' && error.trim()) return error
  return fallback
}

function App() {
  const [players, setPlayers] = useState<Player[]>([])
  const [photos, setPhotos] = useState<Photo[]>([])
  const [view, setView] = useState<View>('all')
  const [photosOpen, setPhotosOpen] = useState(true)
  const [playersOpen, setPlayersOpen] = useState(true)
  const [selectedId, setSelectedId] = useState<string>()
  const [selectedPhotos, setSelectedPhotos] = useState<Set<number>>(new Set())
  const [selectionMode, setSelectionMode] = useState(false)
  const [associationPreset, setAssociationPreset] = useState<Set<string>>(new Set())
  const [associationPhotoIds, setAssociationPhotoIds] = useState<number[]>()
  const [associationFriends, setAssociationFriends] = useState<Set<string>>(new Set())
  const [associating, setAssociating] = useState(false)
  const [previewIndex, setPreviewIndex] = useState<number>()
  const [query, setQuery] = useState('')
  const [sourceFilter, setSourceFilter] = useState('')
  const [loading, setLoading] = useState(true)
  const [syncStatus, setSyncStatus] = useState<SyncStatus>()
  const [lastSync, setLastSync] = useState<LastSync>()
  const [sessionStatus, setSessionStatus] = useState<VrchatSessionStatus>()
  const [loggingIn, setLoggingIn] = useState(false)
  const [authFeedback, setAuthFeedback] = useState<{ message: string, error: boolean }>()
  const [notice, setNotice] = useState('')
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [loginOpen, setLoginOpen] = useState(false)
  const [friendManagerOpen, setFriendManagerOpen] = useState(false)
  const [friendQuery, setFriendQuery] = useState('')
  const [settings, setSettings] = useState<AppSettings>({ syncIntervalMinutes: 15, showSelfInFriends: true })
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [twoFactorCode, setTwoFactorCode] = useState('')
  const [twoFactorMethods, setTwoFactorMethods] = useState<string[]>([])
  const [draggingFriendId, setDraggingFriendId] = useState<string>()
  const [dragOverFriendId, setDragOverFriendId] = useState<string>()
  const syncing = syncStatus?.running ?? false

  const selectedPlayer = players.find((player) => player.userId === selectedId)
  const curatedFriends = useMemo(() => players
    .filter((player) => player.isFriend && player.userId !== sessionStatus?.userId)
    .sort((left, right) => left.sortOrder - right.sortOrder || left.displayName.localeCompare(right.displayName)),
  [players, sessionStatus?.userId])
  const friends = useMemo(() => {
    const self = settings.showSelfInFriends
      ? players.find((player) => player.userId === sessionStatus?.userId)
      : undefined
    return self ? [self, ...curatedFriends] : curatedFriends
  }, [players, curatedFriends, sessionStatus?.userId, settings.showSelfInFriends])
  const canReorderFriends = !query.trim()
  const visibleFriends = useMemo(() => {
    const normalized = query.trim().toLowerCase()
    if (!normalized) return friends
    return friends.filter((player) =>
      [player.note, player.vrcxMemo, player.displayName, player.userId, ...player.previousNames]
        .filter(Boolean).some((value) => value!.toLowerCase().includes(normalized)),
    )
  }, [friends, query])
  const managedPlayers = useMemo(() => {
    const normalized = friendQuery.trim().toLowerCase()
    const candidates = players.filter((player) =>
      player.userId !== sessionStatus?.userId
      && (player.isVrchatFriend || player.isFriend || player.photoCount > 0),
    )
    const sorted = [...candidates].sort((left, right) => {
      const rank = (player: Player) => {
        if (player.isVrchatFriend) return 0
        if (player.isFriend) return 1
        return 2
      }
      const byRank = rank(left) - rank(right)
      return byRank || left.displayName.localeCompare(right.displayName)
    })
    if (!normalized) return sorted
    return sorted.filter((player) =>
      [player.note, player.vrcxMemo, player.displayName, player.userId, ...player.previousNames]
        .filter(Boolean).some((value) => value!.toLowerCase().includes(normalized)),
    )
  }, [players, friendQuery, sessionStatus?.userId])
  const friendStatusLabel = (player: Player) => {
    if (player.isVrchatFriend) return 'VRChat 好友'
    if (player.isFriend) return '已解除 VRChat 好友 · 仍精选'
    return '仅本地 / 有关联照片'
  }
  const visiblePhotos = useMemo(() => photos.filter((photo) => {
    const fromSource = !sourceFilter || photo.source === sourceFilter
    return fromSource
  }), [photos, sourceFilter])
  const preview = previewIndex === undefined ? undefined : visiblePhotos[previewIndex]

  const refreshPlayers = async () => setPlayers(await api.listPlayers())
  const refreshPhotos = useCallback(async () => {
    setLoading(true)
    try {
      const kind = view === 'album' || view === 'screenshot' ? view : undefined
      const userId = view === 'player' ? selectedId : undefined
      const next = await api.listPhotos(userId, kind)
      const nextIds = new Set(next.map((photo) => photo.id))
      setPhotos(next)
      // Keep multi-select across reloads (e.g. sync finished); drop only missing ids.
      setSelectedPhotos((current) => new Set([...current].filter((id) => nextIds.has(id))))
    } finally {
      setLoading(false)
    }
  }, [view, selectedId])

  useEffect(() => {
    void Promise.all([
      refreshPlayers(),
      api.getSettings().then(setSettings),
      api.vrchatSessionStatus().then(async (status) => {
        setSessionStatus(status)
        if (status.status === 'active') await refreshPlayers()
      }),
      api.getSyncStatus().then(setSyncStatus),
      api.getLastSync().then(setLastSync),
    ])
  }, [])
  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | undefined
    void api.onSyncProgress((status) => {
      if (disposed) return
      setSyncStatus(status)
      if (!status.running) {
        setNotice(status.message)
        void Promise.all([
          refreshPlayers(),
          refreshPhotos(),
          api.getLastSync().then(setLastSync),
          api.vrchatSessionStatus().then(setSessionStatus),
        ])
      }
    }).then((stop) => {
      if (disposed) stop()
      else unlisten = stop
    })
    return () => {
      disposed = true
      unlisten?.()
    }
  }, [refreshPhotos])
  useEffect(() => { void refreshPhotos() }, [refreshPhotos])

  const choosePhotoView = (next: Exclude<View, 'player'>) => {
    setView(next)
    setSelectedId(undefined)
    setPhotosOpen(true)
    setSelectedPhotos(new Set())
    setSelectionMode(false)
  }
  const choosePlayer = (player: Player) => {
    setSelectedId(player.userId)
    setView('player')
    setSelectedPhotos(new Set())
    setSelectionMode(false)
  }
  const sync = async () => {
    try {
      setSyncStatus(await api.startSync())
    } catch (error) {
      setNotice(errorMessage(error, '同步失败'))
    }
  }
  const saveAndScan = async () => {
    try {
      await api.saveSettings(settings)
      let count = 0
      if (settings.albumFolder) count += await api.scanPhotoFolder(settings.albumFolder, 'album')
      if (settings.steamScreenshotFolder) count += await api.scanPhotoFolder(settings.steamScreenshotFolder, 'screenshot')
      setNotice(`设置已保存，共索引 ${count} 张照片。`)
      await refreshPhotos()
    } catch (error) {
      setNotice(errorMessage(error, '保存设置失败'))
    }
  }
  const login = async () => {
    setLoggingIn(true)
    setAuthFeedback(undefined)
    try {
      const result = await api.loginVrchat(username, password)
      setAuthFeedback({ message: result.message, error: false })
      setTwoFactorMethods(result.requiresTwoFactorAuth)
      if (result.authenticated) {
        setPassword('')
        setSessionStatus(await api.vrchatSessionStatus())
        await refreshPlayers()
      }
    } catch (error) {
      setAuthFeedback({ message: errorMessage(error, '登录失败'), error: true })
    } finally {
      setLoggingIn(false)
    }
  }
  const verifyTwoFactor = async () => {
    setLoggingIn(true)
    setAuthFeedback(undefined)
    try {
      const result = await api.verifyTwoFactor(twoFactorMethods[0] ?? 'totp', twoFactorCode)
      setAuthFeedback({ message: result.message, error: false })
      setTwoFactorMethods([])
      setTwoFactorCode('')
      setPassword('')
      setSessionStatus(await api.vrchatSessionStatus())
      await refreshPlayers()
    } catch (error) {
      setAuthFeedback({ message: errorMessage(error, '验证失败'), error: true })
    } finally {
      setLoggingIn(false)
    }
  }
  const togglePhoto = (id: number) => setSelectedPhotos((current) => {
    setSelectionMode(true)
    const next = new Set(current)
    if (next.has(id)) next.delete(id); else next.add(id)
    return next
  })
  const toggleFriend = async (player: Player) => {
    const selected = !player.isFriend
    await api.setFriend(player.userId, selected)
    if (selected) {
      const nextOrder = Math.max(0, ...players.filter((item) => item.isFriend).map((item) => item.sortOrder)) + 1
      setPlayers((current) => current.map((item) =>
        item.userId === player.userId ? { ...item, isFriend: true, sortOrder: nextOrder } : item,
      ))
    } else {
      setPlayers((current) => current.map((item) =>
        item.userId === player.userId ? { ...item, isFriend: false, sortOrder: 0 } : item,
      ))
    }
  }
  const reorderCuratedFriends = async (fromId: string, toId: string) => {
    if (fromId === toId) return
    const ids = curatedFriends.map((player) => player.userId)
    const from = ids.indexOf(fromId)
    const to = ids.indexOf(toId)
    if (from < 0 || to < 0) return
    const next = [...ids]
    next.splice(from, 1)
    next.splice(to, 0, fromId)
    setPlayers((current) => current.map((player) => {
      const index = next.indexOf(player.userId)
      return index < 0 ? player : { ...player, sortOrder: index + 1 }
    }))
    try {
      await api.reorderFriends(next)
    } catch (error) {
      setNotice(errorMessage(error, '好友排序失败'))
      await refreshPlayers()
    }
  }
  const openAssociation = (photoIds: number[]) => {
    setAssociationPhotoIds(photoIds)
    setAssociationFriends(new Set(associationPreset))
  }
  const confirmAssociation = async () => {
    if (!associationPhotoIds?.length || !associationFriends.size) return
    setAssociating(true)
    try {
      const count = await api.assignPhotosToFriends(associationPhotoIds, [...associationFriends])
      setNotice(`已添加 ${count} 个照片与好友关联。`)
      setAssociationPhotoIds(undefined)
      setAssociationFriends(new Set())
      setAssociationPreset(new Set())
      setSelectedPhotos(new Set())
      setSelectionMode(false)
      await Promise.all([refreshPlayers(), refreshPhotos()])
    } catch (error) {
      setNotice(errorMessage(error, '关联好友失败'))
    } finally {
      setAssociating(false)
    }
  }
  const startAssociatingSelectedFriend = () => {
    if (!selectedId) return
    setAssociationPreset(new Set([selectedId]))
    setSelectionMode(true)
    choosePhotoView('all')
  }

  return (
    <main className="app-shell">
      <aside className="sidebar">
        <div className="brand"><img className="brand-mark" src="/brand-icon.png" width={26} height={26} alt="" /><span>VRC 相册</span></div>
        <button className="section-button" onClick={() => setPhotosOpen((open) => !open)} aria-expanded={photosOpen}>
          <Images size={16} /><span>照片</span><ChevronDown className={photosOpen ? 'expanded' : ''} size={15} />
        </button>
        {photosOpen && <nav className="photo-nav">
          <button className={view === 'all' ? 'active' : ''} onClick={() => choosePhotoView('all')}><Image size={14} />全部照片</button>
          <button className={view === 'album' ? 'active' : ''} onClick={() => choosePhotoView('album')}><Images size={14} />相册</button>
          <button className={view === 'screenshot' ? 'active' : ''} onClick={() => choosePhotoView('screenshot')}><Camera size={14} />截图</button>
        </nav>}
        <button className="section-button" onClick={() => setPlayersOpen((open) => !open)} aria-expanded={playersOpen}>
          <Users size={16} /><span>好友</span><b>{friends.length}</b><ChevronDown className={playersOpen ? 'expanded' : ''} size={15} />
        </button>
        {playersOpen && <>
          <button className="friend-manage-button" onClick={() => setFriendManagerOpen(true)}><Users size={14} />管理好友</button>
          {friends.length > 0 && <label className="search-box"><Search size={14} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索好友" /></label>}
          <div className="player-list">
            {visibleFriends.map((player) => {
              const isSelf = player.userId === sessionStatus?.userId
              const reorderable = canReorderFriends && !isSelf
              return <button
                className={`player ${view === 'player' && player.userId === selectedId ? 'active' : ''} ${draggingFriendId === player.userId ? 'dragging' : ''} ${dragOverFriendId === player.userId ? 'drag-over' : ''}`}
                key={player.userId}
                type="button"
                draggable={reorderable}
                onClick={() => choosePlayer(player)}
                onDragStart={(event) => {
                  if (!reorderable) {
                    event.preventDefault()
                    return
                  }
                  event.dataTransfer.effectAllowed = 'move'
                  event.dataTransfer.setData('text/player-id', player.userId)
                  setDraggingFriendId(player.userId)
                }}
                onDragEnd={() => {
                  setDraggingFriendId(undefined)
                  setDragOverFriendId(undefined)
                }}
                onDragOver={(event) => {
                  if (!reorderable || !draggingFriendId || draggingFriendId === player.userId) return
                  event.preventDefault()
                  event.dataTransfer.dropEffect = 'move'
                  setDragOverFriendId(player.userId)
                }}
                onDragLeave={() => {
                  if (dragOverFriendId === player.userId) setDragOverFriendId(undefined)
                }}
                onDrop={(event) => {
                  event.preventDefault()
                  const fromId = event.dataTransfer.getData('text/player-id')
                  setDraggingFriendId(undefined)
                  setDragOverFriendId(undefined)
                  if (!reorderable || !fromId || isSelf) return
                  void reorderCuratedFriends(fromId, player.userId)
                }}
                title={player.previousNames.length ? `曾用名：${player.previousNames.join('、')}` : player.userId}
              >
                <img src={player.profilePicUrl || `${avatarFallback}${encodeURIComponent(player.userId)}`} alt="" />
                <span className="player-copy"><b className={trustClass(player.trustLevel)}>{displayPlayer(player)}</b><small>{isSelf ? '自己 · ' : ''}{player.photoCount} 张照片</small></span>
              </button>
            })}
            {!friends.length
              ? <div className="empty friend-empty">尚未选择精选好友<small>{sessionStatus?.status === 'active'
                ? '先点「立即同步」拉取 VRChat 好友，再在「管理好友」中精选'
                : '请先登录 VRChat，同步好友后再精选到左栏'}</small></div>
              : !visibleFriends.length && <div className="empty">没有匹配的好友</div>}
          </div>
        </>}
        <nav className="sidebar-footer">
          <button onClick={() => setSettingsOpen(true)}><Settings size={16} />设置</button>
        </nav>
      </aside>

      <section className="content">
        <header className="topbar">
          <div>
            <h1>{view === 'player' ? (selectedPlayer ? displayPlayer(selectedPlayer) : '玩家照片') : ({ all: '全部照片', album: '相册', screenshot: '截图' } as const)[view]}</h1>
            {selectedPlayer && view === 'player' && <p className="subline">{selectedPlayer.userId}</p>}
          </div>
          <div className="actions">
            <div className="sync-summary">
              <span>{syncing ? syncStatus?.message : `上次同步：${formatTime(lastSync?.at)}`}</span>
              <small>{syncing && syncStatus?.total
                ? `${syncStatus.current}/${syncStatus.total} · 成功 ${syncStatus.succeeded} · 失败 ${syncStatus.failed}`
                : lastSync?.success === false ? '上次同步存在错误' : '同步状态正常'}</small>
            </div>
            <div className={`top-auth ${sessionStatus?.status === 'expired' ? 'warn' : ''}`}>
              {sessionStatus?.status === 'active' && sessionStatus.profilePicUrl && <img src={sessionStatus.profilePicUrl} alt="" />}
              <span>{sessionStatus?.status === 'active'
                ? sessionStatus.displayName ?? '已登录'
                : sessionStatus?.status === 'expired' ? '登录已过期' : '未登录'}</span>
              <button onClick={() => setLoginOpen(true)}>{sessionStatus?.status === 'active' ? '账号' : '登录'}</button>
            </div>
            <button onClick={() => setSettingsOpen(true)}><FolderOpen size={15} />导入目录</button>
            <button className="primary" onClick={() => void sync()} disabled={syncing}><Cloud size={15} />{syncing ? '同步中…' : '立即同步'}</button>
          </div>
        </header>
        {notice && <div className="notice" role="status">{notice}<button onClick={() => setNotice('')}>×</button></div>}

        {view === 'player' && selectedPlayer && <section className="profile-card">
          <img className="profile-avatar" src={selectedPlayer.profilePicUrl || `${avatarFallback}${encodeURIComponent(selectedPlayer.userId)}`} alt="" />
          <div><h2 className={trustClass(selectedPlayer.trustLevel)}>{displayPlayer(selectedPlayer)}</h2>
            <p>{selectedPlayer.vrcxMemo || (selectedPlayer.trustLevel ? `信任等级：${selectedPlayer.trustLevel}` : '已关联 VRChat 玩家')}</p>
            {selectedPlayer.userId !== sessionStatus?.userId && <p className="source">资料来源：{selectedPlayer.source === 'api' ? 'VRChat API' : selectedPlayer.source === 'vrcx' ? '历史 VRCX' : '本地'}{selectedPlayer.isVrchatFriend ? ' · 当前好友' : selectedPlayer.isFriend ? ' · 已解除好友（仍精选）' : ''}</p>}
          </div>
          <a href={`https://vrchat.com/home/user/${selectedPlayer.userId}`} target="_blank" rel="noreferrer">打开 VRChat 资料页</a>
        </section>}

        <div className="gallery-title">
          <h2>照片 <span>{visiblePhotos.length}</span></h2>
          <div className="filters">
            <select value={sourceFilter} onChange={(event) => setSourceFilter(event.target.value)} aria-label="照片来源">
              <option value="">全部来源</option><option value="local">本地</option><option value="vrchat_gallery">VRChat 相册</option><option value="vrchat_print">VRChat 拍立得</option>
            </select>
          </div>
        </div>
        {(selectionMode || selectedPhotos.size > 0) && <div className="selection-bar">
          <span>{selectedPhotos.size ? `已选择 ${selectedPhotos.size} 张` : '多选模式：请选择照片'}</span>
          {associationPreset.size > 0 && <small>目标：{[...associationPreset].map((id) => players.find((player) => player.userId === id)?.displayName ?? id).join('、')}</small>}
          <button className="selection-action" disabled={!selectedPhotos.size} onClick={() => openAssociation([...selectedPhotos])}><Users size={13} />关联好友…</button>
          <button onClick={() => {
            setSelectedPhotos(new Set())
            setSelectionMode(false)
            setAssociationPreset(new Set())
          }}>退出多选</button>
        </div>}
        <section className={`photo-grid ${selectionMode ? 'selection-mode' : ''}`}>
          {visiblePhotos.map((photo, index) => <article className={`photo-card ${selectedPhotos.has(photo.id) ? 'selected' : ''}`} key={photo.id}>
            <button className="photo-preview" onClick={() => selectionMode ? togglePhoto(photo.id) : setPreviewIndex(index)}>
              <img src={photo.thumbnailPath || photo.remoteUrl || photo.localPath} alt="" loading="lazy" />
              <Maximize2 className="zoom-icon" size={17} />
            </button>
            <label className="photo-select" onClick={(event) => event.stopPropagation()}>
              <input type="checkbox" checked={selectedPhotos.has(photo.id)} onChange={() => togglePhoto(photo.id)} />
            </label>
            <span className="photo-info"><small>{photo.kind === 'screenshot' ? 'Steam 截图' : photo.source === 'vrchat_gallery' ? 'VRChat 相册' : photo.source === 'vrchat_print' ? 'VRChat 拍立得' : '相册'}{photo.people.length ? ` · ${photo.people.length} 位玩家` : ' · 未关联'}</small></span>
          </article>)}
          {!loading && !visiblePhotos.length && <div className="empty gallery-empty"><Images size={28} />这里还没有图片
            {view === 'player' && selectedId
              ? <><small>从全部照片中选择要关联给这位好友的图片。</small><button className="empty-action" onClick={startAssociatingSelectedFriend}>关联照片</button></>
              : <small>在设置中添加相册或 Steam 截图目录。</small>}
          </div>}
          {loading && <div className="empty gallery-empty"><LoaderCircle className="spin" size={24} />加载照片中</div>}
        </section>
      </section>

      {preview && <div className="preview-backdrop" onMouseDown={() => setPreviewIndex(undefined)}>
        <section className="preview-modal" onMouseDown={(event) => event.stopPropagation()}>
          <button className="preview-close" onClick={() => setPreviewIndex(undefined)}><X /></button>
          <button className="preview-arrow left" disabled={previewIndex === 0} onClick={() => setPreviewIndex((previewIndex ?? 1) - 1)}><ChevronLeft /></button>
          <img src={preview.localPath || preview.remoteUrl} alt="" />
          <button className="preview-arrow right" disabled={previewIndex === visiblePhotos.length - 1} onClick={() => setPreviewIndex((previewIndex ?? -1) + 1)}><ChevronRight /></button>
          <footer><div><small>{preview.capturedAt || '拍摄时间未知'}</small></div>
            <div className="preview-actions">
              {preview.people.map((id) => <span key={id}>{displayPlayer(players.find((player) => player.userId === id) ?? { userId: id, displayName: id, source: 'local', previousNames: [], photoCount: 0, isFriend: false, isVrchatFriend: false, sortOrder: 0 })}</span>)}
              <button onClick={() => openAssociation([preview.id])}><Users size={14} />关联好友</button>
              {view === 'player' && selectedId && preview.people.includes(selectedId) && <button onClick={async () => { await api.unassignPhoto(preview.id, selectedId); setPreviewIndex(undefined); await refreshPhotos() }}><Unlink size={14} />移除关联</button>}
              <button onClick={() => void api.openPhoto(preview).catch((error) => setNotice(errorMessage(error, '打开原文件失败')))}><FolderOpen size={14} />打开原文件</button>
            </div>
          </footer>
        </section>
      </div>}

      {settingsOpen && <div className="modal-backdrop" onMouseDown={() => setSettingsOpen(false)}>
        <section className="settings-modal" onMouseDown={(event) => event.stopPropagation()}>
          <div className="modal-heading"><h2>设置</h2><button onClick={() => setSettingsOpen(false)}>×</button></div>
          <h3>照片目录</h3>
          <label>相册目录<span className="directory-field"><input value={settings.albumFolder ?? ''} onChange={(event) => setSettings({ ...settings, albumFolder: event.target.value })} placeholder="C:\\Users\\你\\Pictures\\VRChat" /><button type="button" onClick={async () => {
            const path = await api.chooseDirectory(settings.albumFolder)
            if (path) setSettings((current) => ({ ...current, albumFolder: path }))
          }}>选择…</button></span></label>
          <label>Steam 目录<span className="directory-field"><input value={settings.steamScreenshotFolder ?? ''} onChange={(event) => setSettings({ ...settings, steamScreenshotFolder: event.target.value })} placeholder="请选择 Steam、userdata 或 screenshots 目录" /><button type="button" onClick={async () => {
            const path = await api.chooseDirectory(settings.steamScreenshotFolder)
            if (path) setSettings((current) => ({ ...current, steamScreenshotFolder: path }))
          }}>选择…</button></span></label>
          <p className="help">相册会递归扫描 YYYY-MM 等子目录。Steam 会自动识别 userdata，并只扫描各用户 VRChat（App 438100）截图目录下的文件（不含 thumbnails 等子文件夹）。</p>
          <label>同步间隔（分钟）<input type="number" min="5" value={settings.syncIntervalMinutes} onChange={(event) => setSettings({ ...settings, syncIntervalMinutes: Number(event.target.value) })} /></label>
          <label className="setting-toggle"><span>在好友栏顶部显示自己</span><input type="checkbox" checked={settings.showSelfInFriends} onChange={(event) => setSettings({ ...settings, showSelfInFriends: event.target.checked })} /></label>
          <button className="primary wide" onClick={() => void saveAndScan()}><FolderOpen size={16} />保存并扫描目录</button>
        </section>
      </div>}

      {loginOpen && <div className="modal-backdrop" onMouseDown={() => setLoginOpen(false)}>
        <section className="settings-modal login-modal" onMouseDown={(event) => event.stopPropagation()}>
          <div className="modal-heading"><h2>VRChat 登录</h2><button onClick={() => setLoginOpen(false)}>×</button></div>
          {sessionStatus?.status === 'active' && !twoFactorMethods.length ? <div className="auth-card">
            {sessionStatus.profilePicUrl && <img src={sessionStatus.profilePicUrl} alt="" />}
            <div><b>{sessionStatus.displayName}</b><small>{sessionStatus.userId}</small></div>
            <span>会话有效</span>
          </div> : <p className={`help auth-state ${sessionStatus?.status === 'expired' ? 'error' : ''}`}>
            {sessionStatus?.message ?? '正在检查 VRChat 登录状态…'}
          </p>}
          {authFeedback && <p className={`auth-feedback ${authFeedback.error ? 'error' : 'ok'}`} role="status">{authFeedback.message}</p>}
          {sessionStatus?.status !== 'active' && !twoFactorMethods.length ? <>
            <label>用户名<input value={username} onChange={(event) => setUsername(event.target.value)} autoComplete="username" /></label>
            <label>密码<input type="password" value={password} onChange={(event) => setPassword(event.target.value)} autoComplete="current-password" /></label>
            <button className="secondary wide" disabled={!username || !password || loggingIn} onClick={() => void login()}><Cloud size={16} />{loggingIn ? '登录中…' : '登录 VRChat'}</button>
          </> : twoFactorMethods.length ? <>
            <label>两步验证码（{twoFactorMethods[0]}）<input value={twoFactorCode} onChange={(event) => setTwoFactorCode(event.target.value)} inputMode="numeric" autoFocus /></label>
            <button className="primary wide" disabled={!twoFactorCode || loggingIn} onClick={() => void verifyTwoFactor()}>{loggingIn ? '验证中…' : '验证'}</button>
          </> : null}
          {sessionStatus && sessionStatus.status !== 'loggedOut' && <button className="text-button" onClick={async () => {
            await api.logoutVrchat()
            setSessionStatus(await api.vrchatSessionStatus())
            setAuthFeedback({ message: '已退出 VRChat 登录', error: false })
          }}><LogOut size={14} />退出并清除安全凭据</button>}
          <div className="sync-details">
            <b>{syncStatus?.running ? syncStatus.message : lastSync?.message ?? '尚无同步记录'}</b>
            <small>{syncStatus?.running && syncStatus.total
              ? `进度 ${syncStatus.current}/${syncStatus.total} · 成功 ${syncStatus.succeeded} · 失败 ${syncStatus.failed}`
              : `最后同步：${formatTime(lastSync?.at)}`}</small>
          </div>
          <p className="help">会话保存在 Windows Credential Manager，不写入照片数据库。</p>
        </section>
      </div>}

      {friendManagerOpen && <div className="modal-backdrop" onMouseDown={() => setFriendManagerOpen(false)}>
        <section className="settings-modal friend-manager-modal" onMouseDown={(event) => event.stopPropagation()}>
          <div className="modal-heading"><div><h2>管理好友</h2><small>从 VRChat 好友中精选显示在左栏的玩家；解除好友不会自动取消精选</small></div><button onClick={() => setFriendManagerOpen(false)}>×</button></div>
          <label className="friend-manager-search"><Search size={15} /><input value={friendQuery} onChange={(event) => setFriendQuery(event.target.value)} placeholder="搜索备注、昵称、曾用名或 ID" autoFocus /></label>
          <div className="friend-manager-list">
            {managedPlayers.map((player) => <label className="friend-manager-item" key={player.userId}>
              <input type="checkbox" checked={player.isFriend} onChange={() => void toggleFriend(player)} />
              <img src={player.profilePicUrl || `${avatarFallback}${encodeURIComponent(player.userId)}`} alt="" />
              <span><b className={trustClass(player.trustLevel)}>{displayPlayer(player)}</b><small>{friendStatusLabel(player)} · {player.userId}</small></span>
            </label>)}
            {!managedPlayers.length && <div className="empty">{sessionStatus?.status === 'active'
              ? '暂无候选玩家。请先点「立即同步」拉取 VRChat 好友。'
              : '请先登录 VRChat，再同步好友列表。'}</div>}
          </div>
        </section>
      </div>}

      {associationPhotoIds && <div className="modal-backdrop association-backdrop" onMouseDown={() => setAssociationPhotoIds(undefined)}>
        <section className="settings-modal association-modal" onMouseDown={(event) => event.stopPropagation()}>
          <div className="modal-heading"><div><h2>关联好友</h2><small>为 {associationPhotoIds.length} 张照片选择一个或多个精选好友</small></div><button onClick={() => setAssociationPhotoIds(undefined)}>×</button></div>
          {curatedFriends.length ? <div className="friend-manager-list association-friend-list">
            {curatedFriends.map((player) => <label className="friend-manager-item" key={player.userId}>
              <input type="checkbox" checked={associationFriends.has(player.userId)} onChange={() => setAssociationFriends((current) => {
                const next = new Set(current)
                if (next.has(player.userId)) next.delete(player.userId); else next.add(player.userId)
                return next
              })} />
              <img src={player.profilePicUrl || `${avatarFallback}${encodeURIComponent(player.userId)}`} alt="" />
              <span><b className={trustClass(player.trustLevel)}>{displayPlayer(player)}</b><small>{player.photoCount} 张已关联照片</small></span>
            </label>)}
          </div> : <div className="empty association-empty">尚未选择任何好友<button className="empty-action" onClick={() => {
            setAssociationPhotoIds(undefined)
            setFriendManagerOpen(true)
          }}>管理好友</button></div>}
          <footer className="association-footer">
            <button className="secondary" onClick={() => setAssociationPhotoIds(undefined)}>取消</button>
            <button className="primary" disabled={!associationFriends.size || associating} onClick={() => void confirmAssociation()}>{associating ? '关联中…' : `关联到 ${associationFriends.size} 位好友`}</button>
          </footer>
        </section>
      </div>}
    </main>
  )
}

export default App
