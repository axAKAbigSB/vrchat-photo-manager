# VRC Album

Windows desktop photo manager for VRChat. Photos remain local; player associations use VRChat `userId`, so display-name changes never break albums.

## Features

- Browse photos by player in a searchable sidebar.
- Read current names and alias history from VRCX **read-only**.
- Scan a local photo folder. Files inside a `usr_xxx` directory associate automatically; the rest stay unclassified.
- Sync the signed-in account's VRChat Gallery.
- Refresh other tracked players' current profile/avatar images only. It does not fetch other players' Gallery contents.

## Run

```powershell
npm install
npm run tauri dev
```

The frontend alone can be previewed with `npm run dev`.

## Privacy and authentication

VRCX's SQLite database is never modified and its session cookie is never read. VRChat API access uses a separate app-owned session. The app stores that session in its local SQLite database; production distribution should replace that storage with an OS credential vault before using it with a shared machine.
