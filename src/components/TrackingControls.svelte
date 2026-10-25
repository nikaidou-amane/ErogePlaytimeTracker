<script lang="ts">
  import SettingsIcon from './SettingsIcon.svelte';
  import { trackingStatusGroups, trackingStatusText } from '../lib/trackingStatus';
  import type { TrackingStatus } from '../lib/types';

  export let status: TrackingStatus;
  export let disabled = false;
  export let busy = false;
  export let ontoggle: () => void;
  const phaseLabels = {
    foreground: 'プレイ中',
    background: 'バックグラウンド',
    starting: '起動中',
    window_transition: '画面切替・終了中',
  };
  $: groups = trackingStatusGroups(status);
  $: summary = groups
    .map((group) =>
      group.games.map((game) => `${phaseLabels[group.phase]}：${game.title}`).join('\n'),
    )
    .join('\n');
</script>

<div class="tracking-controls">
  <div
    class="tracking-statuses"
    class:multiple={groups.length > 1}
    role="status"
    aria-label="ゲームの起動状態"
  >
    {#if groups.length === 0}<span class="tracking-status idle">● 待機中</span>{:else}
      <div class="tracking-status-groups">
        {#each groups as group}
          <span
            class="tracking-status {group.phase}"
            title={group.games.length === 1
              ? trackingStatusText(group)
              : `${trackingStatusText(group)}\n${group.games.map((game) => game.title).join('\n')}`}
            >● {phaseLabels[group.phase]} · {group.games.length === 1
              ? group.games[0].title
              : `${group.games.length}本`}</span
          >
        {/each}
      </div>
      {#if groups.length > 1}<span
          class="tracking-status tracking-compact-summary {groups[0].phase}"
          title={summary}>● {status.games.length}本起動中</span
        >{/if}
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
