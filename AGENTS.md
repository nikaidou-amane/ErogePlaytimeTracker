# AGENTS.md

## Project overview

- Windows 10/11 x64 専用の local-first desktop app。登録した visual novel の process/session 時間と background 時間を記録し、既定では background 時間を除いて playtime を算出する。
- Stack は Tauri 2 / Rust 2024 / Svelte 5 / TypeScript / SQLite (`rusqlite`)。server、cloud account、workspace/monorepo はない。
- 作業前にuser-facingな概要として [README.md](README.md)、開発・build情報として [docs/technical-notes.md](docs/technical-notes.md) を読む。追跡・データモデルの意図は [eroge-playtime-tracker-spec.md](eroge-playtime-tracker-spec.md) の sections 4–7、9–14、20 を参照する。仕様書内の directory tree は初期の「Suggested layout」であり、現行構成そのものではない。

## Repository map

- `src/main.ts` → `src/App.svelte`: frontend entry point と画面切替、tracking status の polling/event 購読。
- `src/components/`: UI。`GameDetail.svelte` は履歴、timestamp、screenshotも担当する。
- `src/lib/api.ts`: frontend から利用する Tauri command wrapper。`src/lib/types.ts` は Rust の serialized model と対応する。
- `src/app.css`: 全 component 共通の global styles と4 theme。component-local `<style>` は現在使っていない。
- `src-tauri/src/main.rs` → `src-tauri/src/lib.rs`: native entry point、plugin/state/tray/data directory 初期化、command 登録。
- `src-tauri/src/commands.rs`: task-oriented な Tauri boundary。filesystem、shell、autostart 等の native 操作もここを通す。
- `src-tauri/src/database/mod.rs`: schema migration、query、validation、集計を含む現在の persistence layer。別の migrations directory/repository layer はない。
- `src-tauri/src/tracking/`: pure state transition (`state.rs`)、Windows process/window adapter (`platform.rs`)、DB/event との orchestration (`mod.rs`)。
- `src-tauri/src/metadata/`: `GameMetadataProvider`、ErogameScape scraper、VNDB API adapter。selector/API response の詳細は provider の外へ漏らさない。
- `src-tauri/src/screenshot.rs`: global hotkey と foreground game client-area capture。
- `.github/workflows/ci.yml`: stacked PRを含むpull requestのcommit policy、frontend/Rust、license checks。`release.yml`は`v*` tagのaudit/build/publish専用。

## Development

Windows 上で Rust stable MSVC、Visual Studio C++ Build Tools、Node.js 20+、npm、WebView2 が必要。

- network依存のcommandがrestricted sandbox内で失敗した場合は、proxy・DNS・接続拒否などsandbox由来の可能性を確認し、必要なnetwork/escalated permissionで再実行してからcredential不良やremote service障害と診断する。network到達可能な確認でも拒否されるまでは、secretの交換や再認証をユーザーへ案内しない。secret値は表示しない。

```powershell
npm install
npm run tauri dev
```

- lockfile どおりの clean install は CI と同じ `npm ci` を使える。
- `npm run dev` は port 1420 の frontend-only preview。Tauri commands、native tracking、local asset protocol は利用できないため、native feature の動作確認には使わない。
- updater UI を実更新なしで確認する場合だけ、PowerShell で `$env:VITE_MOCK_UPDATE='true'; npm run tauri dev` を使う。この分岐は `import.meta.env.DEV` 時のみ有効。

## Requirement analysis and change design

