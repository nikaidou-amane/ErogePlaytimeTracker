# Technical notes

この文書は、開発・保守・リリースに必要な技術情報をまとめたものです。実装時の規約とdomain invariantは [AGENTS.md](../AGENTS.md)、branch・commit・pull requestの運用は [development-workflow.md](development-workflow.md) を参照してください。

## 技術スタック

Rust 2024、Tauri 2、Svelte 5、TypeScript、SQLite (`rusqlite`)、Windows API (`windows` crate)、reqwest、scraperを使用しています。serverやcloud accountはありません。

## 開発環境

- Windows 10 / 11 x64
- Rust stable（MSVC toolchain）とVisual Studio C++ Build Tools
- Node.js 20以降、npm
- Microsoft Edge WebView2（通常のWindows 10/11には導入済み）

```powershell
npm install
npm run tauri dev
```

lockfileどおりのclean installには`npm ci`を使用します。`npm run dev`はport 1420のfrontend-only previewであり、Tauri command、native tracking、local asset protocolは利用できません。

## テストと品質確認

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
npm run format:check
npm run check
npm run commit-policy:test
npm run test
npm run build
```

TypeScript、Svelte、CSS、JSON、Markdownなどの整形には`npm run format`、Rustの整形には`cargo fmt --manifest-path src-tauri/Cargo.toml`を使用します。Rust testsにはgame単位のsession遷移、launcherを含む複数exe、複数game間のforeground遷移、集計・重複validation、fixtureベースのErogameScape parser検証が含まれます。

## Windows build / installer

```powershell
npm run tauri build
```

NSIS形式のinstallerが`src-tauri/target/release/bundle/nsis/`に生成されます。WebView2 download bootstrapperを使用するため、利用者がRust、Node.js、.NET、開発SDKを手動導入する必要はありません。初回install時にWebView2を取得するため、network接続が必要になる場合があります。

## 保存場所

dataは`%LOCALAPPDATA%\ErogePlaytimeTracker\`配下に保存します。

- `app.db`: SQLite database（WAL、foreign key有効、UTC RFC 3339 timestamp）
- `thumbnails\`: download済みpackage image cache
- `screenshots\`: 撮影したscreenshot
- `backups\`: import直前に自動作成される復旧用`.eptbackup`
- `update-notice-state.json`: このPCでの更新完了通知の確認状態（backup対象外）
- log: Tauri log pluginの標準app log directory

durationはDBへ重複保存せず、sessionとintervalのtimestampからquery時に算出します。
Screenshotの文字起こしは同梱したPP-OCRv5 mobileの検出・認識modelを`paddleocr_rs_onnx`とONNX Runtimeでon-demand実行します。modelは初回実行時にprocess内で初期化し、その後は再利用します。UIで指定した範囲はnormalized coordinatesとしてcommandへ渡し、Rust側で元画像からcropします。文字起こし処理では画像や認識結果をnetworkへ送信せず、認識結果はDBへ保存しません。「Googleで検索」の明示操作時には認識結果の全文をURLのquery parameterへencodeし、既存の`open_external_url` commandを通じて既定のブラウザでGoogle検索を開きます。このとき認識結果は検索語としてGoogleへ送信されます。

ゲーム画面からのOCR検索は`hotkeys.rs`で撮影キーとは別に登録します。`game_ocr.rs`の専用threadでforegroundの計測中gameとclient-area座標を確認し、`WS_EX_NOACTIVATE`・layered・click-throughのWin32 overlayに選択枠と操作案内だけを描画します。静止画への置換、window activation、mouse capture、display mode変更、終了時のfocus復元は行いません。ゲームを前面に保つため、選択中も通常どおりforeground時間として計測し、検索後のbrowserへの移動はbackground時間として扱います。

選択threadに一時的なlow-level mouse/keyboard hookを置き、範囲選択の左クリックと中止用の右クリック・Escだけを受け取ります。Esc以外のkeyは記録せず、そのまま通します。callback内ではstate更新とmessage通知だけを行い、描画・撮影・OCRは実行しません。中止すると枠を即座に非表示にし、受け取ったpressに対応するreleaseまで処理してからhookを解除します。desktop切替などでreleaseを取得できない場合の待機上限は10秒です。focus loss、client-areaの位置・サイズ変更、display/DPI変更では選択を破棄します。物理画面座標を元画像のnormalized coordinatesへ変換し、範囲外へのdragをclampします。

有効な選択でマウスを離すとoverlayとhookを除去し、DWMの描画完了を待ってから同じgame/session・client areaの画面を取得します。frameはmemory上だけで保持し、同じOCR engineへ渡します。画像fileやDB recordは作りません。空でない認識結果だけを既定browserのGoogle検索へ送ります。ゲームは一時停止せず、処理中の再起動は抑止します。エラーは`game-ocr-error` eventで本アプリ内に表示し、modal dialogによるfocus移動を避けます。追加のWebView、asset scope、Tauri capabilityは不要です。排他的fullscreenでは非activationでもoverlayの合成やcaptureにゲーム固有の制約が残ります。

撮影・OCR検索のキーは同じregistryで管理し、修飾キーの順序や別名も含めて重複を拒否します。設定での入力中は両方を一時解除し、入力の完了・中止・設定画面からの離脱時に保存済みのbindingへ戻します。設定の保存では両方を更新し、登録失敗時は元のbindingを復元します。旧設定の`ocr_search_hotkey`は空文字（無効）として読み込み、backupの設定にも含めます。

## Backup / restore

設定画面から作成する`.eptbackup`は、SQLiteの一貫したsnapshot、参照中のthumbnailと任意のscreenshot、format/schema version、各fileのSHA-256を含むZIP archiveです。screenshotを除外した場合はsnapshot側の`game_screenshots`も空にし、その選択をmanifestへ記録します。archive作成はfileを読みながらchecksum計算と圧縮を1 passで行い、処理済みbyte数をIPC channelでUIへ通知します。snapshot内のmedia pathはarchive相対pathへ変換し、import先を検証した後で新しい`%LOCALAPPDATA%`配下の絶対pathへ書き換えます。game executable pathは保持しますが、game本体のfileは含めません。

Importはmergeではなく全置換です。archive path、重複entry、symlink、size、checksum、SQLite integrity/foreign key、schema versionをactive dataへ触れる前にstaging領域で検証します。確定時に現在のdataを`backups\`へ自動exportし、pending markerを書いて再起動します。次回起動時のdata directory切替に失敗した場合や切替中に中断された場合はrollback directoryから元のdataへ戻します。対応済みの古いschemaはstaging内でmigrationし、新しいschema versionのbackupは対応versionへappを更新するまで拒否します。移行元の設定は復元しますが、`last_seen`とskip中のupdate versionは移行先向けにresetし、autostartは移行先のWindowsへ再適用します。

## Tracking方式

登録exeをWindows APIで列挙し、1本のgameに属するprocessを検出すると起動中として監視します。この時点ではsessionを作らず、関連processのvisible top-level windowを初めて検出したtimestampでsessionを開始します。foregroundになる前でも、visible windowがあれば記録を開始します。windowを表示せずに終了した起動処理は履歴に残りません。session開始後は関連processが1以上から0になったときに終了を確定します。関連processが残っている間のlauncherからgame本体への移行や一時的なwindow消失ではsessionを分割しません。

foregroundは`SetWinEventHook(EVENT_SYSTEM_FOREGROUND)`を主な通知経路とし、`GetForegroundWindow`、`GetWindowThreadProcessId`、process image pathで登録exeと照合します。windowのcreate、destroy、show、hide通知とvisible top-level windowの列挙も使用します。Backgroundは、関連processとvisible windowが存在する一方で、そのgameがforegroundではない状態として記録します。関連PIDはprocess handleでも監視し、終了通知の直後に再照合します。通知取りこぼしから復旧するため、3秒間隔のreconciliationも実行します。playtimeは既定でsessionの開始から終了までの時間からbackground intervalの合計を引いて算出します。

旧versionへのrollback compatibilityのため、従来の`focus_intervals` tableは削除せず、compatibility mirrorとして新しいrecordにも併記します。旧dataは初回起動時にfocus intervalの補集合をbackground intervalとしてmigrationし、background除外時の移行前後のplaytime秒数が一致した場合だけ確定します。この検証とcompatibility mirrorは計算設定に依存しません。

起動直後など、関連processはあるがvisible windowがまだない期間はBackgroundとして扱いません。gameのvisible windowが消えた後も一時的なBackground recordを作らず、最後にwindowが消えたtimestampでsessionを閉じます。異常終了後はperiodic `last_seen`より後を加算せず、orphan recordを閉じて`needs_review`にします。要確認のsessionは詳細画面で復旧理由を表示し、記録を修正するか、内容に問題がなければ確認済みにできます。

headerのtracking statusはgame単位のphase（起動中、プレイ中、Background、画面切替・終了処理中）から複数chipを同時表示します。同じphaseが複数本ある場合は件数へ集約し、各titleはchipのtooltipで確認できます。session作成前の起動中gameもstatusに含め、`session_id`は`null`にします。この状態でもupdate・backup操作のguardを維持し、撮影・OCRは有効なsessionとforeground windowがある場合だけ実行します。開始条件の変更は新しい記録だけに適用し、既存履歴やDB schemaは変更しません。

### プレイ時間の計算設定

`AppSettings.exclude_background_time`は既定`true`です。既存の`settings` tableの`app` JSONへ保存し、旧JSONにfieldがなければ`serde(default)`で従来どおりbackgroundを除外します。schema migrationやdurationの再保存は不要です。

`Database`の共通settings readerを使い、game一覧・詳細・並び順、session、timestampの累計と差分、全体統計・game別統計がqueryごとに保存済みの設定を参照します。`false`ではsession全体を算入し、日別統計はsessionを各日の範囲へ分割します。過去履歴と記録中sessionの両方が対象です。設定保存後に各画面を開くと再起動なしで反映されます。

tracking state、background記録、session境界、最終プレイの判定は変更しません。backgroundの合計値や区間編集も計算設定と独立して維持します。UIでは「除外時間／区間」ではなく「バックグラウンド時間／区間」と呼び、現在の計算方法を明示します。backupには設定と区間の両方を含め、復元後も計算設定を切り替えられます。

## ネットワークとWindowsプロキシ

更新確認・更新ファイルのdownload、ErogameScape / VNDBの情報取得、thumbnail取得は、Windowsの手動プロキシ設定を自動的に参照します。アプリ内の切り替え設定は不要です。Windows側の有効・無効、接続先、除外先の変更は、次の取得操作から反映します。進行中の通信は開始時の経路を使い、プロキシへの接続に失敗しても直接接続へ再試行しません。

reqwestの`system-proxy` featureを有効にし、updaterが使う別versionのreqwestにも共通のhyper-util経由で反映します。metadata providerとupdaterは操作ごと、thumbnailはdownloadごとにclientを作成し、起動時の設定を保持し続けないようにします。明示的なHTTP_PROXY / HTTPS_PROXY / ALL_PROXY / NO_PROXY環境変数は従来どおり優先されます。PAC scriptやWPADによる自動構成、Windows統合認証はこの対応の対象外です。

## ErogameScape / VNDB連携

1つの入力欄へgame URLまたはIDを指定し、title、brand、発売日、package imageを取得します。数字のみはErogameScape、`v548`形式またはVNDBの作品URLはVNDBとしてRust側で判定します。`GameMetadataProvider`の下でErogameScapeはHTMLを解析し、VNDBは認証不要の[公式Kana API](https://api.vndb.org/kana)を利用します。

VNDBのtitleは日本語、原語のmain title、表示titleの順で選び、brandはAPIが返す最初のdeveloperのoriginal name（なければname）を採用します。発売日は年月日が揃う場合だけ既存のdate欄へ反映し、年のみ・年月のみ・未定の場合は未設定と理由を表示します。画像はAPIの`image.url`から取得するため、Webページの表示確認ボタンやCookieには依存しません。

thumbnailはlocalにcacheし、画像取得失敗は警告として表示します。画像なしでも情報を登録・更新でき、更新時の既存thumbnailは取得失敗や画像なしの場合に保持します。networkやparser failureはtracking loopへ影響せず、siteへのaccessは明示的な取得・更新操作時だけです。

schema migration 6で`games.vndb_id`（`v548`形式、unique）と`games.metadata_source`を追加します。両siteのIDを保持可能で、再取得は選択中の取得元から行います。詳細画面の情報取得元URLを編集すると取得元と対応IDを更新し、URLを消すと再取得を解除します。旧DBのID・URLから取得元を移行し、backupにも保存します。

本appはErogameScape、VNDBおよび各game makerの公式appではありません。取得したpackage imageは利用者のPC内にのみcacheし、repositoryやinstallerには同梱しません。各画像とgame情報に関する権利は、それぞれの権利者に帰属します。site運営者または権利者から要請があった場合は、連携方法を見直します。

## Release

### 更新完了通知

インストール済みappの起動が成功した後、`update_notice.rs`が実行中versionとこのPCの確認済みversionを比較します。ダウンロードやinstaller起動を完了扱いにしないため、自動更新通知・設定画面・手動installerのいずれから更新しても同じ判定です。playtime tracking、update前の起動中game guard、`AppSettings`やDB schemaは変更しません。

初回インストールでは現在versionを基準として保存するだけで通知しません。この機能を持たない旧appからの更新では、既存`app.db`の有無から導入済みと判断し、まず現在versionの通知を一度表示します。以後は未確認の範囲を新しいversionから順にまとめ、間にある内部変更のみのversionには空の見出しを出しません。現在versionの更新完了は変更点の有無にかかわらず通知します。

`get_update_completion_notice`は未確認通知を消費しません。閉じるボタンまたはEscで`acknowledge_update_completion`を呼び、成功時にだけ確認済みversionを進めます。閉じずに終了した場合は次回起動時も表示します。同一versionでの再起動・再インストールやdowngradeでは確認済みversionを下げません。

確認状態はdata rootの独立JSONを一時file経由で置換し、backup export/importの対象にしません。保存・読み込みの失敗はtrackingやapp操作を止めません。状態が破損している場合はlocal logへ記録し、その起動では通知を抑止して破損fileを保持します。確認状態の保存失敗時はアプリ内で再表示の可能性を知らせます。通知はappが表示・focusされた時に開き、native windowのshow/activateは行いません。設定読込を待ち、通知を閉じてから通常画面と新versionの案内を開始するため、未保存設定の確認などと重なりません。

debug buildは実際の確認状態を読み書きしません。実更新なしのUI確認には、PowerShellで`$env:VITE_MOCK_UPDATE_NOTICE='true'; npm run tauri dev`を使います。`'empty'`では完了文言だけの表示を確認できます。実際のupdaterを試さず表示を確認する既存の`VITE_MOCK_UPDATE`とは別のDEV専用設定です。

### 利用者向け更新内容の準備

`.agents/skills/release/SKILL.md`に従い、release準備時に前回の公開済みstable releaseから対象commitまでを確認し、利用者に伝える機能追加・改善・修正だけを日本語の箇条書きへ整理します。PR titleの転記やprefixによる機械的な抽出は行いません。release準備のreview/PRで文言と調査範囲を確認し、公開前に追加mergeによる漏れを再確認します。GitHub Releaseの自動生成PR一覧は従来どおり残します。

`release-notes/ja.json`を次の形式でversion更新と同時に編集します。`releases`は新しいversion順で、公開済みentryは保持します。下記は形式の例であり、実際のversionや文言は対象releaseに合わせます。

```json
{
  "schema_version": 1,
  "releases": [
    { "version": "1.2.3", "changes": ["設定から新しい機能を利用できるようになりました。"] },
    { "version": "1.2.2", "changes": [] }
  ]
}
```

各`changes`は空白だけでない1行のplain textです。内部変更だけなら明示的に`[]`を入れ、架空の改善文言や「変更点はありません」を追加しません。RustがJSONをbinaryへ同梱し、表示のためのnetwork accessやMarkdown/HTML解釈は行いません。

`npm run release-notes:check`（`npm run build`にも含む）でschema、version、重複、並び順、文言の形式を検証します。release準備とtag workflowでは`npm run release-notes:check -- --version <version>`により**対象versionのentryが存在すること**も必須にします。entry欠落と意図的な`changes: []`を区別し、未準備のまま公開されるのを防ぎます。この機能の導入commitでは既に公開済みのversionの内容を遡って作らずcatalogを空にしておき、次のrelease準備時から記入します。

### 公開

versionを`package.json`、`package-lock.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`で一致させ、同じversionのannotated tagをpushするとGitHub ActionsがWindows x64 installerを生成・公開します。

```powershell
git tag -a v0.1.0 -m "Eroge Playtime Tracker v0.1.0"
git push origin v0.1.0
```

workflowはfrontendとRustのcheck・test、dependency license audit、third-party license一覧生成を通過した場合だけGitHub Releaseを公開します。生成されたNSIS installerには`LICENSE`と`THIRD_PARTY_LICENSES.txt`が同梱されます。現在はcode signingを行っていないため、Windows SmartScreenの警告が表示される場合があります。

## License

app本体は [MIT License](../LICENSE) で提供します。Rustとnpmのdependency、同梱model、native runtimeにはそれぞれのlicenseが適用されます。`third-party/assets.json`ではCargo/npm外の配布物についてsource revision、SHA-256、legal textを管理し、`npm run licenses`で整合性を検証して一覧を生成します。`THIRD_PARTY_LICENSES.txt`はgenerated fileのためGit管理せず、Tauri buildとGitHub Actionsがinstaller作成前に生成します。

## 既知の制約

- Windows専用です。管理者processなど、OSがimage path取得を拒否するprocessは検出できない場合があります。
- 実行ファイルはfile pickerで個別に登録します。disk全体の自動探索は行いません。
- ErogameScapeのHTML構造変更時はprovider selectorの更新が必要です。
- crash recoveryは安全側に倒して`last_seen`で閉じ、該当sessionを要確認にします。失われた数秒は履歴編集画面で修正できます。
