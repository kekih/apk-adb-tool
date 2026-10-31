# apk-adb-tool

Search apps **on the internet** (F-Droid), download APK, install on Android via ADB.

## Flow

1. Connect phone (USB debugging)
2. Search app name (e.g. `newpipe`, `firefox`, `telegram`)
3. Select → **Download APK** → **Install on phone**

Or paste a direct `.apk` URL (APKMirror etc.) for apps not on F-Droid.

## Note

- **F-Droid** = open-source apps (works fully automated)
- Store-only apps (PUBG, Instagram…): use **paste APK URL** — third-party mirrors block automated scrapers (Cloudflare)

## Build

GitHub Actions builds on every push. Download `.exe` from **Actions → Artifacts**.

```bash
cargo run --release
```
