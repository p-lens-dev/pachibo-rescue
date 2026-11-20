# pachibo-rescue

ぱち簿（iPhone 版、2020 年から更新が止まっているパチンコ・パチスロの収支アプリ）のデータを、iPhone のバックアップから取り出すデスクトップアプリです。取り出したデータは、ぱち簿自身の書き出し形式（`.pcb`）で保存します。P-Lens の「設定 → インポート → ぱち簿」で読み込めます。

- 読むのはパソコンにある iPhone のバックアップだけです。iPhone とは直接やりとりしません。
- データはどこにも送りません。
- Windows・macOS・Linux で動きます。

## 使い方

1. iPhone をパソコンにつなぎ、バックアップを作ります。
   - Mac: Finder で iPhone を選び、「今すぐバックアップ」
   - Windows: Apple Devices（または iTunes）で「今すぐバックアップ」
   - Linux: `idevicebackup2 backup --full <フォルダ>`（libimobiledevice）
2. pachibo-rescue を開くと、バックアップの一覧が出ます。Linux では「フォルダを選ぶ」でバックアップのフォルダを選びます。
3. 「取り出す」を押して保存先を選びます。暗号化したバックアップは、そのパスワードを入れてから押します。

## 初めて開くとき

このアプリは Apple や Microsoft の署名を受けていません。そのため、初回だけ次の手順が要ります。

- **macOS**: 開こうとすると止められます。システム設定 →「プライバシーとセキュリティ」の下にある「このまま開く」を押してください。「壊れているため開けません」と出たときは、ターミナルで `xattr -cr /Applications/pachibo-rescue.app` を実行してから開き直してください。
- **macOS（フルディスクアクセス）**: バックアップのフォルダを読むには、システム設定 →「プライバシーとセキュリティ」→「フルディスクアクセス」で pachibo-rescue をオンにしてください。オンにしたら、アプリを開き直します。
- **Windows**: 「Windows によって PC が保護されました」と出たら、「詳細情報」→「実行」を押してください。
- **Linux**: AppImage に実行権限を付けて開きます（`chmod +x pachibo-rescue_*.AppImage`）。

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

手で取り出すこともできます（暗号化したバックアップは `BACKUP_PASSWORD` にパスワードを入れます）。

```sh
cargo run -p ios-backup --example inspect -- <バックアップのフォルダ>
cargo run -p pachibo --example rescue -- <バックアップのフォルダ> pachibo.pcb
```

`v*` のタグを push すると、GitHub Actions が macOS（Universal）・Windows・Linux 版をビルドして、Release の下書きに載せます。

## ライセンス

GPL-3.0-or-later