- 変更・build taskでは、編集前にユーザーが求めるoutcome、現状と根本原因、守るべきinvariant・compatibility、受入条件を整理する。依頼文をそのまま完全な実装仕様とはみなさず、README、technical notes、仕様、現行architecture、data flowから本来必要なbehaviorを確認する。単純な機械変更では作業規模に応じて簡潔に行う。
- 依頼に複数のbehaviorが含まれる場合は、実装・受入・review・release・revertを独立して行えるoutcomeへ分解し、相互の技術的依存を明示する。一方を省いても他方の受入条件が成立し、schema、API、shared prerequisiteなどの依存がなければ別成果と扱う。同じ依頼文、画面、component、file、theme、prefix、作業時期であることは、同一成果の根拠にしない。
- ユーザーが挙げた画面・command・fileを自動的に修正範囲と決めない。関連する入口から保存・runtime state・出力までをend-to-endで追い、同じfield、assumption、pattern、shared abstractionをrepository内で検索して、局所症状か共通原因かを判断する。
- 修正は、意図したoutcomeを完全に満たす最も狭いshared boundaryへ置く。共通原因が別entry pointや隣接機能にも存在する場合は横展開するが、分析で見つけた対応が別のproduct decision、権限、または独立した成果へ広がる場合は無断で実装せず、影響と選択肢をユーザーへ提示する。
- handoff前に、変更した経路だけでなく、同じ原因を共有する類似箇所、alternate entry point、serialized model/default、compatibility、docs、testへの反映漏れを変更内容に応じて確認する。testは変更行の存在ではなく、意図したinvariantと代表経路を検証する。
- ユーザーの指摘で局所修正の背後に一般的な失敗パターンが判明した場合は、指摘された例だけをpatchせず、要件と根本原因を再整理して適用範囲を見直す。残存リスクや意図的に対象外とした範囲はhandoffで明示する。

## README update policy

- READMEはアプリの概要、導入、基本的な使い方を案内する入口とし、仕様書や全機能の詳細な説明書として扱わない。
- ユーザーからREADME更新の明示的な指示がない限り、原則としてREADMEを変更しない。機能の実装、docsの整合確認、commit・PR作成・releaseの依頼だけでは、README更新の許可を得たとみなさない。
- README更新が必要だと判断しても、依頼された実装・要件の遂行と検証を先に完了する。実装前や作業途中にはREADME更新の確認を挟まず、作業結果を報告する最後の段階で確認する。
- 確認時は更新が必要な理由と最小限の変更案を提示し、ユーザーの許可を得てからREADMEを編集する。返答がないことを許可とみなさない。同じ作業・範囲について既に明示的な指示や許可がある場合は、再確認しない。

## Git and pull request workflow

- Integration branch は `main`。`main` 上でcommitせず、`origin/main`へ直接pushしない。変更前に1 PR/1 concernのtopic branch（`update/...`、`fix/...`、`docs/...`、`ci/...`等）を作る。
- 独立した新しい変更タスクでは、編集前にcurrent branchとworking treeを確認する。既存topic branchの継続または明示的なstacked PRでない限り、cleanなworking treeで`origin`をfetchし、local `main`を`origin/main`までfast-forwardし、両者が同じcommitであることを確認してから新しいtopic branchを作る。checkoutされているという理由だけで以前のPR branchを新しい変更のbaseにしない。
- working treeに既存変更がある場合は、自動的なswitch、pull、stashや、現在のHEADからの無条件なbranch作成を行わない。変更の目的と所有者、現在のHEADが意図したbaseかを確認し、安全に分離できなければ変更を保持したままユーザーへ状況を報告する。dirtyなworking treeは暗黙にstackする理由にならない。
- commit/pushの依頼を受けた時点で`main`にいる場合も、上記のbase確認を満たしたうえでworking treeを保持したままtopic branchへ移ってからcommitする。unrelatedな既存変更を混ぜない。
- commitは1機能・1不具合・1保守目的。実装と対応testは同じcommitに含める。prefix、size目安、message形式、stacked PR、merge方法のsource of truthは [docs/development-workflow.md](docs/development-workflow.md)。
- PR作成を依頼されたら `$create-pr` skillを使用する。通常の実装依頼はcommit/push/PR作成までを暗黙に許可しない。
- `main`へのPRは1成果だけを扱い、原則5 commits以内・production code追加500行以内。PR数は依頼文の「PR」の単複ではなくoutcome分解から決める。超過や複数commitを分離できない場合は、PR本文に同一成果へ不可欠な具体的依存を残す。
- force-push、公開済みcommitのrewrite、PRのmergeは、ユーザーが明示的に依頼しない限り行わない。

