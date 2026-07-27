import { convertFileSrc, invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { openPath, openUrl } from '@tauri-apps/plugin-opener'

export interface Player {
  userId: string
  displayName: string
  profilePicUrl?: string
  avatarThumbnailUrl?: string
  trustLevel?: string
  note?: string
  vrcxMemo?: string
  source: 'vrcx' | 'api' | 'local'
  previousNames: string[]
  photoCount: number
  lastSyncedAt?: string
  isFriend: boolean
  isVrchatFriend: boolean
  sortOrder: number
}

export interface Photo {
  id: number
  userId?: string
  source: 'local' | 'vrchat_gallery' | 'vrchat_print' | 'vrchat_avatar'
  kind: 'album' | 'screenshot'
  localPath?: string
  originalPath?: string
  remoteUrl?: string
  thumbnailPath?: string
  fileName: string
  capturedAt?: string
  people: string[]
}

export interface AppSettings {
  albumFolder?: string
  steamScreenshotFolder?: string
  syncIntervalMinutes: number
  showSelfInFriends: boolean
}

export interface LoginResult {
  authenticated: boolean
  requiresTwoFactorAuth: string[]
  message: string
}

export interface VrchatSessionStatus {
  status: 'loggedOut' | 'active' | 'expired' | 'error'
  displayName?: string
  userId?: string
  profilePicUrl?: string
  message: string
}

export interface SyncStatus {
  running: boolean
  phase: 'idle' | 'starting' | 'friends' | 'profiles' | 'gallery' | 'done' | 'failed' | 'expired'
  current: number
  total: number
  succeeded: number
  failed: number
  galleryCount: number
  message: string
  startedAt?: string
  finishedAt?: string
}

export interface LastSync {
  at?: string
  message?: string
  success?: boolean
}

const isTauri = '__TAURI_INTERNALS__' in window

const demoPlayers: Player[] = [
  {
    userId: 'usr_demo_self',
    displayName: '我的 VRChat 相册',
    source: 'local',
    previousNames: [],
    photoCount: 0,
    isFriend: false,
    isVrchatFriend: false,
    sortOrder: 0,
  },
]

export const api = {
  async listPlayers(): Promise<Player[]> {
    return isTauri ? invoke<Player[]>('list_players') : demoPlayers
  },
  async listPhotos(userId?: string, kind?: 'album' | 'screenshot'): Promise<Photo[]> {
    const photos = isTauri ? await invoke<Photo[]>('list_photos', { userId, kind }) : []
    return photos.map((photo) => ({
      ...photo,
      originalPath: photo.localPath,
      localPath: photo.localPath ? convertFileSrc(photo.localPath) : undefined,
      thumbnailPath: photo.thumbnailPath ? convertFileSrc(photo.thumbnailPath) : undefined,
    }))
  },
  async startSync(): Promise<SyncStatus> {
    if (!isTauri) return { running: false, phase: 'done', current: 0, total: 0, succeeded: 0, failed: 0, galleryCount: 0, message: '开发预览模式' }
    return invoke<SyncStatus>('start_sync')
  },
  async getSyncStatus(): Promise<SyncStatus> {
    if (!isTauri) return { running: false, phase: 'idle', current: 0, total: 0, succeeded: 0, failed: 0, galleryCount: 0, message: '开发预览模式' }
    return invoke<SyncStatus>('get_sync_status')
  },
  async onSyncProgress(callback: (status: SyncStatus) => void): Promise<UnlistenFn> {
    if (!isTauri) return () => undefined
    return listen<SyncStatus>('sync-progress', (event) => callback(event.payload))
  },
  async getLastSync(): Promise<LastSync> {
    return isTauri ? invoke<LastSync>('get_last_sync') : {}
  },
  async vrchatSessionStatus(): Promise<VrchatSessionStatus> {
    return isTauri
      ? invoke<VrchatSessionStatus>('vrchat_session_status')
      : { status: 'loggedOut', message: '开发预览模式' }
  },
  async scanPhotoFolder(path: string, kind: 'album' | 'screenshot'): Promise<number> {
    if (!isTauri) return 0
    return invoke<number>('scan_photo_folder', { path, kind })
  },
  async getSettings(): Promise<AppSettings> {
    return isTauri
      ? invoke<AppSettings>('get_settings')
      : { albumFolder: 'D:\\VRChatPhotos', syncIntervalMinutes: 15, showSelfInFriends: true }
  },
  async saveSettings(settings: AppSettings): Promise<void> {
    if (isTauri) await invoke('save_settings', { settings })
  },
  async chooseDirectory(current?: string): Promise<string | undefined> {
    if (!isTauri) return undefined
    const selected = await openDialog({
      directory: true,
      multiple: false,
      defaultPath: current || undefined,
    })
    return typeof selected === 'string' ? selected : undefined
  },
  async assignPhotos(photoIds: number[], userId: string): Promise<number> {
    return isTauri ? invoke<number>('assign_photos', { photoIds, userId }) : 0
  },
  async assignPhotosToFriends(photoIds: number[], userIds: string[]): Promise<number> {
    return isTauri ? invoke<number>('assign_photos_to_friends', { photoIds, userIds }) : 0
  },
  async unassignPhoto(photoId: number, userId: string): Promise<void> {
    if (isTauri) await invoke('unassign_photo', { photoId, userId })
  },
  async setFriend(userId: string, selected: boolean): Promise<void> {
    if (isTauri) await invoke('set_friend', { userId, selected })
  },
  async reorderFriends(userIds: string[]): Promise<void> {
    if (isTauri) await invoke('reorder_friends', { userIds })
  },
  async loginVrchat(username: string, password: string): Promise<LoginResult> {
    if (!isTauri) return { authenticated: true, requiresTwoFactorAuth: [], message: '开发预览模式' }
    return invoke<LoginResult>('login_vrchat', { username, password })
  },
  async verifyTwoFactor(method: string, code: string): Promise<LoginResult> {
    if (!isTauri) return { authenticated: true, requiresTwoFactorAuth: [], message: '开发预览模式' }
    return invoke<LoginResult>('verify_two_factor', { method, code })
  },
  async logoutVrchat(): Promise<void> {
    if (isTauri) await invoke('logout_vrchat')
  },
  async openPhoto(photo: Photo): Promise<void> {
    if (isTauri && photo.originalPath) await openPath(photo.originalPath)
    else if (photo.remoteUrl) await openUrl(photo.remoteUrl)
  },
}
