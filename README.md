# p-lens-rescue

A desktop app for Windows, macOS and Linux that takes the records of an iPhone
pachinko and pachislot app out of an iPhone backup, for apps that can no longer
export them themselves, and writes them in a form P-Lens takes in.

It reads a backup made by Finder (macOS), Apple Devices or iTunes (Windows), or
`idevicebackup2` (Linux). It never talks to the phone and sends nothing anywhere.

## Status

- `crates/ios-backup`: finding backups, reading what they say of themselves,
  unlocking encrypted ones with their password, and taking one app's files out.
- The desktop shell (Tauri) and the conversion to P-Lens's import are still to come.

## Looking into a backup by hand

```sh
cargo run -p ios-backup --example inspect -- <backup folder>
cargo run -p ios-backup --example inspect -- <backup folder> AppDomain-<bundle id>
cargo run -p ios-backup --example inspect -- <backup folder> AppDomain-<bundle id> <out folder>
```

Set `BACKUP_PASSWORD` for an encrypted backup.

## Licence

GPL-3.0-or-later.