## Verification

変更範囲に対応する test を先に実行し、handoff 前は原則として以下のRelease CIと同じ一式を実行する。

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

- `npm run lint` は存在しない。frontend の static check は `svelte-check` を呼ぶ `npm run check`。
- PR作成前は `npm run commit-policy:check -- --base origin/main --head HEAD` も実行する。message違反はerror、commit数とproduction additionsの目安超過はwarningとして報告される。
- formatting が必要なら frontend/docs は `npm run format`、Rust は `cargo fmt --manifest-path src-tauri/Cargo.toml`。repository 全体を整形する前後で diff を確認する。
- tracking、tray、hotkey、screenshot、installer、asset protocol は Windows/Tauri runtime でのみ検証できる。該当変更では `npm run tauri dev` で manual smoke test も行う。
- release workflow 自体は tag push で publish まで進む。検証目的で tag を作成・push しない。

## Frontend and command boundary

- Svelte 5 を使用するが、現行 components は `export let`、`$:`、callback props、`onclick` の既存 style。局所変更で別の component style/runes/event pattern を混在させない。
- UI は presentation/input conversion に留める。tracking、SQLite、metadata fetch、Windows integration を TypeScript 側へ移さない。datetime-local は `src/lib/time.ts` の `inputTime` / `utc` を通し、DB/API は RFC 3339 を維持する。
- command を追加・変更したら、少なくとも次を同時に照合する。
  1. `src-tauri/src/commands.rs` の command と input/output model
  2. `src-tauri/src/lib.rs` の `tauri::generate_handler!`
  3. `src/lib/api.ts` の typed wrapper（JS argument は既存どおり camelCase）
  4. `src/lib/types.ts` と呼び出し元 component
- native/plugin capability を増やす場合は `src-tauri/capabilities/default.json` と CSP/asset scope も確認する。現在 local image 表示は `$LOCALDATA/**` のみ許可される。
- `tracking-status` は event と polling の両方、`screenshot-captured` / `screenshot-error` は event で UI へ届く。片方だけ更新して payload shape をずらさない。

## Tracking and domain invariants

- 関連 process の visible top-level window を初めて検出した timestamp で1つの `PlaySession` を開始する。window 出現前は session なしの Starting として監視し、update・backup guard の対象に含める。window を表示せず終了した process は履歴に残さない。同じ game に登録された launcher/game executable や関連 child process は game 単位でまとめ、PID 単位の session を作らない。
- child process は、登録 executable の descendant かつその game directory 配下にある場合だけ関連付ける。DRM/global helper が session を開き続けないための制約である。
- playtime の正本は session と background interval の timestamp。`AppSettings.exclude_background_time`（既定 true）が true なら `(session end - launch) - background intervals`、false なら session 全体を query 時に算出する。設定にかかわらず background を記録し、過去履歴・並び順・timestamp累計・統計へ同じ設定を適用する。duration/aggregate を保存 field に変えない。
- Background は「関連 process と visible top-level window があるが、その game が foreground ではない」期間だけ。window 出現前や消失後を Background にしない。process 終了時は最後の window-loss timestamp で session を閉じる。
- foreground/window events と process-exit notification が主経路、3秒間隔の reconciliation が取りこぼし回復用。どちらか一方を前提にしない。
- 複数 game の同時起動を許容する。foreground game 以外の visible/running games はそれぞれ独立して Background を持つ。
- 起動時は persisted `last_seen` で orphan intervals/sessions を閉じ、session を `needs_review` にする。crash から次回起動までを playtime に加算しない安全策を維持する。
- update install/relaunch は tracking 中の game がないことを再確認してから行う。自動通知と settings 画面の両経路に同じ guard がある。

