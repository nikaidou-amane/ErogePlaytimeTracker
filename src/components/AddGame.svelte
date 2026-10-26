<script lang="ts">
  import { onDestroy } from 'svelte';
  import { open } from '@tauri-apps/plugin-dialog';
  import DateTimeSelect from './DateTimeSelect.svelte';
  import ThumbnailCropper from './ThumbnailCropper.svelte';
  import { api } from '../lib/api';
  import { metadataSourceLabel } from '../lib/metadata';
  import { userErrorMessage } from '../lib/errors';
  import { imageSrc } from '../lib/time';
  import type { Metadata } from '../lib/types';
  export let ondone: (id: number) => void;
  export let oncancel: () => void;
  let lookup = '',
    title = '',
    brand = '',
    release_date = '',
    releaseDateComplete = true,
    releaseDateError = '',
    thumbnail_path = '',
    paths = '',
    launch_command = '',
    metadata: Metadata | null = null,
    fetchingMeta = false,
    importingThumbnail = false,
    saving = false,
    cropOpen = false,
    error = '',
    toast = '',
    toastError = false,
    toastTimer: number | undefined;
  function showToast(message: string, isError = false) {
    toast = message;
    toastError = isError;
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = window.setTimeout(() => (toast = ''), 4000);
  }
  onDestroy(() => {
    if (toastTimer) clearTimeout(toastTimer);
  });
  async function fetchMeta() {
    if (fetchingMeta || importingThumbnail || saving) return;
    fetchingMeta = true;
    error = '';
    try {
      metadata = await api.fetchMetadata(lookup);
      title = metadata.title;
      brand = metadata.brand ?? '';
      release_date = metadata.release_date ?? '';
      releaseDateComplete = true;
      releaseDateError = '';
      thumbnail_path = metadata.thumbnail_path ?? '';
      showToast(`${metadataSourceLabel(metadata.source.provider)}からゲーム情報を取得しました`);
    } catch (e) {
      error = userErrorMessage(
        e,
        '情報を取得できませんでした。入力内容と通信状態を確認してください。',
      );
      showToast(error, true);
    } finally {
      fetchingMeta = false;
    }
  }
  async function save() {
    if (!title.trim() || fetchingMeta || importingThumbnail || saving) return;
    if (!releaseDateComplete) {
      releaseDateError = '発売日は年・月・日をすべて選択してください。';
      return;
    }
    saving = true;
    error = '';
    try {
      const id = await api.createGame({
        title,
        brand: brand || undefined,
        release_date: release_date || undefined,
        thumbnail_path: thumbnail_path || undefined,
        source: metadata?.source,
        executable_paths: paths
          .split('\n')
          .map((x) => x.trim())
          .filter(Boolean),
        launch_command: launch_command.trim() || undefined,
      });
      ondone(id);
    } catch (e) {
      error = userErrorMessage(e, 'ゲームを登録できませんでした。もう一度お試しください。');
    } finally {
      saving = false;
    }
  }
  async function selectExecutables() {
    const selected = await open({
      title: 'ゲームの実行ファイルを選択',
      multiple: true,
      directory: false,
      filters: [
        { name: 'ゲーム実行ファイル', extensions: ['exe', 'bin'] },
        { name: 'すべてのファイル', extensions: ['*'] },
      ],
    });
    if (selected) {
      const current = paths
        .split('\n')
        .map((x) => x.trim())
        .filter(Boolean);
      paths = [...new Set([...current, ...selected])].join('\n');
    }
  }
  async function selectThumbnail() {
    const selected = await open({
      title: 'サムネイル画像を選択',
      multiple: false,
      directory: false,
      filters: [{ name: '画像ファイル', extensions: ['jpg', 'jpeg', 'png', 'webp'] }],
    });
    if (!selected) return;
    importingThumbnail = true;
    error = '';
    try {
      thumbnail_path = await api.importThumbnail(selected);
    } catch (e) {
      error = userErrorMessage(e, 'サムネイル画像を取り込めませんでした。');
    } finally {
      importingThumbnail = false;
    }
  }
  function openCrop() {
    if (!thumbnail_path) return;
    cropOpen = true;
  }
