import { convertFileSrc, invoke } from '@tauri-apps/api/core'

export interface Player {
  userId: string
  displayName: string
  profilePicUrl?: string
  avatarThumbnailUrl?: string
  trustLevel?: string
  source: 'vrcx' | 'api' | 'local'
  previousNames: string[]
  photoCount: number
}

export interface Photo {
  id: number
  userId?: string
  source: 'local' | 'vrchat_gallery' | 'vrchat_avatar'
  localPath?: string
  remoteUrl?: string
  thumbnailPath?: string
  fileName: string
  capturedAt?: string
}

const isTauri = '__TAURI_INTERNALS__' in window

const demoPlayers: Player[] = [
  { userId: 'usr_demo_self', displayName: '我的 VRChat 相册', source: 'local', previousNames: [], photoCount: 0 },
]

export const api = {
  async listPlayers(): Promise<Player[]> {
    return isTauri ? invoke<Player[]>('list_players') : demoPlayers
  },
  async listPhotos(userId: string): Promise<Photo[]> {
    const photos = isTauri ? await invoke<Photo[]>('list_photos', { userId }) : []
    return photos.map((photo) => ({
      ...photo,
      localPath: photo.localPath ? convertFileSrc(photo.localPath) : undefined,
      thumbnailPath: photo.thumbnailPath ? convertFileSrc(photo.thumbnailPath) : undefined,
    }))
  },
  async syncNow(): Promise<string> {
    if (!isTauri) return '开发预览模式：Tauri 后端启动后可执行真实同步。'
    return invoke<string>('sync_now')
  },
  async scanPhotoFolder(path: string): Promise<number> {
    if (!isTauri) return 0
    return invoke<number>('scan_photo_folder', { path })
  },
  async loginVrchat(username: string, password: string): Promise<string> {
    if (!isTauri) return '开发预览模式不会发送 VRChat 登录请求。'
    return invoke<string>('login_vrchat', { username, password })
  },
}
