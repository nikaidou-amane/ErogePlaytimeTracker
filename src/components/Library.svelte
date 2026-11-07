<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { userErrorMessage } from '../lib/errors';
  import {
    defaultLibraryView,
    filterSummary,
    normalizeLibraryView,
    toggleValue,
  } from '../lib/libraryFilters';
  import { duration, lastPlayed, imageSrc, playStatusLabel, playStatusOptions } from '../lib/time';
  import type {
    GameSummary,
    LibraryView,
    LibraryViewMode,
    PlayStatus,
    SortKey,
  } from '../lib/types';
  export let refresh = 0;
  export let openGame: (id: number) => void;
  let view: LibraryView = { ...defaultLibraryView };
  let search = '';
  let brands: string[] = [];
  let games: GameSummary[] = [],
    error = '',
    loaded = false;
  let loadedKey = '';
  let brandMenuOpen = false;
  let statusMenuOpen = false;
  $: viewMode = view.viewMode;
  $: totalPlaytimeSeconds = games.reduce((total, game) => total + game.total_playtime_seconds, 0);
  $: maximumPlaytimeSeconds = games.reduce(
    (maximum, game) => Math.max(maximum, game.total_playtime_seconds),
    0,
  );
  async function load() {
    try {
      games = await api.listGames(
        search,
        view.brands,
        view.playStatuses,
        view.sort,
        view.descending,
      );
    } catch (e) {
      error = userErrorMessage(e, 'ライブラリを読み込めませんでした。');
    }
  }
  $: {
    const key = JSON.stringify([
      refresh,
      search,
      view.brands,
      view.playStatuses,
      view.sort,
      view.descending,
    ]);
    if (loaded && key !== loadedKey) {
      loadedKey = key;
      load();
    }
  }
  $: if (loaded && view) persist();
  let savedJson = '';
  function persist() {
    const next = JSON.stringify(view);
    if (next === savedJson) return;
    savedJson = next;
    api.setLibraryView(view).catch((e) => {
      error = userErrorMessage(e, '表示設定を保存できませんでした。');
    });
  }
  $: brandOptions = brands;
  onMount(() => {
    api
      .libraryView()
      .then((saved) => {
        view = normalizeLibraryView(saved);
        savedJson = JSON.stringify(view);
      })
      .catch((e) => {
        error = userErrorMessage(e, '表示設定を読み込めませんでした。');
      })
      .then(() => {
        loaded = true;
        loadedKey = '';
      });
    api
      .listBrands()
      .then((value) => (brands = value))
      .catch((e) => (error = userErrorMessage(e, 'ブランド一覧を読み込めませんでした。')));
    const timer = setInterval(load, 1000);
    // メニューの外側を押したら閉じる。toolbar 全体ではなく、ブランド・
    // プレイ状況の各コントロールの外かどうかで判定する。
    // （toolbar 内の並び順・検索を押したときにも閉じるようにするため）
    function handleDocumentClick(event: MouseEvent) {
      if (!(event.target instanceof Element)) return;
      if (!event.target.closest('.filter-control')) {
        brandMenuOpen = false;
        statusMenuOpen = false;
      }
    }
    document.addEventListener('click', handleDocumentClick);
    return () => {
      clearInterval(timer);
      document.removeEventListener('click', handleDocumentClick);
    };
  });
  function setViewMode(mode: LibraryViewMode) {
    view = { ...view, viewMode: mode };
  }
  function selectSort(value: SortKey) {
    view = { ...view, sort: value };
  }
  async function launch(gameId: number) {
    try {
      await api.launchGame(gameId);
    } catch (e) {
      error = userErrorMessage(e, 'ゲームを起動できませんでした。');
    }
  }
  function openTableRow(event: KeyboardEvent, gameId: number) {
    if (event.target !== event.currentTarget || (event.key !== 'Enter' && event.key !== ' '))
      return;
    event.preventDefault();
    openGame(gameId);
  }
</script>

