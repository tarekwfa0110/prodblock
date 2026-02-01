# Prodblock

Windows desktop focus app: block distracting apps and websites during activities.

## Features

- **Activity Management** - Define activities with allowed apps/websites
- **Time-Based Suggestions** - Shows 3 activities closest to current time
- **App Blocking** - Minimizes non-whitelisted apps during focus
- **Website Blocking** - Proxy blocks non-whitelisted domains + browser extension overlay
- **Full Focus Mode** - Empty allowlist = block everything

## Tech Stack

- **Tauri 2** (Rust backend + web frontend)
- **Windows** only (uses Win32 APIs for foreground watcher)

## Setup

1. Install [Rust](https://rustup.rs/) and [Node.js](https://nodejs.org/)
2. Open **VS Developer Command Prompt** (required for RC.EXE)
3. From the project root:

```bash
npm install
npm run tauri dev
```

## Browser Extension

For website blocking overlay:

1. Open `chrome://extensions`
2. Enable "Developer mode"
3. Click "Load unpacked" → select `extension/`

## Data

- Activities: `%APPDATA%/prodblock/activities.json`
- Run at startup: Registry `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\prodblock`

## License

MIT