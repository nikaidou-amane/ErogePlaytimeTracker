<script lang="ts">
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import Library from './components/Library.svelte';
  import Statistics from './components/Statistics.svelte';
  import GameDetail from './components/GameDetail.svelte';
  import AddGame from './components/AddGame.svelte';
  import Settings from './components/Settings.svelte';
  import UpdatePrompt from './components/UpdatePrompt.svelte';
  import UpdateCompletionNotice from './components/UpdateCompletionNotice.svelte';
  import { api } from './lib/api';
  import { trackingStatusGroups, trackingStatusText } from './lib/trackingStatus';
  import type {
    BackupImportNotice,
    Theme,
    TrackingStatus,
    UpdateCompletionNotice as CompletionNotice,
  } from './lib/types';
  type Page = 'library' | 'statistics' | 'game' | 'add' | 'settings';
  let page: Page = 'library',
    gameId = 0,
    refresh = 0,
    status: TrackingStatus = { games: [], paused: false },
    savedTheme: Theme = 'dark',
    autoCheckUpdates = true,
    excludeBackgroundTime = true,
    skippedUpdateVersion: string | null = null,
    settingsLoaded = false,
    settingsDirty = false,
    pendingPage: Page | null = null,
    pendingReload = false,
    gameReturnPage: 'library' | 'statistics' = 'library',
    importNotice: BackupImportNotice | null = null,
    gameOcrError = '';
  let completionNotice: CompletionNotice | null = null;
  let completionLoaded = false;
  let completionWarning = '';
  const completionPreview =
    import.meta.env.DEV && ['true', 'empty'].includes(import.meta.env.VITE_MOCK_UPDATE_NOTICE);
  $: startupBlocked = !completionLoaded || completionNotice !== null;
  function goTo(next: Page, shouldReload = false) {
    if (startupBlocked) return;
    if (page === 'settings' && next !== 'settings' && settingsDirty) {
      pendingPage = next;
      pendingReload = shouldReload;
      return;
    }
    page = next;
    if (shouldReload) reload();
  }
  function discardSettingsAndLeave() {
    if (!pendingPage) return;
    applyTheme(savedTheme);
    settingsDirty = false;
    page = pendingPage;
    if (pendingReload) reload();
    pendingPage = null;
    pendingReload = false;
  }
  const openGame = (id: number) => {
    if (page === 'library' || page === 'statistics') gameReturnPage = page;
    gameId = id;
    goTo('game');
  };
  const reload = () => refresh++;
  $: statusGroups = trackingStatusGroups(status);
  const applyTheme = (theme: Theme) => (document.documentElement.dataset.theme = theme);
  function updateStatus(next: TrackingStatus) {
    status = next;
  }
  async function setPaused(paused: boolean) {
    try {
      await api.setTrackingPaused(paused);
    } catch (error) {
      console.warn('追跡状態を切り替えられませんでした', error);
    }
  }
  onMount(() => {
    api
      .settings()
      .then((v) => {
        savedTheme = v.theme;
        autoCheckUpdates = v.auto_check_updates;
        excludeBackgroundTime = v.exclude_background_time;
        skippedUpdateVersion = v.skipped_update_version;
        applyTheme(v.theme);
      })
      .catch((error) => console.warn('設定の読み込みに失敗しました', error))
      .then(() => (settingsLoaded = true));
    const pendingNotice: Promise<CompletionNotice | null> = completionPreview
      ? Promise.resolve({
          version: '1.2.3',
          releases:
            import.meta.env.VITE_MOCK_UPDATE_NOTICE === 'empty'
              ? []
              : [
                  {
                    version: '1.2.3',
                    changes: [
                      '新機能の説明がここに表示されます。',
                      '修正された不具合の説明がここに表示されます。',
                    ],
                  },
                  {
                    version: '1.2.2',
                    changes: ['途中のバージョンの更新内容もまとめて確認できます。'],
                  },
                ],
        })
      : api.updateCompletionNotice();
    pendingNotice
      .then((notice) => (completionNotice = notice))
      .catch((error) => console.warn('更新完了通知を読み込めませんでした', error))
      .then(() => (completionLoaded = true));
    api.status().then(updateStatus);
    api
      .takeBackupImportNotice()
      .then((notice) => (importNotice = notice))
      .catch(() => {});
    const timer = setInterval(() => api.status().then(updateStatus), 3000);
    let off = () => {};
    let offOcr = () => {};
    let destroyed = false;
    listen<TrackingStatus>('tracking-status', (e) => updateStatus(e.payload)).then(
      (f) => (off = f),
    );
    listen<string>('game-ocr-error', (e) => (gameOcrError = e.payload)).then((f) => {
      if (destroyed) f();
      else offOcr = f;
    });
    return () => {
      destroyed = true;
      clearInterval(timer);
      off();
      offOcr();
    };
  });
