# Development workflow

この文書はbranch、commit、pull request運用のsource of truthです。repository固有のbuild/test方法は [AGENTS.md](../AGENTS.md) と [technical notes](technical-notes.md) を参照してください。

## Protected integration branch

- Integration branchは`main`です。`master`ではありません。
- `main`上でcommitしません。`origin/main`へ直接pushしません。
- 通常の変更はtopic branchからpull requestとしてmergeします。
- PR作成の依頼はbranch作成、commit、検証、topic branchのpush、PR作成を許可しますが、mergeまでは許可しません。
- 単なる実装依頼はcommit、push、PR作成を暗黙に許可しません。

独立した新しい変更タスクでは、ファイルを編集する前にcurrent branchとworking treeを確認します。既存topic branchの継続または明示的なstacked PRでない限り、cleanなworking treeで`origin`をfetchし、local `main`を`origin/main`までfast-forwardしてから、1 PRにつき1本のbranchを作ります。同期後は`main`と`origin/main`が同じcommitであることを確認し、以前のPR branchをcheckoutしているという理由だけで新しい変更のbaseにしません。

```powershell
git status --short --branch
git fetch origin
git switch main
git merge --ff-only origin/main
git rev-list --left-right --count main...origin/main
git switch -c update/short-description
```

`git rev-list`の結果は`0 0`でなければなりません。local `main`だけのcommitがある場合や、`main`と`origin/main`がdivergeしてfast-forwardできない場合は、その状態を解消せずに新しいbranchを作りません。履歴を書き換えたり変更を捨てたりせず、状況をユーザーへ報告します。

working treeに既存変更がある場合は、自動的にswitch、pull、stashせず、現在のHEADから無条件にbranchを作りません。まず変更が今回のタスクに属するか、現在のHEADが意図したbaseかを確認します。正しいbaseから安全に分離できない場合は変更を保持したまま停止してユーザーへ確認します。dirtyなworking treeは、別のPR branchをbaseにする理由にはなりません。

branch名は`<type>/<short-kebab-description>`とします。typeは変更のprimary commit prefixに合わせます。

```text
update/add-library-filter
fix/prevent-update-during-tracking
docs/development-workflow
ci/pull-request-checks
```

## Commit scope

- 1 commitは1機能、1不具合、または1つの独立した保守目的だけを扱います。
- 同じprefixでも機能AとB、bug CとDは別commitにします。
- 実装と、その実装を直接検証するtestは同じcommitに含めます。
- test追加が現実的でないbehavior変更では、PRのVerificationに理由とmanual testを記録します。
- 必須のdocumentation、migration、lockfile、generated metadataは、それが属する変更と同じcommitに含めて構いません。これらは別の変更目的とは数えません。
- dependency変更と、それにより更新されたlockfileを分離しません。
- broad formattingは`[format]` commitへ分離し、behavior変更と混ぜません。触った行だけのincidental formattingはprimary変更に含めて構いません。
- 各commitは可能な限りbuild可能で、対応するcheckが通る状態にします。
- 最終PRに`WIP`、`fixup!`、一時debug commitを残しません。
- unrelatedなworking-tree変更や、ユーザーが別目的で作成した変更をstageしません。`git add .`ではなく明示的なpathまたはhunkをstageします。

### Size guidelines

行数はsoft guidelineであり、単一責務より優先しません。

- 1 commitあたりproduction code追加200行以内を目安にします。
- standalone test、docs、lockfile、generated file、純粋なformat変更は200行の集計から除外します。Rustのinline testはproduction fileと同じ行数へ含まれるため、表示値はあくまで目安です。
- 200行を超えたら、別の責務や別機能を含んでいないか再検討します。
- 分割するとbuild不能になる、migrationと利用codeが離れる、実装とtestが離れる場合は分割しません。
- 小さなdiffでも複数の成果を含むなら分割します。

`npm run commit-policy:check -- --base <base> --head <head>`はcommitごとのproduction additionsを表示し、200行超過をwarningにします。

## Commit messages

subjectは次の形式です。

```text
[prefix] Imperative summary
[prefix, test] Imperative summary
```

### Primary prefixes

- `[update]`: 機能追加、既存機能の改善、意図的なbehavior変更
- `[fix]`: 不具合修正
- `[refactor]`: 外部behaviorを変えない構造整理
- `[docs]`: documentationのみ
- `[build]`: dependency、build、packaging
- `[ci]`: CIまたは検証automation
- `[format]`: formattingのみの機械的変更
- `[release]`: version更新などrelease準備
- `[chore]`: 他のprefixに該当しない限定的なrepository保守
- `[test]`: product codeを変更しないtest追加・改善

primary prefixは1つだけです。対応testを同じcommitに含める場合に限り、`test`を唯一のsecondary prefixとして追加できます。許可する組み合わせは次だけです。

```text
[update, test]
[fix, test]
[refactor, test]
[build, test]
[ci, test]
```

`[update, fix]`、`[update, docs]`、3個以上のprefixなど、2つのprimary目的を表す組み合わせは禁止します。documentationを伴う機能実装のprimaryは`update`であり、`docs`をsecondaryにしません。

subjectはprefixを含め72文字以内を目安ではなく必須とし、末尾にperiodを付けません。repository履歴との一貫性のため、簡潔な英語の命令形を使用します。

詳細が必要な場合は空行を1行挟み、1〜3行のbulletでwhat/whyを記載します。subjectだけで十分なら本文を省略します。

```text
[fix, test] Prevent updates while games are running

- Block installation from both update entry points
- Cover the active-tracking guard
```

message形式は`npm run commit-policy:check`で検証します。公開済みcommitは、ユーザーの明示的な許可なしにamend、rebase、force-pushしません。

