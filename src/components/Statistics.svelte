<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { userErrorMessage } from '../lib/errors';
  import { compactDuration, formatDateKey } from '../lib/statistics';
  import { imageSrc } from '../lib/time';
  import type { StatisticsPeriodInput, StatisticsPeriodKind, StatisticsReport } from '../lib/types';
  import PlaytimeTrend from './PlaytimeTrend.svelte';

  export let openGame: (id: number) => void;
  export let trackingActive = false;
  export let calculationKey = '';

  const today = new Date();
  const currentYear = today.getFullYear();
  const currentMonth = today.getMonth() + 1;
  let kind: StatisticsPeriodKind = 'month';
  let selectedYear = currentYear;
  let selectedMonth = currentMonth;
  let report: StatisticsReport | null = null;
  let error = '';
  let loading = true;
  let requestSequence = 0;
  let loadedKey = '';
  let availableYears = [currentYear];

  $: maximumPlaytimeSeconds = (report?.games ?? []).reduce(
    (maximum, game) => Math.max(maximum, game.playtime_seconds),
    0,
  );
  $: request = periodInput(kind, selectedYear, selectedMonth);
  $: requestKey = `${request.kind}|${request.year ?? ''}|${request.month ?? ''}|${calculationKey}`;
  $: if (requestKey !== loadedKey) {
    loadedKey = requestKey;
    report = null;
    load(request);
  }
  $: minimumYear = Math.min(...availableYears);
  $: canMoveBackward =
    kind === 'month'
      ? selectedYear > minimumYear || selectedMonth > 1
      : kind === 'year'
        ? selectedYear > minimumYear
        : false;
  $: canMoveForward =
    kind === 'month'
      ? selectedYear < currentYear || selectedMonth < currentMonth
      : kind === 'year'
        ? selectedYear < currentYear
        : false;

  function periodInput(
    periodKind: StatisticsPeriodKind,
    year: number,
    month: number,
  ): StatisticsPeriodInput {
    if (periodKind === 'month') return { kind: periodKind, year, month };
    if (periodKind === 'year') return { kind: periodKind, year };
    return { kind: periodKind };
  }

  async function load(period = request) {
    const sequence = ++requestSequence;
    loading = true;
    error = '';
    try {
      const next = await api.statistics(period);
      if (sequence !== requestSequence) return;
      report = next;
      availableYears = next.available_years.length ? next.available_years : [currentYear];
    } catch (cause) {
      if (sequence === requestSequence) {
        error = userErrorMessage(cause, '統計情報を読み込めませんでした。');
      }
    } finally {
      if (sequence === requestSequence) loading = false;
    }
  }

  function selectKind(next: StatisticsPeriodKind) {
    kind = next;
  }

  function movePeriod(direction: -1 | 1) {
    if (kind === 'month') {
      const next = new Date(selectedYear, selectedMonth - 1 + direction, 1);
      selectedYear = next.getFullYear();
      selectedMonth = next.getMonth() + 1;
    } else if (kind === 'year') {
      selectedYear += direction;
    }
  }

  function changeYear(value: number) {
    selectedYear = value;
    if (selectedYear === currentYear && selectedMonth > currentMonth) {
      selectedMonth = currentMonth;
    }
  }

  function sessionDate(value: string) {
    return new Intl.DateTimeFormat('ja-JP', {
      year: 'numeric',
      month: 'long',
      day: 'numeric',
    }).format(new Date(value));
  }

  onMount(() => {
    const timer = setInterval(() => {
      if (trackingActive) load();
    }, 30_000);
    return () => clearInterval(timer);
  });
</script>

