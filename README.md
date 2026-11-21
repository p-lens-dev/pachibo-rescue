# pachibo-rescue

ぱち簿（パチンコ・パチスロの収支アプリ）のデータをパソコンに取り出すデスクトップアプリです。

- iPhone 版: パソコンにある iPhone のバックアップから取り出し、ぱち簿自身の書き出し形式（`.pcb`）で保存します。P-Lens の「設定 → インポート → ぱち簿」で読み込めます。iPhone とは直接やりとりしません。
- Android 版: USB でつないだスマホの画面を読み取り、CSV で保存します。P-Lens の「設定 → インポート → 汎用形式」で読み込めます。
- データはどこにも送りません。
- Windows と macOS 向けに配布しています。ダウンロードと詳しい使い方は Releases にあります。

## 使い方

ぱち簿を iPhone から削除しないでください。アプリを削除すると、そのデータも iPhone から消え、バックアップにも入らなくなります。

1. iPhone をパソコンにつなぎ、バックアップを作ります。
   - Mac: Finder で iPhone を選び、「今すぐバックアップ」
   - Windows: Apple Devices（または iTunes）で「今すぐバックアップ」
2. pachibo-rescue を開くと、バックアップの一覧が出ます。
3. 「取り出す」を押して保存先を選びます。暗号化したバックアップは、そのパスワードを入れてから押します。

## Android 版のぱち簿から取り出す

Android 版のぱち簿（Google Play 版）は、アプリ自身の書き出しが有料版限定で、課金がアカウントに紐づくため買い直しが要ったり、端末によっては書き出しそのものが動かないことがあります。また Android 12 以降では、パソコンからアプリのデータを直接取り出す正規の方法がありません。そこで pachibo-rescue は、**スマホの画面を読み取って**収支を集め、P-Lens の「設定 → インポート → 汎用形式」で読み込める CSV にします。課金もアプリの書き出しも要りません。

アプリの「Android」タブを開き、画面の案内どおりに進めます。

1. スマホで「USB デバッグ」をオンにします（設定 → システム →「開発者向けオプション」。無ければ「端末情報」の「ビルド番号」を 7 回タップして出します）。
2. スマホをパソコンに USB でつなぎ、スマホに出る「USB デバッグを許可しますか？」で「許可」を押します。
3. スマホの画面をつけたまま（ロックを解除したまま）にします。
4. 「スマホを確認」→「取り出す」を押し、保存先を選びます。取り出しの間はスマホを触らずに待ちます（画面が自動で動きます）。
5. できた CSV を、P-Lens の「設定 → インポート → 汎用形式」で読み込み、列（日付・ホール・機種・投資・回収）を合わせます。

取り出しの間は、パソコンが自動でスリープしないようにしています。ただし、ノートパソコンのふたを閉じるなどして手でスリープさせると、取り出しは失敗します。

取り出せるのは、ぱち簿がスマホの画面に表示できる範囲です。1 件ずつ編集画面を開いて投資・回収・メモ・タグまで読むため、件数が多いと時間がかかります。端末のメーカーや機種によって画面の作りが違うと、うまく読めないことがあります。

Windows では、スマホのメーカーの USB ドライバ（ADB ドライバ）を入れないと、つないでもスマホが見つからないことがあります。そのときはメーカーのサポートページからドライバを入れてください（Pixel は Google USB ドライバ）。Mac では要りません。

## 初めて開くとき

このアプリは Apple や Microsoft の署名を受けていません。そのため、初回だけ次の手順が要ります。

- **macOS**: 開こうとすると止められます。システム設定 →「プライバシーとセキュリティ」の下にある「このまま開く」を押してください。「壊れているため開けません」と出たときは、ターミナルで `xattr -cr /Applications/pachibo-rescue.app` を実行してから開き直してください。
- **macOS（フルディスクアクセス）**: バックアップのフォルダを読むには、システム設定 →「プライバシーとセキュリティ」→「フルディスクアクセス」で pachibo-rescue をオンにしてください。オンにしたら、アプリを開き直します。
- **Windows**: 「Windows によって PC が保護されました」と出たら、「詳細情報」→「実行」を押してください。
- **Windows 11（スマート アプリ コントロール）**: 「スマート アプリ コントロールによって、安全でない可能性のあるアプリがブロックされました」と出たときは、署名のないアプリを例外として許可する方法がありません。「Windows セキュリティ」→「アプリとブラウザー コントロール」→「スマート アプリ コントロールの設定」で「オフ」にしてから、もう一度インストールしてください（Microsoft Defender などの保護は、オフにしても続きます）。Windows のバージョンによっては、一度オフにすると Windows を入れ直すまでオンに戻せないことがあります。

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
- `crates/pachibo-android`: Android 版のぱち簿を adb の UI オートメーション（`uiautomator` のダンプと `input`）で読み、汎用形式 CSV に書き出します。画面のダンプを解釈する部分は純粋で、端末を動かす部分は `Device` トレイトに預けてあります。
- `src-tauri`, `src`: デスクトップアプリ本体です（Tauri 2 と Solid）。「iPhone」「Android」のタブがあります。

Android の取り出しには `adb`（Android Platform Tools）が要ります。配布版はこれを同梱します（`scripts/fetch-adb.sh` が各 OS 向けに取ってきて、`bundle-adb.json` を重ねたビルドがリソースとして入れます。リリースの GitHub Actions がこの手順を踏みます）。開発中は PATH の `adb`、または環境変数 `ADB` に指定したものを使います（`scripts/fetch-adb.sh` を一度実行すれば同梱版と同じ経路も確かめられます）。コマンドラインからも試せます（スマホをつなぎ、画面をつけたまま）。

```sh
cargo run -p pachibo-android --example scrape -- out.csv            # 投資・回収・メモ・タグまで
cargo run -p pachibo-android --example scrape -- out.csv --shallow  # 日付・ホール・機種・収支のみ（速い）
```

Linux 版は配布していませんが、`bunx tauri build --bundles appimage` で作れます。バックアップは `idevicebackup2 backup --full <フォルダ>`（libimobiledevice）で作り、アプリの「フォルダを選ぶ」でそのフォルダを選びます。

手で取り出すこともできます（暗号化したバックアップは `BACKUP_PASSWORD` にパスワードを入れます）。

```sh
cargo run -p ios-backup --example inspect -- <バックアップのフォルダ>
cargo run -p pachibo --example rescue -- <バックアップのフォルダ> pachibo.pcb
```

`v*` のタグを push すると、GitHub Actions が macOS（Universal）版と Windows 版をビルドします。Release の下書きに、`.github/release-notes.md` の使い方と一緒に載ります。

## ライセンス

GPL-3.0-or-later