この規約より前の`main`履歴は遡及修正しません。CIとvalidatorはpull requestのbaseからheadまでに追加されたcommitだけを検査します。

## Pull request scope

- 1 PRは1つの成果または問題だけを扱います。
- 成果は、1つの受入条件としてreview、release、revertできる単位です。一方を含めなくても他方の受入条件が成立し、schema、API、shared prerequisiteなどの技術的依存がなければ、別成果として別PRにします。
- 同じ依頼文、画面、component、file、theme、prefix、作業時期であることや、conflictの可能性は、同一成果またはstacked PRの根拠になりません。包括的なtitleで複数成果を1つに言い換えることも認めません。
- test、documentation、migration、必要なpreparatory refactorは、同じ成果に不可欠なら同一PRに含められます。
- 複数commitを同一PRへ含める場合は、各commitが同じ受入条件に必要であることを説明できなければなりません。commit subjectが独立したuser-visible behaviorを表している場合は、具体的な技術的依存を示せない限り別PRにします。
- PR数はoutcome分解から決めます。ユーザーの「PRを作成して」という単数表現を、独立した成果を1本へまとめる本数指定とは解釈しません。ユーザーが明示的に1本への統合を求め、この規約と衝突する場合は、公開前に衝突と分割案を報告します。
- 原則として5 commits以内、production code追加500行以内とします。
- 目安を超えたら分割を再検討します。分割不能ならPR本文の`Why this cannot be split`に理由を記録します。
- failed checkがある状態でready PRを作りません。未完了状態の共有を明示的に依頼された場合だけdraft PRを使います。
- unrelatedなlocal変更はPRに含めません。

### Title

PR titleはrelease notesで単独でも理解できる成果を、primary prefix付き・72文字以内で表します。`test` secondary prefixはPR titleに付けません。

```text
[fix] Prevent updates while games are running
[update] Add play-status filtering
```

### Body

`.github/pull_request_template.md`に従い、最低限次を記録します。

- Purpose: 何を解決するか
- Changes: 何を変えたか
- Verification: 実行したcommandと結果、必要なmanual test
- Scope: commit数とproduction additions
- Related PRs: dependencyとmerge順
- Why this cannot be split: 複数commitが同じ成果に不可欠な具体的依存、またはsize/scope例外の理由。単一commitで例外がなければ`None`

CodexはPR作成後、URL、base/head、commit一覧、検証結果、例外、current branchを報告します。CIやreviewへのfollow-upに備えてhead branchに留まり、PR作成の依頼だけでは`main`へのswitch、branch削除、mergeを行いません。

## Multiple and stacked pull requests

複数の変更がlocalに存在していても、論理的に独立していればそれぞれ`main`からbranchします。conflictの可能性だけを理由にstackしません。単独管理で即時mergeする通常運用では、先行PRをmergeし、`main`を更新してから次のPRを作る方法を優先します。

BがAのcode、schema、APIを技術的に前提とし、Aのmerge前にBもPRとして公開する必要がある場合だけstackします。

```text
main <- update/a
update/a <- update/b
```

後続PRの本文には次を記録します。

```markdown
## Related PRs

- Depends on #123
- Merge order: #123, then this PR
- Current base: `update/a`
```

先行PRをmergeした後、後続PRのbaseを`main`へ変更し、先行PRのdiffが重複しないことを確認してからmergeします。stacked PRは先行順にmergeし、後続を先にmergeしません。

## Merge policy

- GitHubの`Create a merge commit`を使用し、review済みのcommit境界とmessageを保持します。
- Squash mergeは使用しません。commit規則で作った境界を失うためです。
- Rebase mergeはstacked branchのrewrite/force-pushを必要にしやすいため使用しません。
- topic branch内へ`main`のmerge commitを取り込まず、必要ならPR作成前に安全に更新します。
- branch deletionは、そのbranchをbaseにするstacked PRを`main`へretargetした後に行います。

## Repository enforcement

GitHub rulesetは`main`を対象にし、pull requestとrequired PR checksを必須とします。別の承認用rulesetでapprovalを1件以上、Code Ownerの承認を必須とし、`.github/CODEOWNERS`で全fileのownerを`@tinypark2010`に指定します。承認用rulesetだけはRepository adminに`For pull requests only`のbypassを許可し、owner自身のPRも承認なしでmergeできるようにします。CI必須rulesetにはbypass actorを追加せず、ownerのPRもCI通過を必須にします。repository管理画面ではmerge commitを許可し、squash/rebase mergeを無効にします。

`.github/workflows/ci.yml`はstacked PRを含むすべてのpull requestでcommit policy、frontend/Rust checks、tests、build、dependency license生成を実行します。既存のrelease workflowはtag publish専用です。

このworkflowが一度成功した後、GitHubのrepository settingsで次を設定します。

1. `main`を対象にするactive rulesetを作成する。
2. CI必須rulesetでは`Require a pull request before merging`を有効にし、required approvalsは0、bypass actorはなしにする。
3. CI必須rulesetで`Audit Rust dependency licenses`と`Verify pull request`をrequired status checksにする。
4. 別の承認用rulesetで`Require a pull request before merging`、required approvals 1、`Require review from Code Owners`、`Dismiss stale pull request approvals when new commits are pushed`を有効にする。
5. 承認用rulesetのbypass listにRepository adminだけを追加し、`For pull requests only`にする。ownerは自身のPRをbypassでmergeでき、他のcollaboratorはownerの承認が必要になる。
6. repositoryのpull request merge methodは`Create a merge commit`だけを有効にする。
7. `main`とactive stacked PRのbase branchを削除対象から保護する。