<section class="statistics-page">
  <div class="statistics-page-heading">
    <h1>統計</h1>
  </div>

  <section class="statistics-period-panel" aria-label="統計期間">
    <div class="statistics-period-controls">
      <div class="statistics-period-kind">
        <button
          class:active={kind === 'month'}
          aria-pressed={kind === 'month'}
          onclick={() => selectKind('month')}>月</button
        ><button
          class:active={kind === 'year'}
          aria-pressed={kind === 'year'}
          onclick={() => selectKind('year')}>年</button
        ><button
          class:active={kind === 'all'}
          aria-pressed={kind === 'all'}
          onclick={() => selectKind('all')}>全期間</button
        >
      </div>
      {#if kind !== 'all'}
        <div class="statistics-period-navigation">
          <button aria-label="前の期間" disabled={!canMoveBackward} onclick={() => movePeriod(-1)}
            >‹</button
          >
          <div class="statistics-period-selects">
            <select
              aria-label="年"
              value={selectedYear}
              onchange={(event) =>
                changeYear(Number((event.currentTarget as HTMLSelectElement).value))}
            >
              {#each availableYears as year}<option value={year}>{year}年</option>{/each}
            </select>
            {#if kind === 'month'}
              <select aria-label="月" bind:value={selectedMonth}>
                {#each Array.from({ length: 12 }, (_, index) => index + 1) as month}
                  <option
                    value={month}
                    disabled={selectedYear === currentYear && month > currentMonth}
                    >{month}月</option
                  >
                {/each}
              </select>
            {/if}
          </div>
          <button aria-label="次の期間" disabled={!canMoveForward} onclick={() => movePeriod(1)}
            >›</button
          >
        </div>
      {:else}
        <strong class="statistics-all-period">最初の記録から現在まで</strong>
      {/if}
      {#if report}
        <span class="statistics-period-range"
          >{formatDateKey(report.period.start_date)}〜{formatDateKey(report.period.end_date)}</span
        >
      {/if}
      <button class="statistics-refresh" disabled={loading} onclick={() => load()}
        >{loading ? '集計中…' : '再集計'}</button
      >
    </div>
  </section>

  {#if error}<p class="error statistics-error">{error}</p>{/if}
  {#if report}
    <div class:loading class="statistics-content">
      <section class="statistics-data-section statistics-trend-panel">
        <div class="statistics-section-heading">
          <div>
            <h2>プレイ時間の推移</h2>
          </div>
        </div>
        <PlaytimeTrend days={report.days} {kind} />
      </section>

      <section class="statistics-data-section statistics-ranking-panel">
        <div class="statistics-section-heading">
          <div>
            <h2>この期間にプレイしたゲーム</h2>
          </div>
          <span>{report.games.length}本</span>
        </div>
        {#if report.games.length}
          <div class="statistics-ranking">
            {#each report.games as game, index}
              {@const share = report.summary.total_playtime_seconds
                ? (game.playtime_seconds / report.summary.total_playtime_seconds) * 100
                : 0}
              {@const playtimeShare = maximumPlaytimeSeconds
                ? (game.playtime_seconds / maximumPlaytimeSeconds) * 100
                : 0}
              <button
                class="statistics-game-row"
                style={`--game-share: ${playtimeShare}%`}
                onclick={() => openGame(game.game_id)}
                aria-label={`${game.title}の詳細を開く`}
              >
                <span class="statistics-game-rank">{index + 1}</span>
                <span class="statistics-game-image">
                  {#if game.thumbnail_path}<img
                      src={imageSrc(game.thumbnail_path)}
                      alt=""
                    />{:else}<span>NO IMAGE</span>{/if}
                </span>
                <span class="statistics-game-info">
                  <strong>{game.title}</strong><small>{game.brand ?? 'ブランド未設定'}</small>
                </span>
                <span class="statistics-game-meta">
                  <strong>{compactDuration(game.playtime_seconds)}</strong><small
                    >{share.toFixed(1)}% ・ {game.session_count}回 ・ {game.active_day_count}日</small
                  >
                </span>
              </button>
            {/each}
          </div>
        {:else}
          <div class="statistics-empty">この期間にはプレイ記録がありません。</div>
        {/if}
      </section>

      <section
        class="statistics-data-section statistics-summary"
        aria-labelledby="statistics-summary-heading"
      >
        <h2 id="statistics-summary-heading">期間の概要</h2>
        <dl class="statistics-summary-list">
          <div>
            <dt>累計プレイ時間</dt>
            <dd>
              <strong>{compactDuration(report.summary.total_playtime_seconds)}</strong>
              <small>{report.summary.game_count}本のゲーム</small>
            </dd>
          </div>
          <div>
            <dt>日平均</dt>
            <dd><strong>{compactDuration(report.summary.average_per_day_seconds)}</strong></dd>
          </div>
          <div>
            <dt>プレイ日数</dt>
            <dd><strong>{report.summary.active_day_count}日</strong></dd>
          </div>
          <div>
            <dt>期間内最長セッション</dt>
            <dd>
              <strong
                >{compactDuration(report.summary.longest_session?.playtime_seconds ?? 0)}</strong
              >
              {#if report.summary.longest_session}
                <small
                  >{report.summary.longest_session.title} ・
                  {sessionDate(report.summary.longest_session.launched_at)}</small
                >
              {:else}
                <small>記録なし</small>
              {/if}
            </dd>
          </div>
          <div>
            <dt>最もプレイした日</dt>
            <dd>
              <strong
                >{report.summary.busiest_day
                  ? formatDateKey(report.summary.busiest_day.date)
                  : '記録なし'}</strong
              >
              {#if report.summary.busiest_day}<small
                  >{compactDuration(report.summary.busiest_day.playtime_seconds)}</small
                >{/if}
            </dd>
          </div>
          <div>
            <dt>セッション数</dt>
            <dd><strong>{report.summary.session_count}回</strong></dd>
          </div>
          <div>
            <dt>平均セッション時間</dt>
            <dd><strong>{compactDuration(report.summary.average_session_seconds)}</strong></dd>
          </div>
        </dl>
        {#if report.summary.needs_review_session_count > 0}
          <p class="statistics-review-note">
            要確認のセッション {report.summary.needs_review_session_count}件を集計に含んでいます。
          </p>
        {/if}
      </section>

      <p class="statistics-generated-at">
        集計日時: {new Date(report.period.generated_at).toLocaleString()}
      </p>
    </div>
  {:else if loading}
    <div class="statistics-loading">統計情報を集計しています…</div>
  {/if}
</section>
