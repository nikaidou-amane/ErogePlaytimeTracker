<script lang="ts">
  import SettingsIcon from './SettingsIcon.svelte';
  import {
    currentSessionSeconds,
    sessionMinutes,
    trackingStatusChips,
    trackingStatusGroups,
    trackingStatusLines,
    trackingStatusRows,
  } from '../lib/trackingStatus';
  import type { TrackingStatus } from '../lib/types';

  export let status: TrackingStatus;
  export let disabled = false;
  export let busy = false;
  export let ontoggle: () => void;
  $: groups = trackingStatusGroups(status);
  $: rows = trackingStatusRows(trackingStatusChips(groups));
  // Whole minutes straight from the backend playtime, truncated. The tray tooltip
  // shows the same granularity, so the two never disagree.
  $: sessionSeconds = currentSessionSeconds(status);
  $: sessionLine =
    sessionSeconds === null ? undefined : `● このセッション · ${sessionMinutes(sessionSeconds)}`;
  // The tooltip is derived from the rendered lines, so hover text always matches
  // what the header shows and only adds the parts hidden by the tail clipping.
  $: statusTitle = trackingStatusLines(rows, sessionLine).join('\n') || '● 待機中';
</script>

<div class="tracking-controls">
  <div class="tracking-statuses" role="status" aria-label="ゲームの起動状態" title={statusTitle}>
    {#if groups.length === 0}<span class="tracking-status idle">● 待機中</span>{:else}
      <div class="tracking-status-stack">
        {#each rows as row}
          <div class="tracking-status-row">
            {#each row as chip}
              <span class="tracking-status {chip.phase}">● {chip.label}</span>
            {/each}
          </div>
        {/each}
        {#if sessionLine !== undefined}<span class="tracking-status tracking-session-duration"
            >{sessionLine}</span
          >{/if}
      </div>
    {/if}
  </div>
  <button
    class="tracking-pause-action"
    class:paused={status.is_paused}
    disabled={disabled || busy}
    onclick={ontoggle}
    ><SettingsIcon name={status.is_paused ? 'play' : 'pause'} />{status.is_paused
      ? '計測を再開'
      : '計測を一時停止'}</button
  >
</div>