</script>

<section class="panel form">
  <h1>ゲーム追加</h1>
  <label
    >ErogameScape / VNDB URL・ID
    <div class="row">
      <input
        bind:value={lookup}
        placeholder="サイトのURL、1234（ErogameScape）、v548（VNDB）"
      /><button
        class="metadata-lookup"
        disabled={fetchingMeta || saving || !lookup.trim()}
        onclick={fetchMeta}
        >{#if fetchingMeta}<span class="spinner" aria-hidden="true"
          ></span>取得中…{:else}取得{/if}</button
      >
    </div></label
  >
  <p class="hint">取得できない場合も下の項目から手動登録できます。</p>
  {#if metadata?.warnings.length}<p class="hint" role="status">
      {metadata.warnings.join(' ')}
    </p>{/if}
  <label>タイトル<input bind:value={title} /></label><label
    >ブランド<input bind:value={brand} /></label
  >
  <div class="form-field">
    <DateTimeSelect
      label="発売日"
      value={release_date}
      withTime={false}
      optional
      invalid={Boolean(releaseDateError)}
      onchange={(value, complete) => {
        release_date = value;
        releaseDateComplete = complete;
        releaseDateError = '';
      }}
    />
    {#if releaseDateError}<p class="form-error" role="alert">{releaseDateError}</p>{/if}
  </div>
  <div class="form-field">
    <span class="field-title">サムネイル</span>
    <div class="thumbnail-editor">
      <div class="thumbnail-preview">
        {#if thumbnail_path}<img src={imageSrc(thumbnail_path)} alt="サムネイルプレビュー" />
          <div class="thumbnail-preview-controls">
            <button
              type="button"
              class="thumbnail-preview-remove"
              aria-label="サムネイルの選択を解除"
              onclick={() => (thumbnail_path = '')}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <path d="m7 7 10 10M17 7 7 17" />
              </svg>
            </button>
            <button
              type="button"
              class="thumbnail-preview-edit"
              aria-label="サムネイルをトリミング"
              onclick={openCrop}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <path
                  d="M4 16.5V20h3.5L18.1 9.4l-3.5-3.5L4 16.5Zm16.7-9.8a1 1 0 0 0 0-1.4l-2-2a1 1 0 0 0-1.4 0l-1.6 1.6 3.5 3.5 1.5-1.7Z"
                />
              </svg>
            </button>
          </div>{:else}<span>NO IMAGE</span>{/if}
      </div>
      <div class="thumbnail-actions">
        <button
          type="button"
          disabled={fetchingMeta || importingThumbnail || saving}
          onclick={selectThumbnail}>{importingThumbnail ? '取込中…' : '画像を選択…'}</button
        >
      </div>
    </div>
  </div>
  <label
    >実行ファイル（複数選択可能）<textarea
      rows="5"
      bind:value={paths}
      placeholder="選択したexeのフルパスが表示されます"></textarea><button
      type="button"
      class="file-picker"
      onclick={selectExecutables}>参照…</button
    ></label
  ><label
    >起動コマンド<input
      bind:value={launch_command}
      placeholder="例: game.exe -w（省略時は上の実行ファイルを起動）"
    /></label
  >{#if error}<p class="error">{error}</p>{/if}
  <div class="actions">
    <button
      class="primary"
      disabled={fetchingMeta || importingThumbnail || saving || !title.trim()}
      onclick={save}>{saving ? '登録中…' : '登録'}</button
    ><button disabled={saving} onclick={oncancel}>キャンセル</button>
  </div>
</section>
{#if cropOpen && thumbnail_path}<ThumbnailCropper
    imagePath={thumbnail_path}
    ondone={(path) => {
      thumbnail_path = path;
      cropOpen = false;
    }}
    onclose={() => (cropOpen = false)}
  />{/if}
{#if toast}<div class:error-toast={toastError} class="toast" role="status">
    <span>{toastError ? '!' : '✓'}</span>
    <p>{toast}</p>
    <button aria-label="通知を閉じる" onclick={() => (toast = '')}>×</button>
  </div>{/if}
