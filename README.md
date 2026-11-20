# pachibo-rescue

A desktop app for Windows, macOS and Linux that takes the records of ぱち簿, an
iPhone app for pachinko and pachislot results no longer updated, out of an
iPhone backup, and writes them in a form P-Lens takes in.

It reads a backup made by Finder (macOS), Apple Devices or iTunes (Windows), or
`idevicebackup2` (Linux). It never talks to the phone and sends nothing anywhere.

## Status

- `crates/ios-backup`: finding backups, reading what they say of themselves,
  unlocking encrypted ones with their password, and taking one app's files out.
- `crates/pachibo`: ぱち簿's `Documents/data.plist` written as its own `.pcb`
  export, which P-Lens takes in (設定 → インポート → ぱち簿).
- The desktop shell (Tauri) is still to come.

## Rescuing by hand

```sh
cargo run -p pachibo --example rescue -- <backup folder> pachibo.pcb
```

## Looking into a backup by hand

```sh
cargo run -p ios-backup --example inspect -- <backup folder>
cargo run -p ios-backup --example inspect -- <backup folder> AppDomain-<bundle id>
cargo run -p ios-backup --example inspect -- <backup folder> AppDomain-<bundle id> <out folder>
```

Set `BACKUP_PASSWORD` for an encrypted backup.

## Licence

GPL-3.0-or-later.
