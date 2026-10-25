<script lang="ts">
  import { onMount, tick } from 'svelte';
  import DateTimeSelect from './DateTimeSelect.svelte';
  import SettingsIcon from './SettingsIcon.svelte';
  import { api } from '../lib/api';
  import { userErrorMessage } from '../lib/errors';
  import { local, inputTime, utc } from '../lib/time';
  import type { TrackingPauseInterval, TrackingStatus } from '../lib/types';
  export let status: TrackingStatus;
  export let onchange: (status: TrackingStatus) => void;
  export let onclose: () => void;
  let dialog: HTMLDialogElement;
  let heading: HTMLHeadingElement;
  let returnFocus: HTMLElement | null = null;
  let intervals: TrackingPauseInterval[] = [];
  let loading = false,
    busy = false,
    more = false,
    reloadRequested = false;
  let error = '',
    message = '';
  let view: 'list' | 'edit' | 'delete' = 'list';
  let selected: TrackingPauseInterval | null = null;
  let start = '',
    end = '',
    closeInterval = false,
    startComplete = true,
    endComplete = true;
  let loadedPause: string | null | undefined = undefined;
  let now = Date.now();
  $: if (loadedPause !== status.pause_started_at) {
    loadedPause = status.pause_started_at;
    void load();
  }
  function dateKey(value: string) {
    const date = new Date(value);
    return `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`;
  }
  function dateLabel(value: string, currentTime: number) {
    const yesterday = new Date(currentTime);
    yesterday.setDate(yesterday.getDate() - 1);
    if (dateKey(value) === dateKey(new Date(currentTime).toISOString())) return '今日';
    if (dateKey(value) === dateKey(yesterday.toISOString())) return '昨日';
    return new Date(value).toLocaleDateString('ja-JP', {
      year: 'numeric',
      month: 'long',
      day: 'numeric',
      weekday: 'short',
    });
  }
  function time(value: string) {
    const date = new Date(value);
    return [date.getHours(), date.getMinutes(), date.getSeconds()]
      .map((part) => String(part).padStart(2, '0'))
      .join(':');
  }
  function elapsed(from: string, to: string | null, currentTime: number) {
    const seconds = Math.max(
      0,
      Math.floor(((to ? new Date(to).getTime() : currentTime) - new Date(from).getTime()) / 1000),
    );
    if (!Number.isFinite(seconds)) return '—';
    return [Math.floor(seconds / 3600), Math.floor((seconds % 3600) / 60), seconds % 60]
      .map((value) => String(value).padStart(2, '0'))
      .join(':');
  }
  $: groups = [...intervals]
    .sort((a, b) => b.started_at.localeCompare(a.started_at))
    .reduce<Array<{ key: string; date: string; rows: TrackingPauseInterval[] }>>(
      (result, interval) => {
        const key = dateKey(interval.started_at);
        if (result[result.length - 1]?.key !== key)
          result.push({ key, date: interval.started_at, rows: [] });
        result[result.length - 1].rows.push(interval);
        return result;
      },
      [],
    );
  async function load(append = false) {
    if (loading) {
      reloadRequested = true;
      return;
    }
    loading = true;
    try {
      const page = await api.pauseIntervals(
        append ? (intervals[intervals.length - 1]?.id ?? null) : null,
      );
      intervals = append ? [...intervals, ...page] : page;
      more = page.length === 50;
    } catch (cause) {
      error = userErrorMessage(cause, '履歴を読み込めませんでした。');
    } finally {
      loading = false;
      if (reloadRequested) {
        reloadRequested = false;
        void load();
      }
    }
  }
  async function openDetail(interval: TrackingPauseInterval, next: 'edit' | 'delete') {
    returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    selected = interval;
    start = inputTime(interval.started_at);
    end = inputTime(interval.ended_at) || inputTime(new Date().toISOString());
    closeInterval = interval.ended_at !== null;
    startComplete = endComplete = true;
    error = message = '';
    view = next;
    await tick();
    heading.focus();
  }
  async function back() {
    if (busy) return;
    view = 'list';
    selected = null;
    error = '';
    await tick();
    if (returnFocus?.isConnected) returnFocus.focus();
    else heading.focus();
  }
  async function refresh() {
    onchange(await api.status());
    await load();
  }
  async function save() {
    if (!selected || busy) return;
    if (!startComplete || (closeInterval && !endComplete) || !start || (closeInterval && !end)) {
      error = '日時を最後まで入力してください。';
      return;
    }
    busy = true;
    error = '';
    try {
      await api.updatePauseInterval(
        selected.id,
        utc(start),
        closeInterval ? utc(end) : null,
        selected.updated_at,
      );
      message = '保存しました';
      await refresh();
      busy = false;
      await back();
    } catch (cause) {
      error = userErrorMessage(cause);
    } finally {
      busy = false;
    }
  }
  async function remove() {
    if (!selected || busy) return;
    busy = true;
    error = '';
    try {
      await api.deletePauseInterval(selected.id, selected.updated_at);
      message = selected.ended_at ? '削除しました' : '取り消して再開しました';
      await refresh();
      busy = false;
      await back();
    } catch (cause) {
      error = userErrorMessage(cause);
    } finally {
      busy = false;
    }
  }
  function closeDialog() {
    if (busy) return;
    dialog.close();
    onclose();
  }
  function cancel(event: Event) {
    event.preventDefault();
    if (busy) return;
    if (view !== 'list') void back();
    else closeDialog();
  }
  onMount(() => {
    dialog.showModal();
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });
</script>

<dialog
  bind:this={dialog}
  class="pause-dialog"
  aria-labelledby="pause-dialog-title"
  oncancel={cancel}
