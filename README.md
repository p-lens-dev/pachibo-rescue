# pachibo-rescue

> **The Japanese text is the original.** [README.ja.md](README.ja.md) is the original; this English text is a translation, and where the two differ the Japanese holds.

A desktop app that takes the records of ぱち簿 — an iPhone app for pachinko
and pachislot results, not updated since 2020 — out of an iPhone backup, and
saves them as ぱち簿's own export (`.pcb`), which P-Lens takes in at
設定 → インポート → ぱち簿.

- It reads only the iPhone backups on the computer; it never talks to the phone.
- It sends nothing anywhere.
- It is released for Windows and macOS; downloads and full instructions are under Releases.

## Using it

Do not delete ぱち簿 from the iPhone: deleting it deletes its records from the phone, and no backup holds them after.

1. Connect the iPhone and make a backup: Finder on a Mac, Apple Devices (or
   iTunes) on Windows.
2. Open pachibo-rescue: it lists the backups.
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

No Linux build is released, but `bunx tauri build --bundles appimage` makes
one; make the backup with `idevicebackup2 backup --full <folder>`
(libimobiledevice) and choose that folder with フォルダを選ぶ.

By hand (`BACKUP_PASSWORD` for an encrypted backup):

```sh
cargo run -p ios-backup --example inspect -- <backup folder>
cargo run -p pachibo --example rescue -- <backup folder> pachibo.pcb
```

Pushing a `v*` tag has GitHub Actions build the macOS (universal) and Windows
apps into a draft Release, with the instructions in `.github/release-notes.md`.

## Licence

GPL-3.0-or-later.
