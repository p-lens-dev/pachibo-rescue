# pachibo-rescue

ぱち簿（iPhone 版、2020 年から更新が止まっているパチンコ・パチスロの収支アプリ）のデータを、iPhone のバックアップから取り出すデスクトップアプリです。取り出したデータは、ぱち簿自身の書き出し形式（`.pcb`）で保存します。P-Lens の「設定 → インポート → ぱち簿」で読み込めます。

- 読むのはパソコンにある iPhone のバックアップだけです。iPhone とは直接やりとりしません。
- データはどこにも送りません。
- Windows と macOS 向けに配布しています。ダウンロードと詳しい使い方は Releases にあります。

## 使い方

ぱち簿を iPhone から削除しないでください。アプリを削除すると、そのデータも iPhone から消え、バックアップにも入らなくなります。

1. iPhone をパソコンにつなぎ、バックアップを作ります。
   - Mac: Finder で iPhone を選び、「今すぐバックアップ」
   - Windows: Apple Devices（または iTunes）で「今すぐバックアップ」
2. pachibo-rescue を開くと、バックアップの一覧が出ます。
3. 「取り出す」を押して保存先を選びます。暗号化したバックアップは、そのパスワードを入れてから押します。

## 初めて開くとき

このアプリは Apple や Microsoft の署名を受けていません。そのため、初回だけ次の手順が要ります。

- **macOS**: 開こうとすると止められます。システム設定 →「プライバシーとセキュリティ」の下にある「このまま開く」を押してください。「壊れているため開けません」と出たときは、ターミナルで `xattr -cr /Applications/pachibo-rescue.app` を実行してから開き直してください。
- **macOS（フルディスクアクセス）**: バックアップのフォルダを読むには、システム設定 →「プライバシーとセキュリティ」→「フルディスクアクセス」で pachibo-rescue をオンにしてください。オンにしたら、アプリを開き直します。
- **Windows**: 「Windows によって PC が保護されました」と出たら、「詳細情報」→「実行」を押してください。

## 開発

```sh
bun install
bun run dev                       # Tauri の開発起動
bun run check                     # tsc
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

- `crates/ios-backup`: バックアップを探し、内容の概要を読み、暗号化をパスワードで解き、アプリ一つ分のファイルを取り出します。
- `crates/pachibo`: ぱち簿の `Documents/data.plist` を `.pcb` に書き換えます。
- `src-tauri`, `src`: デスクトップアプリ本体です（Tauri 2 と Solid）。

Linux 版は配布していませんが、`bunx tauri build --bundles appimage` で作れます。バックアップは `idevicebackup2 backup --full <フォルダ>`（libimobiledevice）で作り、アプリの「フォルダを選ぶ」でそのフォルダを選びます。

手で取り出すこともできます（暗号化したバックアップは `BACKUP_PASSWORD` にパスワードを入れます）。

```sh
cargo run -p ios-backup --example inspect -- <バックアップのフォルダ>
cargo run -p pachibo --example rescue -- <バックアップのフォルダ> pachibo.pcb
```

`v*` のタグを push すると、GitHub Actions が macOS（Universal）版と Windows 版をビルドします。Release の下書きに、`.github/release-notes.md` の使い方と一緒に載ります。

## ライセンス

GPL-3.0-or-later
