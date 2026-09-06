# nocheater-nfa-loader

Public standalone desktop Account Center for [sitnn.dog](https://sitnn.dog) — Steam token import/login, license status, redeem, replacement, and History. Store: [nocheater.store](https://nocheater.store).

This is the **public** repo (`nocheater-nfa-loader`). It is not the private full-featured `nfa-loader`.

The shipped exe is self-contained Rust/Tauri. **No Node.js**, no checker scripts, and no extra sidecars at runtime (WebView2 only).

## Run (dev)

```powershell
npm i
$env:Path = "$env:USERPROFILE\.cargo\bin;" + $env:Path
npm run tauri dev
```

Requires [Rust](https://rustup.rs/), [Node.js](https://nodejs.org/) 18+ (Tauri CLI only — not used by the exe), and WebView2 (Windows).

Build installer / portable:

```powershell
.\scripts\build-release.ps1
```

Or:

```powershell
npm run tauri build
```

## Features

- **Login** — Import `username----token` (optional `----key:value…` meta is accepted and ignored for export). Closes Steam, writes ConnectCache, logs in. License status via `GET /api/status?key=`.
- **History** — Steam accounts. Refresh, Sign in, Remove, Export (`username----token`), Clear all.
- **Redeem / Replacement** — sitnn.dog `{ ok }` API from Rust (not the webview).
- **Settings** — optional Run as administrator if Steam kill fails (Access denied), Quit.
- Top right: **Market** → nocheater.store, **Redeem** → sitnn.dog, **Discord** → nocheater.cc, **Close Steam**.
- Steam is **not** auto-started when the loader opens — only on Import & Login / Sign in.
- Import / Sign in write **Invisible** persona into `localconfig.vdf` before Steam launches.

## Single instance

Windows named mutex `Local\nocheater.desktop.single-instance`. A second launch focuses the existing window and exits.

## sitnn.dog API

| Tab | Call |
|-----|------|
| Account status | `GET /api/status?key=` |
| Redeem | `POST /api/redeem` `{ "key" }` → may include `account` (`username----token`) |
| Replacement | `POST /api/replacement` `{ "key", "reason" }` |

Optional header `X-App-Key` from `%APPDATA%\nocheater\config.json` (`appKey`). Empty = omit. After the sitnn.dog APP_KEY policy update, empty `appKey` works unless the server sets `APP_KEY_REQUIRED=true`. Wrong `appKey` still gets **Unauthorized**.

Timeout 20s. On network/site-down failures the app toasts **Site not ready** and opens nocheater.store. If the delivery provider rejects the site's API key, the app shows a delivery-API message (not Site not ready).

## Config

`%APPDATA%\nocheater\config.json`:

```json
{
  "appKey": "",
  "lastKey": "",
  "lastToken": ""
}
```

Also: `%APPDATA%\nocheater\accounts.json` (stored Steam JWTs).