<section class="toolbar">
  <label class="search-control"
    ><span>タイトル検索</span><input
      placeholder="ゲームタイトルを入力"
      bind:value={search}
    /></label
  >
  <div class="filter-control" class:open={brandMenuOpen}>
    <span>ブランド</span>
    <button
      type="button"
      class="filter-trigger"
      aria-expanded={brandMenuOpen}
      onclick={() => {
        brandMenuOpen = !brandMenuOpen;
        statusMenuOpen = false;
      }}>{filterSummary(view.brands, 'すべてのブランド')}</button
    >
    {#if brandMenuOpen}
      <div class="filter-menu">
        <button
          type="button"
          class="filter-all"
          onclick={() => {
            view = { ...view, brands: [] };
            brandMenuOpen = false;
          }}>すべてのブランド</button
        >
        {#each brandOptions as b}
          <label class="filter-option"
            ><input
              type="checkbox"
              checked={view.brands.includes(b)}
              onchange={() => (view = { ...view, brands: toggleValue(view.brands, b) })}
            /><span>{b}</span></label
          >
        {/each}
      </div>
    {/if}
  </div>
  <div class="filter-control" class:open={statusMenuOpen}>
    <span>プレイ状況</span>
    <button
      type="button"
      class="filter-trigger"
      aria-expanded={statusMenuOpen}
      onclick={() => {
        statusMenuOpen = !statusMenuOpen;
        brandMenuOpen = false;
      }}
      >{filterSummary(
        view.playStatuses.map((status) => playStatusLabel(status)),
        'すべての状況',
      )}</button
    >
    {#if statusMenuOpen}
      <div class="filter-menu">
        <button
          type="button"
          class="filter-all"
          onclick={() => {
            view = { ...view, playStatuses: [] };
            statusMenuOpen = false;
          }}>すべての状況</button
        >
        {#each playStatusOptions as option}
          <label class="filter-option"
            ><input
              type="checkbox"
              checked={view.playStatuses.includes(option.value)}
              onchange={() =>
                (view = {
                  ...view,
                  playStatuses: toggleValue(view.playStatuses, option.value),
                })}
            /><span>{option.label}</span></label
          >
        {/each}
      </div>
    {/if}
  </div>
  <label
    ><span>並び順</span><select
      value={view.sort}
      onchange={(event) => selectSort((event.currentTarget as HTMLSelectElement).value as SortKey)}
      ><option value="last_played">最終プレイ</option><option value="total_playtime"
        >プレイ時間</option
      ><option value="title">タイトル</option><option value="brand">ブランド</option><option
        value="release_date">発売日</option
      ><option value="created_at">登録日</option><option value="session_count">セッション数</option
      ></select
    ></label
  ><button onclick={() => (view = { ...view, descending: !view.descending })}
    >{view.descending ? '降順' : '昇順'}</button
  >
  <div class="view-control">
    <span>表示形式</span>
    <div class="view-switch" aria-label="ライブラリの表示形式">
      <button
        class:active={viewMode === 'grid'}
        aria-pressed={viewMode === 'grid'}
        title="グリッド表示"
        onclick={() => setViewMode('grid')}>▦<span>グリッド</span></button
      ><button
        class:active={viewMode === 'list'}
        aria-pressed={viewMode === 'list'}
        title="リスト表示"
        onclick={() => setViewMode('list')}>☷<span>リスト</span></button
      ><button
        class:active={viewMode === 'table'}
        aria-pressed={viewMode === 'table'}
        title="テーブル表示"
        onclick={() => setViewMode('table')}>▤<span>テーブル</span></button
      >
    </div>
  </div>
</section>
<section class="library-summary" aria-live="polite">
  <span>表示中 <strong>{games.length}</strong> 本</span>
  <span>合計プレイ時間 <strong>{duration(totalPlaytimeSeconds)}</strong></span>
</section>
{#if error}<p class="error">{error}</p>{/if}{#if !games.length}<div class="empty">
    ゲームがありません。「ゲーム追加」から登録してください。
  </div>{/if}
{#if viewMode === 'grid'}
  <div class="grid">
    {#each games as g}<article class="card">
        <button
          class="card-main"
          onclick={() => openGame(g.id)}
          aria-label={`${g.title}の詳細を開く`}
          ><div class="card-image">
            {#if g.thumbnail_path}<img src={imageSrc(g.thumbnail_path)} alt="" />{:else}<div
                class="placeholder"
              >
                NO IMAGE
              </div>{/if}
          </div>
          <div class="card-info">
            <h2>{g.title}</h2>
            <p>{g.brand ?? 'ブランド未設定'}</p>
            <span class="play-status status-{g.play_status}">{playStatusLabel(g.play_status)}</span>
            <strong>{duration(g.total_playtime_seconds)}</strong><small
              >最終: {lastPlayed(g.last_played)} ・ {g.session_count}回</small
            >
          </div></button
        ><button class="launch-overlay" onclick={() => launch(g.id)} aria-label={`${g.title}を起動`}
          >▶ 起動</button
        >
      </article>{/each}
  </div>
{:else if viewMode === 'list'}
  <div class="library-list">
    {#each games as g}<article class="library-row">
        <button
          class="library-row-main"
          onclick={() => openGame(g.id)}
          aria-label={`${g.title}の詳細を開く`}
          ><div class="library-row-image">
            {#if g.thumbnail_path}<img src={imageSrc(g.thumbnail_path)} alt="" />{:else}<div
                class="placeholder"
              >
                NO IMAGE
              </div>{/if}
          </div>
          <div class="library-row-info">
            <h2>{g.title}</h2>
            <p>
              {g.brand ?? 'ブランド未設定'}{#if g.release_date}<span>
                  ・ {g.release_date}</span
                >{/if}
            </p>
            <span class="play-status status-{g.play_status}">{playStatusLabel(g.play_status)}</span>
          </div>
          <div class="library-row-stats">
            <strong>{duration(g.total_playtime_seconds)}</strong><small
              >最終: {lastPlayed(g.last_played)}</small
            ><small>{g.session_count} セッション</small>
          </div></button
        ><button
          class="library-row-launch"
          onclick={() => launch(g.id)}
          aria-label={`${g.title}を起動`}>▶ 起動</button
        >
      </article>{/each}
  </div>
{:else}
  <div class="library-table-scroll">
    <table class="library-table">
      <colgroup>
        <col />
        <col />
        <col />
        <col class="library-table-playtime-column" />
        <col />
        <col />
        <col />
      </colgroup>
      <thead>
        <tr>
          <th scope="col">ゲーム</th>
          <th scope="col">ブランド</th>
          <th scope="col">状態</th>
          <th class="library-table-number" scope="col">プレイ時間</th>
          <th scope="col">最終プレイ</th>
          <th scope="col">セッション</th>
          <th scope="col">発売日</th>
        </tr>
      </thead>
      <tbody>
        {#each games as g}
          {@const playtimeShare = maximumPlaytimeSeconds
            ? (g.total_playtime_seconds / maximumPlaytimeSeconds) * 100
            : 0}
          <tr
            class="library-table-row"
            role="link"
            tabindex="0"
            aria-label={`${g.title}の詳細を開く`}
            onclick={() => openGame(g.id)}
            onkeydown={(event) => openTableRow(event, g.id)}
          >
            <td class="library-table-game-cell">
              <span class="library-table-title library-table-game-detail"
                ><span>{g.title}</span></span
              >
              <button
                class="library-table-launch"
                onclick={(event) => {
                  event.stopPropagation();
                  launch(g.id);
                }}
                aria-label={`${g.title}を起動`}
              >
                <span class="library-table-thumbnail">
                  {#if g.thumbnail_path}<img src={imageSrc(g.thumbnail_path)} alt="" />{:else}<span
                      >NO IMAGE</span
                    >{/if}
                </span>
                <span class="library-table-launch-icon" aria-hidden="true">▶</span>
              </button>
            </td>
            <td class="library-table-muted">{g.brand ?? 'ブランド未設定'}</td>
            <td
              ><span class="play-status status-{g.play_status}"
                >{playStatusLabel(g.play_status)}</span
              ></td
            >
            <td class="library-table-number">
              <div class="library-table-playtime">
                <strong title={duration(g.total_playtime_seconds)}
                  >{duration(g.total_playtime_seconds)}</strong
                >
                <span class="library-table-playtime-bar" aria-hidden="true"
                  ><i style={`width: ${playtimeShare}%`}></i></span
                >
              </div>
            </td>
            <td class="library-table-muted">{lastPlayed(g.last_played)}</td>
            <td class="library-table-session-count">{g.session_count}回</td>
            <td class="library-table-muted">{g.release_date ?? '—'}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}
