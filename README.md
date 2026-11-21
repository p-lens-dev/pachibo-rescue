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

## From ぱち簿 on Android

The Android build of ぱち簿 keeps its own export behind the paid version — and
the purchase is tied to a Google account, so changing accounts asks you to buy
it again; on some phones the export is broken outright. On Android 12 and later
there is also no sanctioned way to take an app's data off the phone from a
computer. So pachibo-rescue **reads the phone's screen** instead, over adb, and
writes the records as a CSV that P-Lens takes in at 設定 → インポート → 汎用形式.
No purchase, no in-app export.

Open the Android tab in the app and follow it: turn on USB debugging on the
phone, connect it over USB and allow this computer, keep the screen on and
unlocked, then press スマホを確認 and 取り出す. It reads what ぱち簿 can show on
screen; opening each record for 投資/回収/メモ/タグ takes time over a long
history, and a very different phone UI may not read cleanly. Needs `adb`
(bundled, or on the PATH, or at `$ADB`).

The app keeps the computer from sleeping on its own during a read; put to
sleep by hand (a laptop's lid closed, say), the read fails.

On Windows the phone maker's USB (ADB) driver may be needed before the phone
shows up at all; get it from the maker's support pages (for a Pixel, Google's
USB driver). A Mac needs none.

## Opening it the first time

The app is not signed by Apple or Microsoft.

- **macOS**: when it is stopped, press "Open Anyway" under System Settings →
  Privacy & Security. If it says the app is damaged, run
  `xattr -cr /Applications/pachibo-rescue.app` and open it again.
- **macOS, Full Disk Access**: to read the backup folder, turn pachibo-rescue
  on under System Settings → Privacy & Security → Full Disk Access, then
  reopen the app.
- **Windows**: at "Windows protected your PC", press "More info" → "Run anyway".
- **Windows 11, Smart App Control**: if it says Smart App Control blocked an
  app that may be unsafe, there is no way to allow an unsigned app as an
  exception. Turn it off under Windows Security → App & browser control →
  Smart App Control settings, then install again (Microsoft Defender's other
  protection carries on). On some versions of Windows it cannot be turned
  back on without reinstalling Windows.

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
- `crates/pachibo-android`: the Android build read over adb (a `uiautomator`
  dump and `input`), written as a 汎用形式 CSV. Reading the dump is pure; the
  phone is driven through a `Device` trait. By hand:
  `cargo run -p pachibo-android --example scrape -- out.csv` (add `--shallow`
  for day/parlor/machine/result only). A released build carries `adb` itself
  (`scripts/fetch-adb.sh` stages it and `bundle-adb.json` bundles it as a
  resource; the release workflow does this per OS); a dev build uses `adb` on
  the PATH or at `$ADB`.
- `src-tauri`, `src`: the desktop app (Tauri 2 and Solid), with iPhone and
  Android tabs.

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
