<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { getVersion } from '@tauri-apps/api/app';
  import { open, save as saveFile } from '@tauri-apps/plugin-dialog';
  import { relaunch } from '@tauri-apps/plugin-process';
  import { check } from '@tauri-apps/plugin-updater';
  import { api } from '../lib/api';
  import PauseHistory from './PauseHistory.svelte';
  import SettingsIcon from './SettingsIcon.svelte';
  import { userErrorMessage } from '../lib/errors';
  import type {
    BackupExportProgress,
    BackupImportPreview,
    Settings,
    Theme,
    TrackingStatus,
  } from '../lib/types';
  export let ontheme: (theme: Theme) => void = () => {};
  export let ondirty: (dirty: boolean) => void = () => {};
  export let onsaved: (settings: Settings) => void = () => {};
  export let trackingActive = false;
  export let status: TrackingStatus = { games: [], is_paused: false, pause_started_at: null };
  export let onstatus: (status: TrackingStatus) => void = () => {};
  export let ontogglepause: () => void = () => {};
  export let pauseBusy = false;
  let historyOpen = false;
  let saving = false;
  const categories = [
    { id: 'tracking', label: 'プレイ時間', icon: 'clock' },
    { id: 'shortcuts', label: 'ショートカット', icon: 'keyboard' },
    { id: 'appearance', label: '表示と動作', icon: 'palette' },
    { id: 'updates', label: '更新', icon: 'refresh' },
    { id: 'data', label: 'データ', icon: 'database' },
  ] as const;
  const themes = [
    { value: 'dark', label: 'ダーク' },
    { value: 'light', label: 'ライト' },
    { value: 'pink', label: 'ピンク' },
    { value: 'blue', label: 'ブルー' },
  ] as const;
  type HotkeyField = 'screenshot_hotkey' | 'ocr_search_hotkey' | 'tracking_pause_hotkey';
  const hotkeyFields: Array<{ field: HotkeyField; label: string }> = [
    { field: 'screenshot_hotkey', label: 'スクリーンショット' },
    { field: 'ocr_search_hotkey', label: 'OCR検索' },
    { field: 'tracking_pause_hotkey', label: '計測の一時停止／再開' },
  ];
  let recordingHotkey: HotkeyField | null = null;
  let settings: Settings = {
      autostart: false,
      auto_check_updates: true,
      skipped_update_version: null,
      close_to_tray: true,
      exclude_background_time: true,
      exclude_pause_time: true,
      track_only_playing_status: false,
      tracking_pause_hotkey: '',
      theme: 'dark',
      screenshot_hotkey: '',
      ocr_search_hotkey: '',
    },
    message = '',
    error = '',
    hotkeyError = '',
    checkingHotkey = false,
    hotkeyStatus = '',
    currentVersion = '',
    updateStatus = '',
    updateError = '',
    checkingUpdate = false,
    installingUpdate = false,
    updateDownloaded = 0,
    updateContentLength: number | undefined,
    backupBusy = false,
    backupMessage = '',
    backupError = '',
    importConfirmed = false,
    destroyed = false,
    includeScreenshots = true;
  let importPreview: BackupImportPreview | null = null;
  let backupProgress: BackupExportProgress | null = null;
  let availableUpdate: Awaited<ReturnType<typeof check>> = null;
  let previewUpdateAvailable = false;
  $: availableVersion = availableUpdate?.version ?? '0.1.6';
  let savedSettings: Settings | null = null,
    lastDirty = false;
  $: dirty =
    recordingHotkey !== null ||
    checkingHotkey ||
    (savedSettings !== null && JSON.stringify(settings) !== JSON.stringify(savedSettings));
  $: {
    if (dirty !== lastDirty) {
      lastDirty = dirty;
      ondirty(dirty);
    }
  }
  onMount(async () => {
    try {
      [settings, currentVersion] = await Promise.all([api.settings(), getVersion()]);
      savedSettings = { ...settings };
      ontheme(settings.theme);
      if (
        settings.screenshot_hotkey ||
        settings.ocr_search_hotkey ||
        settings.tracking_pause_hotkey
      ) {
        try {
          await api.validateHotkeys(settings);
        } catch (e) {
          hotkeyError = userErrorMessage(e, 'ショートカットキーを確認できませんでした。');
        }
      }
    } catch (e) {
      error = userErrorMessage(e, '設定を読み込めませんでした。');
    }
  });
  onDestroy(() => {
    destroyed = true;
    if (recordingHotkey || checkingHotkey) api.resumeHotkeys().catch(() => {});
    if (importPreview && !importConfirmed) {
      api.cancelBackupImport(importPreview.import_id).catch(() => {});
    }
  });
  function selectTheme(theme: Theme) {
    settings.theme = theme;
    ontheme(theme);
  }
  async function save() {
    if (recordingHotkey || checkingHotkey || saving) return;
    saving = true;
    message = error = '';
    try {
      await api.updateSettings(settings);
      settings = await api.settings();
      savedSettings = { ...settings };
      onsaved(settings);
      ondirty(false);
      message = '保存しました';
      error = '';
      hotkeyError = '';
    } catch (e) {
      const saveError = userErrorMessage(e, '設定を保存できませんでした。');
      error = saveError;
      if (saveError.includes('キー') || saveError.includes('ホット')) {
        hotkeyError = saveError;
      } else {
        error = saveError;
      }
    } finally {
      saving = false;
    }
  }
  function hotkeyFromEvent(event: KeyboardEvent) {
    if (['Control', 'Alt', 'Shift', 'Meta'].includes(event.key)) return '';
    const modifiers = [
      event.ctrlKey ? 'Ctrl' : '',
      event.altKey ? 'Alt' : '',
      event.shiftKey ? 'Shift' : '',
      event.metaKey ? 'Win' : '',
    ].filter(Boolean);
    let key = event.key;
    if (key.length === 1) key = key.toUpperCase();
    if (key === 'PrintScreen') key = 'PrintScreen';
    return [...modifiers, key].join('+');
  }
  async function resumeHotkeys() {
    await api
      .resumeHotkeys()
      .catch(
        (e) => (hotkeyError = userErrorMessage(e, 'ショートカットキーを再登録できませんでした。')),
      );
  }
  async function recordHotkey(event: KeyboardEvent) {
    if (!recordingHotkey) return;
    event.preventDefault();
    event.stopPropagation();
    if (checkingHotkey || event.repeat || event.isComposing) return;
    if (event.key === 'Escape') {
      recordingHotkey = null;
      hotkeyStatus = '';
      checkingHotkey = true;
      await resumeHotkeys();
      checkingHotkey = false;
      return;
    }
    const candidate = hotkeyFromEvent(event);
    if (!candidate) return;
    const field = recordingHotkey;
    checkingHotkey = true;
    error = '';
    hotkeyError = '';
    hotkeyStatus = `${candidate} を確認中…`;
    try {
      await api.validateHotkeys({ ...settings, [field]: candidate });
      if (!destroyed) settings[field] = candidate;
      recordingHotkey = null;
    } catch (e) {
      hotkeyError = userErrorMessage(e, 'ショートカットキーを確認できませんでした。');
    } finally {
      hotkeyStatus = '';
      // Keep both keys suspended after rejection so the next attempt can be recorded.
      if (!recordingHotkey || destroyed) await resumeHotkeys();
      checkingHotkey = false;
    }
  }
  async function startHotkeyRecording(field: HotkeyField) {
    if (recordingHotkey || checkingHotkey) return;
    hotkeyStatus = '';
    error = '';
    hotkeyError = '';
    checkingHotkey = true;
    try {
      await api.suspendHotkeys();
      if (destroyed) await resumeHotkeys();
      else recordingHotkey = field;
    } catch (e) {
      hotkeyError = userErrorMessage(e, 'ショートカットキーを解除できませんでした。');
    } finally {
      checkingHotkey = false;
    }
  }
  function clearHotkey(field: HotkeyField) {
    if (recordingHotkey || checkingHotkey) return;
    settings[field] = '';
    hotkeyStatus = '';
    error = '';
    hotkeyError = '';
  }
  async function checkForUpdate() {
    if (checkingUpdate || installingUpdate) return;
    checkingUpdate = true;
    availableUpdate = null;
    previewUpdateAvailable = false;
    updateError = '';
    updateStatus = '更新を確認しています…';
    try {
      if (import.meta.env.DEV && import.meta.env.VITE_MOCK_UPDATE === 'true') {
        previewUpdateAvailable = true;
        updateStatus = `バージョン ${availableVersion} を利用できます。`;
        return;
      }
      availableUpdate = await check();
      updateStatus = availableUpdate
        ? `バージョン ${availableUpdate.version} を利用できます。`
        : '現在のバージョンが最新です。';
    } catch (e) {
      updateStatus = '';
      updateError = userErrorMessage(e, '更新を確認できませんでした。');
    } finally {
      checkingUpdate = false;
    }
  }
  async function installAvailableUpdate() {
    let gameIsRunning = trackingActive;
    try {
      gameIsRunning ||= (await api.status()).games.length > 0;
    } catch (e) {
      updateError = userErrorMessage(e, 'ゲームの起動状態を確認できませんでした。');
      return;
    }
    if (gameIsRunning) {
      updateError = '';
      updateStatus = '起動中のゲームを終了してから更新してください。';
      return;
    }
    if (previewUpdateAvailable) {
      updateError = '';
      updateStatus = 'モック表示のため、実際の更新は行いません。';
      return;
    }
    if (!availableUpdate || installingUpdate) return;
    installingUpdate = true;
    updateError = '';
    updateDownloaded = 0;
    updateContentLength = undefined;
    try {
      await availableUpdate.downloadAndInstall((event) => {
        if (event.event === 'Started') {
          updateContentLength = event.data.contentLength ?? undefined;
        } else if (event.event === 'Progress') {
          updateDownloaded += event.data.chunkLength;
        }
      });
      await relaunch();
    } catch (e) {
      updateError = userErrorMessage(e, '更新できませんでした。もう一度お試しください。');
      installingUpdate = false;
    }
  }
  function updateProgressText() {
    if (!updateContentLength) return '更新をダウンロードしています…';
    const percent = Math.min(100, Math.round((updateDownloaded / updateContentLength) * 100));
    return `更新をダウンロードしています… ${percent}%`;
  }
  function backupFilename() {
    const date = new Date();
    const year = date.getFullYear();
    const month = String(date.getMonth() + 1).padStart(2, '0');
    const day = String(date.getDate()).padStart(2, '0');
    return `eroge-playtime-tracker-${year}-${month}-${day}.eptbackup`;
  }
  function formatBytes(bytes: number) {
    if (bytes < 1024) return `${bytes} B`;
    const units = ['KB', 'MB', 'GB', 'TB'];
    let value = bytes / 1024;
    let unit = units[0];
    for (let index = 1; index < units.length && value >= 1024; index++) {
      value /= 1024;
      unit = units[index];
    }
    return `${value.toFixed(value >= 10 ? 1 : 2)} ${unit}`;
  }
  function updateBackupProgress(progress: BackupExportProgress, automatic = false) {
    backupProgress = progress;
    const target = automatic ? '現在のデータを自動バックアップ' : 'バックアップ';
    if (progress.phase === 'preparing') {
      backupMessage = `${target}する準備をしています…`;
    } else if (progress.phase === 'archiving') {
      const percent = progress.total_bytes
        ? Math.min(100, Math.round((progress.processed_bytes / progress.total_bytes) * 100))
        : 0;
      backupMessage = `${target}しています… ${percent}%`;
    } else {
      backupMessage = `${target}を仕上げています…`;
    }
  }
  async function exportData() {
    if (backupBusy || trackingActive) return;
    let destination: string | null;
    try {
      destination = await saveFile({
        defaultPath: backupFilename(),
        filters: [{ name: 'Eroge Playtime Tracker バックアップ', extensions: ['eptbackup'] }],
      });
    } catch (e) {
      backupError = userErrorMessage(e, '保存先を選択できませんでした。');
      return;
    }
    if (!destination) return;
    backupBusy = true;
    backupProgress = null;
    backupMessage = 'バックアップを作成しています…';
    backupError = '';
    try {
      const result = await api.exportBackup(destination, includeScreenshots, (progress) =>
        updateBackupProgress(progress),
      );
      if (destroyed) return;
      backupMessage = `バックアップを作成しました（${formatBytes(result.file_size)}）。`;
      if (!result.includes_screenshots) {
        backupMessage += ' スクリーンショットは除外しました。';
      }
      if (result.missing_media_count > 0) {
        backupMessage += ` 見つからなかった画像 ${result.missing_media_count} 件は含まれていません。`;
      }
    } catch (e) {
      backupMessage = '';
      backupError = userErrorMessage(e, 'バックアップを作成できませんでした。');
    } finally {
      backupProgress = null;
      backupBusy = false;
    }
  }
  async function chooseImport() {
    if (backupBusy || trackingActive) return;
    let source: string | string[] | null;
    try {
      source = await open({
        multiple: false,
        directory: false,
        filters: [{ name: 'Eroge Playtime Tracker バックアップ', extensions: ['eptbackup'] }],
      });
    } catch (e) {
      backupError = userErrorMessage(e, 'バックアップファイルを選択できませんでした。');
      return;
    }
    if (!source || Array.isArray(source)) return;
    backupBusy = true;
    backupProgress = null;
    backupMessage = 'バックアップを検証しています…';
    backupError = '';
    try {
      const preview = await api.prepareBackupImport(source);
      if (destroyed) {
        await api.cancelBackupImport(preview.import_id).catch(() => {});
        return;
      }
      importPreview = preview;
      backupMessage = '';
    } catch (e) {
      backupMessage = '';
      backupError = userErrorMessage(e, 'バックアップを読み込めませんでした。');
    } finally {
      backupProgress = null;
      backupBusy = false;
    }
  }
  async function cancelImport() {
    if (!importPreview || backupBusy) return;
    const preview = importPreview;
    importPreview = null;
    try {
      await api.cancelBackupImport(preview.import_id);
    } catch (e) {
      backupError = userErrorMessage(e, 'インポートの準備データを削除できませんでした。');
    }
  }
  async function confirmImport() {
    if (!importPreview || backupBusy || trackingActive) return;
    backupBusy = true;
    backupProgress = null;
    backupError = '';
    try {
      await api.confirmBackupImport(importPreview.import_id, (progress) =>
        updateBackupProgress(progress, true),
      );
      importConfirmed = true;
      backupMessage = 'インポートを適用するため再起動しています…';
      await relaunch();
    } catch (e) {
      importConfirmed = false;
      await api.cancelBackupImport(importPreview.import_id).catch(() => {});
      backupMessage = '';
      backupProgress = null;
      backupError = userErrorMessage(e, 'インポートを開始できませんでした。');
      importPreview = null;
      backupBusy = false;
    }
  }