</script>

<header>
  <button
    class="brand"
    disabled={startupBlocked}
    onclick={() => {
      goTo('library', true);
    }}>Eroge Playtime Tracker</button
  >
  <nav>
    <button
      disabled={startupBlocked}
      class:active={page === 'library'}
      onclick={() => goTo('library')}>ライブラリ</button
    ><button
      disabled={startupBlocked}
      class:active={page === 'statistics'}
      onclick={() => goTo('statistics')}>統計</button
    ><button disabled={startupBlocked} class:active={page === 'add'} onclick={() => goTo('add')}
      >ゲーム追加</button
    ><button
      disabled={startupBlocked}
      class:active={page === 'settings'}
      onclick={() => goTo('settings')}>設定</button
    >
  </nav>
  <div class="tracking-statuses">
    {#if status.paused}<span
        class="tracking-status paused"
        title="タスクトレイまたは設定から再開できます">● 計測停止中</span
      >{:else if statusGroups.length === 0}<span class="tracking-status idle">● 待機中</span>{/if}
    {#if !status.paused}
      {#each statusGroups as group}<span
          class="tracking-status {group.phase}"
          title={group.games.map((game) => game.title).join('\n')}
          >● {trackingStatusText(group)}</span
        >{/each}
    {/if}
  </div>
</header>
<main>
  {#if completionWarning}<div class="app-notice warning" role="alert">
      <p>{completionWarning}</p>
      <button type="button" aria-label="通知を閉じる" onclick={() => (completionWarning = '')}
        >×</button
      >
    </div>{/if}
  {#if gameOcrError}<div class="app-notice warning" role="alert">
      <div>
        <strong>ゲーム画面のOCR検索</strong>
        <p>{gameOcrError}</p>
      </div>
      <button type="button" aria-label="通知を閉じる" onclick={() => (gameOcrError = '')}>×</button>
    </div>{/if}
  {#if importNotice}<div class="app-notice" class:warning={!importNotice.success} role="status">
      <div>
        <strong
          >{importNotice.success
            ? 'データ移行が完了しました'
            : 'データ移行を取り消しました'}</strong
        >
        <p>{importNotice.message}</p>
        <small title={importNotice.auto_backup_path}
          >自動バックアップ: {importNotice.auto_backup_path}</small
        >
      </div>
      <button type="button" aria-label="通知を閉じる" onclick={() => (importNotice = null)}
        >×</button
      >
    </div>{/if}
  {#if !startupBlocked}
    {#if page === 'library'}<Library
        {refresh}
        {openGame}
      />{:else if page === 'statistics'}<Statistics
        {openGame}
        trackingActive={status.games.length > 0}
      />{:else if page === 'game'}<GameDetail
        {excludeBackgroundTime}
        {gameId}
        onback={() => {
          page = gameReturnPage;
          if (gameReturnPage === 'library') reload();
        }}
      />{:else if page === 'add'}<AddGame
        ondone={(id) => {
          openGame(id);
        }}
        oncancel={() => (page = 'library')}
      />{:else}<Settings
        trackingActive={status.games.length > 0}
        trackingPaused={status.paused}
        onpausetoggle={() => setPaused(!status.paused)}
        ontheme={applyTheme}
        ondirty={(dirty) => (settingsDirty = dirty)}
        onsaved={(settings) => {
          savedTheme = settings.theme;
          autoCheckUpdates = settings.auto_check_updates;
          excludeBackgroundTime = settings.exclude_background_time;
          skippedUpdateVersion = settings.skipped_update_version;
        }}
      />{/if}
  {/if}
</main>
{#if settingsLoaded && completionNotice}
  <UpdateCompletionNotice
    notice={completionNotice}
    preview={completionPreview}
    onclose={(warning) => {
      completionWarning = warning;
      completionNotice = null;
    }}
  />
{/if}
{#if settingsLoaded && !startupBlocked}
  <UpdatePrompt
    autoCheck={autoCheckUpdates}
    skippedVersion={skippedUpdateVersion}
    trackingActive={status.games.length > 0}
    onskip={(version) => (skippedUpdateVersion = version)}
  />
{/if}
{#if pendingPage}<div class="modal confirm-modal">
    <div
      class="panel confirm-box"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="unsaved-settings-title"
      aria-describedby="unsaved-settings-message"
    >
      <div class="confirm-icon">!</div>
      <h2 id="unsaved-settings-title">設定の変更を破棄しますか？</h2>
      <p id="unsaved-settings-message">
        保存されていない変更があります。移動すると変更内容は破棄されます。
      </p>
      <div class="confirm-actions">
        <button onclick={() => (pendingPage = null)}>設定に戻る</button>
        <button class="danger" onclick={discardSettingsAndLeave}>破棄して移動</button>
      </div>
    </div>
  </div>{/if}