>
  <div class="pause-dialog-heading">
    {#if view !== 'list'}<button
        class="settings-icon-button"
        aria-label="履歴に戻る"
        disabled={busy}
        onclick={back}><SettingsIcon name="back" /></button
      >{:else}<span class="settings-section-icon"><SettingsIcon name="history" /></span>{/if}
    <div>
      <h2 bind:this={heading} tabindex="-1" id="pause-dialog-title">
        {view === 'list'
          ? '一時停止の履歴'
          : view === 'edit'
            ? '区間を編集'
            : selected?.ended_at
              ? '区間を削除'
              : '一時停止を取り消す'}
      </h2>
      <span class="settings-scope">全ゲーム共通</span>
    </div>
    <button
      class="settings-icon-button pause-dialog-close"
      aria-label="履歴を閉じる"
      disabled={busy}
      onclick={closeDialog}><SettingsIcon name="close" /></button
    >
  </div>
  <div class="pause-dialog-body">
    {#if view === 'list'}
      <div class="pause-list-toolbar">
        <span>開始 → 終了</span><span class="pause-list-duration">停止時間</span><button
          class="settings-icon-button"
          aria-label="履歴を再読み込み"
          disabled={loading || busy}
          onclick={() => {
            error = '';
            void load();
          }}><SettingsIcon name="refresh" /></button
        >
      </div>
      {#if message}<p class="settings-success" role="status">
          <SettingsIcon name="check" />{message}
        </p>{/if}
      {#if !intervals.length}<div class="pause-empty">
          <SettingsIcon name="history" />
          <p>{loading ? '読み込み中…' : '履歴はまだありません'}</p>
        </div>{/if}
      {#each groups as group (group.key)}
        <section class="pause-day">
          <h3>{dateLabel(group.date, now)}</h3>
          {#each group.rows as interval (interval.id)}
            <div class="pause-history-row" class:is-active={!interval.ended_at}>
              <button
                class="pause-history-edit"
                aria-label={`${local(interval.started_at)}の区間を編集`}
                disabled={busy}
                onclick={() => openDetail(interval, 'edit')}
              >
                <span class="pause-time-range"
                  ><time datetime={interval.started_at} title={local(interval.started_at)}
                    >{time(interval.started_at)}</time
                  ><span class="pause-time-arrow">→</span>{#if interval.ended_at}<time
                      datetime={interval.ended_at}
                      title={local(interval.ended_at)}
                      >{#if dateKey(interval.started_at) !== dateKey(interval.ended_at)}<small
                          >{new Date(interval.ended_at).toLocaleDateString('ja-JP', {
                            month: 'numeric',
                            day: 'numeric',
                          })}</small
                        >{/if}{time(interval.ended_at)}</time
                    >{:else}<span class="pause-live-tag">継続中</span>{/if}</span
                >
                <span class="pause-time-duration"
                  >{elapsed(interval.started_at, interval.ended_at, now)}</span
                ><SettingsIcon name="edit" />
              </button>
              <button
                class="settings-icon-button pause-delete-button"
                aria-label={`${local(interval.started_at)}の区間を${interval.ended_at ? '削除' : '取り消し'}`}
                title={interval.ended_at ? '削除' : '取り消し'}
                disabled={busy}
                onclick={() => openDetail(interval, 'delete')}><SettingsIcon name="trash" /></button
              >
            </div>
          {/each}
        </section>
      {/each}
      {#if more}<button
          class="pause-load-more"
          disabled={loading || busy}
          onclick={() => load(true)}>{loading ? '読み込み中…' : '以前の履歴'}</button
        >{/if}
    {:else if selected}
      <div class="pause-duration-preview">
        <SettingsIcon name="clock" /><strong
          >{view === 'edit'
            ? elapsed(start, closeInterval ? end : null, now)
            : elapsed(selected.started_at, selected.ended_at, now)}</strong
        ><span>停止時間</span>
      </div>
      {#if view === 'edit'}
        <div class="pause-edit-fields">
          <DateTimeSelect
            label="開始"
            value={start}
            withTime
            disabled={busy}
            onchange={(value, complete) => {
              start = value;
              startComplete = complete;
            }}
          />
          {#if !selected.ended_at}<div class="pause-end-mode" role="group" aria-label="区間の終了">
              <button
                aria-pressed={!closeInterval}
                disabled={busy}
                onclick={() => (closeInterval = false)}>継続中</button
              ><button
                aria-pressed={closeInterval}
                disabled={busy}
                onclick={() => (closeInterval = true)}>終了する</button
              >
            </div>{/if}
          {#if closeInterval}<DateTimeSelect
              label="終了"
              value={end}
              withTime
              disabled={busy}
              onchange={(value, complete) => {
                end = value;
                endComplete = complete;
              }}
            />{/if}
        </div>
      {:else}<p class="pause-delete-range">
          {local(selected.started_at)}<br /><span>↓</span><br />{selected.ended_at
            ? local(selected.ended_at)
            : '継続中'}
        </p>{/if}
    {/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </div>
  <div class="pause-dialog-footer">
    <small>全ゲームのプレイ時間に反映</small>{#if view !== 'list'}<div class="actions">
        <button disabled={busy} onclick={back}>キャンセル</button><button
          class:danger={view === 'delete'}
          class:primary={view === 'edit'}
          disabled={busy}
          onclick={view === 'edit' ? save : remove}
          >{busy
            ? '処理中…'
            : view === 'edit'
              ? !selected?.ended_at && closeInterval
                ? '保存して再開'
                : '保存'
              : selected?.ended_at
                ? '削除'
                : '取り消して再開'}</button
        >
      </div>{:else}<button onclick={closeDialog}>閉じる</button>{/if}
  </div>
</dialog>
