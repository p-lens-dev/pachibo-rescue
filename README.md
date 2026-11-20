# pachibo-rescue

> **The Japanese text is the original.** [README.ja.md](README.ja.md) is the original; this English text is a translation, and where the two differ the Japanese holds.

A desktop app that takes the records of ぱち簿 — an iPhone app for pachinko
and pachislot results, not updated since 2020 — out of an iPhone backup, and
saves them as ぱち簿's own export (`.pcb`), which P-Lens takes in at
設定 → インポート → ぱち簿.

- It reads only the iPhone backups on the computer; it never talks to the phone.
- It sends nothing anywhere.
- It runs on Windows, macOS and Linux.

## Using it

1. Connect the iPhone and make a backup: Finder on a Mac, Apple Devices (or
   iTunes) on Windows, `idevicebackup2 backup --full <folder>` on Linux.
2. Open pachibo-rescue: it lists the backups. On Linux, choose the backup's
   folder with フォルダを選ぶ.
3. Press 取り出す and choose where to save. For an encrypted backup, type its
   password first.

## Opening it the first time

The app is not signed by Apple or Microsoft.

- **macOS**: when it is stopped, press "Open Anyway" under System Settings →
  Privacy & Security. If it says the app is damaged, run
  `xattr -cr /Applications/pachibo-rescue.app` and open it again.
- **macOS, Full Disk Access**: to read the backup folder, turn pachibo-rescue
  on under System Settings → Privacy & Security → Full Disk Access, then
  reopen the app.
- **Windows**: at "Windows protected your PC", press "More info" → "Run anyway".
- **Linux**: make the AppImage executable (`chmod +x pachibo-rescue_*.AppImage`).

## Developing

```sh
bun install
bun run dev                       # Tauri in development
bun run check                     # tsc
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

- `crates/ios-backup`: finding backups, reading what they say of themselves,
  unlocking encrypted ones with their password, and taking one app's files out.
- `crates/pachibo`: ぱち簿's `Documents/data.plist` written as a `.pcb`.
- `src-tauri`, `src`: the desktop app (Tauri 2 and Solid).

By hand (`BACKUP_PASSWORD` for an encrypted backup):

```sh
cargo run -p ios-backup --example inspect -- <backup folder>
cargo run -p pachibo --example rescue -- <backup folder> pachibo.pcb
```

Pushing a `v*` tag has GitHub Actions build the macOS (universal), Windows and
Linux apps into a draft Release.

## Licence

GPL-3.0-or-later.