## Database and compatibility

- SQLite file は `%LOCALAPPDATA%\ErogePlaytimeTracker\app.db`。foreign keys と WAL を有効化し、UTC RFC 3339 strings を保存する。thumbnail、screenshotも同じ root 配下。
- schema 変更は `database/mod.rs` に次の numbered migration constant と `schema_migrations` 適用 block を追加する。既に配布済みの `MIGRATION_1`–`MIGRATION_5` を書き換えたり、起動時の ad-hoc schema mutation に置き換えない。
- `background_intervals` が現行計算の正本。`focus_intervals` は旧 version への rollback compatibility mirror なので削除しない。closed session の background/session 編集では `rebuild_focus_mirror` と migration validation を保つ。
- 旧 focus data は補集合を background として移行し、SQLite と同じ秒丸めで playtime が一致した場合だけ `background_migrated=1` にする。この検証を緩めない。
- interval は parent session 内かつ非重複でなければならない。session bounds の変更で既存 interval が範囲外になる場合は reject する。
- executable path は trim、quote除去、`/`→`\`、`\\?\` 除去、小文字化して照合し、DB 全体で case-insensitive unique。path semantics を変更する場合は launcher移行・child association の test も更新する。
- settings は `settings` table の key `app` に JSON 保存し、Rust model は `#[serde(default)]` で旧 JSON を読む。setting 追加時は `AppSettings::default`、TS `Settings` と UI 初期値/保存処理を揃える。

## Testing conventions

- testは現行のbehavior・invariant、または明示的に維持するmigration/backward compatibility処理を対象にする。fieldやfeatureを削除した際に、その不在だけをassertする恒久testを追加・維持しない。削除の完全性は変更時のrepository search、diff、build/checkで確認し、対応test、fixture、旧identifierも同時に整理する。明示的なcompatibility codeを残す場合だけ、そのcontractを検証するtestを維持する。
- frontend unit tests は対象 helper と同じ `src/lib/*.test.ts` に置き、Vitest を使う。現在 UI/E2E test harness はない。
- Rust unit tests は各 module 内の `#[cfg(test)]` に置く。DB tests は `Database::memory()`、metadata parser は network を使わない HTML fixture を使う。
- DB/schema/集計変更では migration、interval validation、両計算モードと切替後の再計算、legacy focus mirror をテストする。tracking変更では launcher重複、複数game、windowなし/foreground/background transition を `tracking/state.rs` の pure tests で覆う。

## Generated files, dependencies, and release

- `dist/`, `src-tauri/target/`, `src-tauri/gen/`, `node_modules/` は生成物。編集・commitしない。
- `THIRD_PARTY_LICENSES.txt` は ignored generated file。手編集せず `npm run licenses` で再生成する。Tauri build は packaging 前に生成し、`build.rs` は欠落時に placeholder を作るだけ。
- dependency 追加時は tracked `package-lock.json` / `src-tauri/Cargo.lock` を更新し、`npm run licenses` を通す。npm/Rust notice generator の allowlist (`scripts/generate-licenses.mjs`) と Rust audit policy (`deny.toml`) は別なので両方を確認し、license review なしに allowlist を広げない。
- installer は `npm run tauri build` で NSIS のみ。output は `src-tauri/target/release/bundle/nsis/`。WebView2 は download bootstrapper、updater artifacts も生成する。
- release version は `package.json`、`package-lock.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json` の5箇所を一致させる。`v<version>` tag と一致しないと workflow が失敗する。
- `TAURI_SIGNING_PRIVATE_KEY` は GitHub Actions の updater signing secret。値や local signing key を repository、logs、documentation に書かない。
- core tracking は local-only。network access は明示的な ErogameScape/VNDB metadata/thumbnail 操作と updater に限定し、play history や screenshots を upload する処理を追加しない。
