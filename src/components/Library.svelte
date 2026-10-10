<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { userErrorMessage } from '../lib/errors';
  import { loadLibraryFilters, saveLibraryFilters } from '../lib/libraryFilters';
  import { duration, lastPlayed, imageSrc, playStatusLabel, playStatusOptions } from '../lib/time';
  import type { GameSummary } from '../lib/types';
  type ViewMode = 'grid' | 'list' | 'table';
  export let refresh = 0;
  export let openGame: (id: number) => void;
  let { search, brand, playStatus, sort, descending } = loadLibraryFilters();
  let games: GameSummary[] = [],
    error = '',
    viewMode: ViewMode = 'grid';
  let loadedKey = '';
  $: saveLibraryFilters({ search, brand, playStatus, sort, descending });
  $: totalPlaytimeSeconds = games.reduce((total, game) => total + game.total_playtime_seconds, 0);
  $: maximumPlaytimeSeconds = games.reduce(
    (maximum, game) => Math.max(maximum, game.total_playtime_seconds),
    0,
  );
  async function load() {
    try {
      games = await api.listGames(search, brand, playStatus, sort, descending);
    } catch (e) {
      error = userErrorMessage(e, 'ライブラリを読み込めませんでした。');
    }
  }
  $: {
    const key = JSON.stringify([refresh, search, brand, playStatus, sort, descending]);
    if (key !== loadedKey) {
      loadedKey = key;
      load();
    }
  }
  let brands: string[] = [];
  $: brandOptions = brand && !brands.includes(brand) ? [brand, ...brands] : brands;
  onMount(() => {
    const saved = localStorage.getItem('library-view-mode');
    if (saved === 'grid' || saved === 'list' || saved === 'table') viewMode = saved;
    api
      .listBrands()
      .then((value) => (brands = value))
      .catch((e) => (error = userErrorMessage(e, 'ブランド一覧を読み込めませんでした。')));
    const timer = setInterval(load, 1000);
    return () => clearInterval(timer);
  });
  function setViewMode(mode: ViewMode) {
    viewMode = mode;
    localStorage.setItem('library-view-mode', mode);
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
  ><label
    ><span>ブランド</span><select bind:value={brand}
      ><option value="">すべてのブランド</option>{#each brandOptions as b}<option>{b}</option
        >{/each}</select
    ></label
  ><label
    ><span>プレイ状況</span><select bind:value={playStatus}
      ><option value="">すべての状況</option>{#each playStatusOptions as option}<option
          value={option.value}>{option.label}</option
        >{/each}</select
    ></label
  ><label
    ><span>並び順</span><select bind:value={sort}
      ><option value="last_played">最終プレイ</option><option value="total_playtime"
        >プレイ時間</option
      ><option value="title">タイトル</option><option value="brand">ブランド</option><option
        value="release_date">発売日</option
      ><option value="created_at">登録日</option><option value="session_count">セッション数</option
      ></select
    ></label
  ><button onclick={() => (descending = !descending)}>{descending ? '降順' : '昇順'}</button>
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
