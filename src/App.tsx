import { useEffect, useMemo, useState } from 'react'
import {
  Cloud, FolderOpen, ImagePlus, LoaderCircle, Search, Settings, Users,
} from 'lucide-react'
import { api, type Photo, type Player } from './lib/api'
import './App.css'

const avatarFallback = 'https://api.dicebear.com/9.x/shapes/svg?seed='

function App() {
  const [players, setPlayers] = useState<Player[]>([])
  const [selectedId, setSelectedId] = useState<string>()
  const [photos, setPhotos] = useState<Photo[]>([])
  const [query, setQuery] = useState('')
  const [loading, setLoading] = useState(true)
  const [syncing, setSyncing] = useState(false)
  const [notice, setNotice] = useState('')
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [photoFolder, setPhotoFolder] = useState('D:\\VRChatPhotos')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')

  const selected = players.find((player) => player.userId === selectedId)
  const visiblePlayers = useMemo(() => {
    const normalized = query.trim().toLowerCase()
    if (!normalized) return players
    return players.filter((player) =>
      [player.displayName, player.userId, ...player.previousNames]
        .some((value) => value.toLowerCase().includes(normalized)),
    )
  }, [players, query])

  const refresh = async () => {
    setLoading(true)
    try {
      const nextPlayers = await api.listPlayers()
      setPlayers(nextPlayers)
      setSelectedId((current) => current ?? nextPlayers[0]?.userId)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { void refresh() }, [])
  useEffect(() => {
    if (!selectedId) return
    void api.listPhotos(selectedId).then(setPhotos)
  }, [selectedId])

  const sync = async () => {
    setSyncing(true)
    try {
      const result = await api.syncNow()
      setNotice(result)
      await refresh()
    } catch (error) {
      setNotice(error instanceof Error ? error.message : '同步失败')
    } finally {
      setSyncing(false)
    }
  }

  const scan = async () => {
    try {
      const count = await api.scanPhotoFolder(photoFolder)
      setNotice(`扫描完成：已索引 ${count} 张本地照片。`)
      await refresh()
    } catch (error) {
      setNotice(error instanceof Error ? error.message : '扫描失败')
    }
  }

  const login = async () => {
    try {
      setNotice(await api.loginVrchat(username, password))
      setPassword('')
    } catch (error) {
      setNotice(error instanceof Error ? error.message : '登录失败')
    }
  }

  return (
    <main className="app-shell">
      <aside className="sidebar">
        <div className="brand"><span className="brand-mark">V</span><span>VRC Album</span></div>
        <div className="sidebar-heading"><span>玩家</span><span className="count">{players.length}</span></div>
        <label className="search-box"><Search size={16} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索昵称或 ID" /></label>
        <div className="player-list">
          {loading ? <div className="empty"><LoaderCircle className="spin" size={20} />读取资料中</div> : visiblePlayers.map((player) => (
            <button className={`player ${player.userId === selectedId ? 'active' : ''}`} key={player.userId} onClick={() => setSelectedId(player.userId)}>
              <img src={player.profilePicUrl || `${avatarFallback}${encodeURIComponent(player.userId)}`} alt="" />
              <span className="player-copy"><b>{player.displayName}</b><small>{player.photoCount} 张照片</small></span>
            </button>
          ))}
          {!loading && !visiblePlayers.length && <div className="empty">没有匹配的玩家</div>}
        </div>
        <nav className="sidebar-footer">
          <button onClick={() => setSettingsOpen(true)}><FolderOpen size={17} />照片目录</button>
          <button onClick={() => setSettingsOpen(true)}><Settings size={17} />设置</button>
        </nav>
      </aside>

      <section className="content">
        <header className="topbar">
          <div>
            <p className="eyebrow">VRCHAT PHOTO MANAGER</p>
            <h1>{selected?.displayName ?? '选择一位玩家'}</h1>
            {selected && <p className="subline">{selected.userId}{selected.previousNames.length ? ` · 曾用名：${selected.previousNames.join('、')}` : ''}</p>}
          </div>
          <div className="actions">
            <button className="secondary" onClick={() => setNotice('导入功能将在连接本地目录后启用。')}><ImagePlus size={17} />导入照片</button>
            <button className="primary" onClick={() => void sync()} disabled={syncing}><Cloud size={17} />{syncing ? '同步中…' : '立即同步'}</button>
          </div>
        </header>

        {notice && <div className="notice" role="status">{notice}<button onClick={() => setNotice('')}>×</button></div>}

        <section className="profile-card">
          {selected ? <><img className="profile-avatar" src={selected.profilePicUrl || `${avatarFallback}${encodeURIComponent(selected.userId)}`} alt="" />
            <div><h2>{selected.displayName}</h2><p>{selected.trustLevel ? `信任等级：${selected.trustLevel}` : '已关联 VRChat 玩家'}</p><p className="source">资料来源：{selected.source === 'vrcx' ? 'VRCX 本地资料' : 'VRChat API'}</p></div>
            <a href={`https://vrchat.com/home/user/${selected.userId}`} target="_blank" rel="noreferrer">打开 VRChat 资料页</a></> :
            <div className="empty"><Users size={25} />从侧栏选择玩家</div>}
        </section>

        <div className="gallery-title"><h2>照片 <span>{photos.length}</span></h2><p>本地文件与自己的 VRChat Gallery</p></div>
        <section className="photo-grid">
          {photos.map((photo) => <button className="photo-card" key={photo.id} onClick={() => window.open(photo.remoteUrl || photo.localPath, '_blank')}>
            <img src={photo.thumbnailPath || photo.remoteUrl || photo.localPath} alt={photo.fileName} loading="lazy" />
            <span className="photo-info"><b>{photo.fileName}</b><small>{photo.source === 'vrchat_gallery' ? 'VRChat Gallery' : '本地照片'}</small></span>
          </button>)}
          {!photos.length && selected && <div className="empty gallery-empty"><ImagePlus size={28} />还没有与 {selected.displayName} 关联的照片<br /><small>添加 D 盘照片目录后，或将未分类照片拖到这里。</small></div>}
        </section>
      </section>
      {settingsOpen && <div className="modal-backdrop" role="presentation" onMouseDown={() => setSettingsOpen(false)}>
        <section className="settings-modal" role="dialog" aria-modal="true" aria-label="设置" onMouseDown={(event) => event.stopPropagation()}>
          <div className="modal-heading"><h2>设置</h2><button onClick={() => setSettingsOpen(false)}>×</button></div>
          <label>本地照片目录<input value={photoFolder} onChange={(event) => setPhotoFolder(event.target.value)} placeholder="D:\\VRChatPhotos" /></label>
          <button className="primary wide" onClick={() => void scan()}><FolderOpen size={17} />扫描并索引照片</button>
          <p className="help">使用 <code>{'{userId}'}</code> 子目录可自动关联玩家；其他照片会保留为未分类。</p>
          <hr />
          <label>VRChat 用户名<input value={username} onChange={(event) => setUsername(event.target.value)} autoComplete="username" /></label>
          <label>VRChat 密码<input type="password" value={password} onChange={(event) => setPassword(event.target.value)} autoComplete="current-password" /></label>
          <button className="secondary wide" disabled={!username || !password} onClick={() => void login()}><Cloud size={17} />登录并同步自己的 Gallery</button>
          <p className="help">登录用于同步你自己的 VRChat Gallery，以及在 VRCX 不可用时刷新他人的当前头像和昵称。</p>
        </section>
      </div>}
    </main>
  )
}

export default App