</script>

<svelte:window onkeydown={recordHotkey} />

<section class="settings-page">
  <div class="settings-page-heading">
    <h1>設定</h1>
    <div class="settings-save-actions">
      <span aria-live="polite">{dirty ? '未保存の変更' : message}</span>
      <button
        class="primary settings-save"
        disabled={!savedSettings || !dirty || saving || recordingHotkey !== null || checkingHotkey}
        onclick={save}><SettingsIcon name="check" />{saving ? '保存中…' : '変更を保存'}</button
      >
    </div>
  </div>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  <div class="settings-layout">
    <nav class="settings-nav" aria-label="設定カテゴリ">
      {#each categories as category}<a href={`#settings-${category.id}`}
          ><SettingsIcon name={category.icon} />{category.label}</a
        >{/each}
    </nav>
    <div class="settings-sections">
      <section
        id="settings-tracking"
        class="settings-section"
        aria-labelledby="settings-tracking-title"
      >
        <div class="settings-section-heading">
          <SettingsIcon name="clock" />
          <h2 id="settings-tracking-title">プレイ時間</h2>
          <span class="settings-scope">全ゲーム共通</span>
        </div>
        <div class="settings-card">
          <div class="settings-pause-control" class:is-paused={status.is_paused}>
            <span class="settings-pause-symbol"
              ><SettingsIcon name={status.is_paused ? 'pause' : 'clock'} /></span
            >
            <div class="settings-pause-state">
              <div class="settings-pause-title">
                <strong>プレイ時間の計測</strong>{#if status.is_paused}<span class="pause-live-tag"
                    >一時停止中</span
                  >{/if}
              </div>
              <span
                >{status.is_paused
                  ? savedSettings?.exclude_pause_time
                    ? 'プレイ時間から除外中'
                    : '停止時間をプレイ時間に含む設定です'
                  : 'ゲーム起動時に自動で計測'}</span
              >
            </div>
            <button class="settings-pause-toggle" disabled={pauseBusy} onclick={ontogglepause}
              ><SettingsIcon name={status.is_paused ? 'play' : 'pause'} />{status.is_paused
                ? '計測を再開'
                : '計測を一時停止'}</button
            >
          </div>
          <div class="settings-pause-links">
            <a href="#settings-shortcuts"
              ><SettingsIcon name="keyboard" /><span>停止・再開キー</span><kbd
                >{settings.tracking_pause_hotkey || 'キー未設定'}</kbd
              ></a
            ><button
              class="settings-text-button"
              disabled={recordingHotkey !== null || checkingHotkey}
              onclick={() => (historyOpen = true)}
              ><SettingsIcon name="history" />一時停止の履歴<SettingsIcon name="arrow" /></button
            >
          </div>
          <label class="settings-row settings-switch-row"
            ><span
              ><strong>一時停止時間を除外</strong><small
                >プレイ時間に含めない。過去の履歴・統計にも適用</small
              ></span
            ><input
              class="settings-switch"
              type="checkbox"
              role="switch"
              bind:checked={settings.exclude_pause_time}
            /></label
          >
          <label class="settings-row settings-switch-row"
            ><span
              ><strong>バックグラウンド時間を除外</strong><small
                >ゲームが最前面にない時間。過去の履歴・統計にも適用</small
              ></span
            ><input
              class="settings-switch"
              type="checkbox"
              role="switch"
              bind:checked={settings.exclude_background_time}
            /></label
          >
          <label class="settings-row settings-switch-row"
            ><span
              ><strong>「攻略中」のみ計測</strong><small
                >それ以外のプレイ状況のゲームは一時停止として扱う</small
              ></span
            ><input
              class="settings-switch"
              type="checkbox"
              role="switch"
              bind:checked={settings.track_only_playing_status}
              disabled={trackingActive}
            /></label
          >
          {#if trackingActive}<p class="settings-card-note">
              起動中のゲームがあるため、終了すると切り替えられます。
            </p>{/if}
        </div>
      </section>
      <section
        id="settings-shortcuts"
        class="settings-section"
        aria-labelledby="settings-shortcuts-title"
      >
        <div class="settings-section-heading">
          <SettingsIcon name="keyboard" />
          <h2 id="settings-shortcuts-title">ショートカット</h2>
        </div>
        <div class="settings-card settings-shortcuts">
          {#each hotkeyFields as { field, label }}
            <div class="settings-hotkey-row">
              <div class="settings-hotkey-label">
                <strong>{label}</strong>{#if field === 'ocr_search_hotkey'}<small
                    >ゲーム画面の文字を選択して検索</small
                  >{:else if field === 'screenshot_hotkey'}<small>最前面のゲームを撮影</small
                  >{:else}<small>全ゲーム共通</small>{/if}
              </div>
              <div
                class:recording={recordingHotkey === field}
                class:invalid={Boolean(hotkeyError)}
                class="hotkey-recorder"
              >
                <kbd
                  >{recordingHotkey === field
                    ? 'キーを押してください…'
                    : settings[field] || '未設定'}</kbd
                >
                <button
                  type="button"
                  disabled={saving || checkingHotkey || recordingHotkey !== null}
                  onclick={() => startHotkeyRecording(field)}
                  >{settings[field] ? '変更' : '設定'}</button
                >
                <button
                  class="settings-icon-button"
                  type="button"
                  aria-label={`${label}のキーを解除`}
                  title="キーを解除"
                  disabled={!settings[field] ||
                    saving ||
                    checkingHotkey ||
                    recordingHotkey !== null}
                  onclick={() => clearHotkey(field)}><SettingsIcon name="close" /></button
                >
              </div>
            </div>
          {/each}
          {#if recordingHotkey}<p class="settings-card-note" role="status">
              キーを入力 · Escでキャンセル
            </p>{/if}
          {#if hotkeyStatus}<p class="hotkey-status" role="status">{hotkeyStatus}</p>{/if}
          {#if hotkeyError}<p class="error" role="alert">{hotkeyError}</p>{/if}
        </div>
      </section>
      <section
        id="settings-appearance"
        class="settings-section"
        aria-labelledby="settings-appearance-title"
      >
        <div class="settings-section-heading">
          <SettingsIcon name="palette" />
          <h2 id="settings-appearance-title">表示と動作</h2>
        </div>
        <div class="settings-card">
          <div class="settings-theme-field">
            <strong>カラーテーマ</strong>
            <div class="settings-theme-options" role="group" aria-label="カラーテーマ">
              {#each themes as theme}<button
                  class={`settings-theme-option preview-${theme.value}`}
                  aria-pressed={settings.theme === theme.value}
                  onclick={() => selectTheme(theme.value)}
                  ><span class="settings-theme-mini" aria-hidden="true"
                    ><span></span><i></i><i></i></span
                  ><span
                    >{theme.label}{#if settings.theme === theme.value}<SettingsIcon
                        name="check"
                      />{/if}</span
                  ></button
                >{/each}
            </div>
          </div>
          <label class="settings-row settings-switch-row"
            ><span><strong>Windowsログイン時に起動</strong></span><input
              class="settings-switch"
              type="checkbox"
              role="switch"
              bind:checked={settings.autostart}
            /></label
          >
          <label class="settings-row settings-switch-row"
            ><span><strong>閉じるとトレイへ格納</strong></span><input
              class="settings-switch"
              type="checkbox"
              role="switch"
              bind:checked={settings.close_to_tray}
            /></label
          >
        </div>
      </section>
      <section
        id="settings-updates"
        class="settings-section"
        aria-labelledby="settings-updates-title"
      >
        <div class="settings-section-heading">
          <SettingsIcon name="refresh" />
          <h2 id="settings-updates-title">アプリの更新</h2>
        </div>
        <div class="settings-card">
          <div class="settings-row">
            <div class="settings-version">
              <strong>Eroge Playtime Tracker</strong><span>v{currentVersion || '—'}</span>
            </div>
            <button
              type="button"
              disabled={checkingUpdate || installingUpdate}
              onclick={checkForUpdate}
              ><SettingsIcon name="refresh" />{checkingUpdate ? '確認中…' : '更新を確認'}</button
            >
          </div>
          <label class="settings-row settings-switch-row"
            ><span><strong>起動時に更新を確認</strong></span><input
              class="settings-switch"
              type="checkbox"
              role="switch"
              bind:checked={settings.auto_check_updates}
            /></label
          >
          {#if settings.skipped_update_version}<p class="settings-card-note">
              v{settings.skipped_update_version} の通知はスキップ中
            </p>{/if}
          {#if updateStatus || updateError || availableUpdate || previewUpdateAvailable || installingUpdate}<div
              class="settings-feedback"
              aria-live="polite"
            >
              {#if updateStatus}<p>{updateStatus}</p>{/if}
              {#if updateError}<p class="error" role="alert">{updateError}</p>{/if}
              {#if availableUpdate || previewUpdateAvailable}<button
                  class="primary"
                  disabled={installingUpdate || trackingActive}
                  onclick={installAvailableUpdate}
                  >{installingUpdate ? '更新中…' : '更新して再起動'}</button
                >{#if trackingActive}<p class="hint">
                    起動中のゲームを終了してください。
                  </p>{/if}{/if}
              {#if installingUpdate}<p>{updateProgressText()}</p>
                {#if updateContentLength}<progress
                    value={updateDownloaded}
                    max={updateContentLength}
                  ></progress>{/if}{/if}
            </div>{/if}
        </div>
      </section>
      <section id="settings-data" class="settings-section" aria-labelledby="settings-data-title">
        <div class="settings-section-heading">
          <SettingsIcon name="database" />
          <h2 id="settings-data-title">データの移行</h2>
        </div>
        <div class="settings-card">
          <div class="settings-data-actions">
            <button disabled={backupBusy || trackingActive} onclick={exportData}
              ><SettingsIcon name="upload" /><strong
                >{backupBusy && !importPreview ? '処理中…' : 'エクスポート'}</strong
              ><small>バックアップを保存</small></button
            ><button disabled={backupBusy || trackingActive} onclick={chooseImport}
              ><SettingsIcon name="download" /><strong>インポート</strong><small
                >バックアップから復元</small
              ></button
            >
          </div>
          <label class="settings-row settings-switch-row"
            ><span><strong>スクリーンショットを含める</strong><small>エクスポート時</small></span
            ><input
              class="settings-switch"
              type="checkbox"
              role="switch"
              bind:checked={includeScreenshots}
              disabled={backupBusy}
            /></label
          >
          {#if trackingActive}<p class="settings-card-note">
              起動中のゲームを終了すると操作できます。
            </p>{/if}
          {#if backupMessage || backupError || backupBusy}<div
              class="settings-feedback"
              aria-live="polite"
            >
              {#if backupMessage}<p>{backupMessage}</p>{/if}{#if backupError}<p
                  class="error"
                  role="alert"
                >
                  {backupError}
                </p>{/if}
              {#if backupBusy}<div class="backup-progress">
                  {#if backupProgress?.phase === 'archiving' && backupProgress.total_bytes > 0}<progress
                      value={backupProgress.processed_bytes}
                      max={backupProgress.total_bytes}
                    ></progress><small
                      >{formatBytes(backupProgress.processed_bytes)} / {formatBytes(
                        backupProgress.total_bytes,
                      )}</small
                    >{:else}<progress></progress>{/if}
                </div>{/if}
            </div>{/if}
        </div>
      </section>
    </div>
  </div>
</section>
{#if historyOpen}<PauseHistory
    {status}
    onchange={onstatus}
    onclose={() => (historyOpen = false)}
  />{/if}

{#if importPreview}<div class="modal backup-import-modal">
    <div
      class="panel backup-import-dialog"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="backup-import-title"
      aria-describedby="backup-import-message"
    >
      <div class="confirm-icon">!</div>
      <h2 id="backup-import-title">現在のデータを置き換えますか？</h2>
      <p id="backup-import-message">
        インポートは追加・統合ではありません。現在のデータをバックアップ内容で置き換えて再起動します。置換前のデータは自動で退避します。
      </p>
      <dl class="backup-import-summary">
        <div>
          <dt></dt>
          <dd>現在</dd>
          <dd>バックアップ</dd>
        </div>
        <div>
          <dt>ゲーム</dt>
          <dd>{importPreview.current_summary.game_count} 件</dd>
          <dd>{importPreview.summary.game_count} 件</dd>
        </div>
        <div>
          <dt>プレイ履歴</dt>
          <dd>{importPreview.current_summary.session_count} 件</dd>
          <dd>{importPreview.summary.session_count} 件</dd>
        </div>
        <div>
          <dt>タイムスタンプ</dt>
          <dd>{importPreview.current_summary.timestamp_count} 件</dd>
          <dd>{importPreview.summary.timestamp_count} 件</dd>
        </div>
        <div>
          <dt>スクリーンショット</dt>
          <dd>{importPreview.current_summary.screenshot_count} 件</dd>
          <dd>
            {importPreview.summary.screenshot_count} 件{importPreview.includes_screenshots
              ? ''
              : '（除外）'}
          </dd>
        </div>
        <div>
          <dt>サムネイル</dt>
          <dd>{importPreview.current_summary.thumbnail_count} 件</dd>
          <dd>{importPreview.summary.thumbnail_count} 件</dd>
        </div>
      </dl>
      <p class="hint">
        作成日時: {new Date(importPreview.exported_at).toLocaleString('ja-JP')} ／ アプリ {importPreview.app_version}
        ／ {formatBytes(importPreview.file_size)}
      </p>
      {#if !importPreview.includes_screenshots}<p class="backup-warning">
          このバックアップにはスクリーンショットが含まれていません。置き換えると、現在のスクリーンショットも削除されます。
        </p>{/if}
      {#if importPreview.missing_executable_count > 0}<p class="backup-warning">
          登録済みの実行ファイルのうち {importPreview.missing_executable_count}
          件はこのPCで見つかりません。移行後にゲーム詳細から登録し直してください。
        </p>{/if}
      <div class="confirm-actions">
        <button disabled={backupBusy} onclick={cancelImport}>キャンセル</button>
        <button class="danger" disabled={backupBusy || trackingActive} onclick={confirmImport}>
          {backupBusy ? '準備中…' : '置き換えて再起動'}
        </button>
      </div>
    </div>
  </div>{/if}
