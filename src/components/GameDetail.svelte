<script lang="ts">
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { open } from '@tauri-apps/plugin-dialog';
  import DateTimeSelect from './DateTimeSelect.svelte';
  import DeleteButton from './DeleteButton.svelte';
  import ThumbnailCropEditor from './ThumbnailCropEditor.svelte';
  import HistoryDataRow from './HistoryDataRow.svelte';
  import PlaytimeTrend from './PlaytimeTrend.svelte';
  import { api } from '../lib/api';
  import { metadataSourceLabel } from '../lib/metadata';
  import { backgroundIntervalDescription } from '../lib/playtime';
  import { userErrorMessage } from '../lib/errors';
  import { formatDateKey } from '../lib/statistics';
  import { screenshotNavigation } from '../lib/screenshotNavigation';
  import {
    validateBackgroundInterval,
    validateManualSession,
    validateRunningSessionEdit,
    validateSessionEdit,
  } from '../lib/historyValidation';
  import {
    duration,
    local,
    inputTime,
    utc,
    imageSrc,
    playStatusLabel,
    playStatusOptions,
  } from '../lib/time';
  import type {
    BackgroundInterval,
    GameDetail,
    GameTimestamp,
    GameScreenshot,
    PlayStatus,
    ScreenshotOcrRegion,
    Session,
    StatisticsReport,
    TrackingStatus,
  } from '../lib/types';
  export let gameId: number;
  export let excludeBackgroundTime = true;
  export let onback: () => void;
  let game: GameDetail | null = null,
    sessions: Session[] = [],
    timestamps: GameTimestamp[] = [],
    screenshots: GameScreenshot[] = [],
    gameStatistics: StatisticsReport | null = null,
    selectedScreenshot: GameScreenshot | null = null,
    selected: Session | null = null,
    intervals: BackgroundInterval[] = [],
    pageError = '',
    gameStatisticsError = '',
    gameStatisticsLoading = true,
    newPath = '',
    manualSessionOpen = false,
    manualStart = '',
    manualEnd = '',
    manualSessionError = '',
    manualSessionSaving = false,
    timestampName = '',
    nowMs = Date.now(),
    refreshingMeta = false,
    creatingTimestamp = false,
    toast = '',
    toastError = false,
    toastTimer: number | undefined,
    editingGame = false,
    editTitle = '',
    editBrand = '',
    editReleaseDate = '',
    editReleaseDateComplete = true,
    editReleaseDateError = '',
    editSourceUrl = '',
    sessionEditOpen = false,
    sessionStart = '',
    sessionEnd = '',
    sessionFormDirty = false,
    sessionFormError = '',
    sessionActionError = '',
    intervalListError = '',
    sessionSaving = false,
    sessionReviewSaving = false,
    newIntervalOpen = false,
    newIntervalStart = '',
    newIntervalEnd = '',
    newIntervalError = '',
    intervalCreating = false,
    editingIntervalId: number | null = null,
    intervalEditStart = '',
    intervalEditEnd = '',
    intervalEditError = '',
    savingIntervalId: number | null = null;
  let launchCommandDraft = '',
    launchCommandSaving = false,
    launchCommandEditing = false;
  let gameStatisticsRequestSequence = 0;
  let trackingStateKey = '';
  let thumbnailEditorOpen = false,
    thumbnailDraftPath: string | null = null,
    thumbnailOriginalPath: string | null = null,
    thumbnailImporting = false,
    thumbnailCropBusy = false,
    thumbnailSaving = false,
    thumbnailError = '';
  let editingTimestampId: number | null = null,
    savingTimestampId: number | null = null,
    editingTimestampName = '',
    editingTimestampTime = '',
    editingTimestampTimeComplete = true,
    timestampEditError = '';
  let screenshotOcrAttempted = false,
    screenshotOcrLoading = false,
    screenshotOcrSearching = false,
    screenshotOcrText = '',
    screenshotOcrError = '',
    screenshotOcrRequestId = 0;
  let screenshotOcrRegion: ScreenshotOcrRegion | null = null;
  let screenshotSelectionDraft: {
    pointerId: number;
    startX: number;
    startY: number;
    currentX: number;
    currentY: number;
  } | null = null;
  let visibleScreenshotOcrRegion: ScreenshotOcrRegion | null;
  let screenshotViewer: HTMLElement | undefined;
  const pageSizeOptions = [10, 25, 50];
  let screenshotPage = 1,
    screenshotPageSize = 10,
    sessionPage = 1,
    sessionPageSize = 10;
  let intervalBeingEdited: BackgroundInterval | null;
  $: screenshotPageCount = Math.max(1, Math.ceil(screenshots.length / screenshotPageSize));
  $: screenshotNeighbors = screenshotNavigation(screenshots, selectedScreenshot?.id ?? null);
  $: sessionPageCount = Math.max(1, Math.ceil(sessions.length / sessionPageSize));
  $: pagedScreenshots = screenshots.slice(
    (screenshotPage - 1) * screenshotPageSize,
    screenshotPage * screenshotPageSize,
  );
  $: pagedSessions = sessions.slice(
    (sessionPage - 1) * sessionPageSize,
    sessionPage * sessionPageSize,
  );
  $: intervalBeingEdited =
    editingIntervalId === null
      ? null
      : (intervals.find((interval) => interval.id === editingIntervalId) ?? null);
  $: visibleScreenshotOcrRegion = screenshotSelectionDraft
    ? regionFromPoints(
        screenshotSelectionDraft.startX,
        screenshotSelectionDraft.startY,
        screenshotSelectionDraft.currentX,
        screenshotSelectionDraft.currentY,
      )
    : screenshotOcrRegion;
  async function loadGameStatistics() {
    const sequence = ++gameStatisticsRequestSequence;
    gameStatisticsLoading = true;
    gameStatisticsError = '';
    try {
      const next = await api.gameStatistics(gameId);
      if (sequence === gameStatisticsRequestSequence) gameStatistics = next;
    } catch (cause) {
      if (sequence === gameStatisticsRequestSequence) {
        gameStatisticsError = userErrorMessage(cause, 'プレイ時間の推移を読み込めませんでした。');
      }
    } finally {
      if (sequence === gameStatisticsRequestSequence) gameStatisticsLoading = false;
    }
  }
  async function load(refreshStatistics = false) {
    if (refreshStatistics) void loadGameStatistics();
    try {
      const [nextGame, nextSessions, nextTimestamps, nextScreenshots] = await Promise.all([
        api.getGame(gameId),
        api.sessions(gameId),
        api.timestamps(gameId),
        api.screenshots(gameId),
      ]);
      game = nextGame;
      sessions = nextSessions;
      timestamps = nextTimestamps;
      screenshots = nextScreenshots;
      if (selectedScreenshot && !screenshots.some((shot) => shot.id === selectedScreenshot?.id)) {
        closeScreenshotViewer();
      }
      screenshotPage = Math.min(
        screenshotPage,
        Math.max(1, Math.ceil(nextScreenshots.length / screenshotPageSize)),
      );
      sessionPage = Math.min(
        sessionPage,
        Math.max(1, Math.ceil(nextSessions.length / sessionPageSize)),
      );
      if (selected) {
        selected = nextSessions.find((session) => session.id === selected?.id) ?? null;
        if (selected && sessionEditOpen && !sessionFormDirty) {
          sessionStart = inputTime(selected.launched_at);
          sessionEnd = inputTime(selected.exited_at);
        }
      }
    } catch {
      pageError = 'ゲーム情報を読み込めませんでした。しばらくしてからもう一度お試しください。';
    }
  }
  load(true);
  onMount(() => {
    let unlisten = () => {};
    const refreshIntervals = (status: TrackingStatus) => {
      const trackingGame = status.games.find((entry) => entry.game_id === gameId);
      const nextTrackingStateKey = trackingGame
        ? `${trackingGame.session_id}:${trackingGame.phase}:${status.is_paused}:${status.pause_started_at}`
        : '';
      const refreshStatistics = nextTrackingStateKey !== trackingStateKey;
      trackingStateKey = nextTrackingStateKey;
      load(refreshStatistics);
      const selectedId = selected?.id;
      if (selectedId) {
        api.intervals(selectedId).then((value) => {
          if (selected?.id === selectedId) intervals = value;
        });
      }
    };
    listen<TrackingStatus>('tracking-status', (event) => refreshIntervals(event.payload)).then(
      (fn) => (unlisten = fn),
    );
    let unlistenScreenshot = () => {};
    listen<number>('screenshot-captured', (event) => {
      if (event.payload === gameId) {
        screenshotPage = 1;
        load();
        showToast('スクリーンショットを保存しました');
      }
    }).then((fn) => (unlistenScreenshot = fn));
    let unlistenScreenshotError = () => {};
    listen<string>('screenshot-error', (event) => showToast(event.payload, true)).then(
      (fn) => (unlistenScreenshotError = fn),
    );
    const timer = setInterval(() => {
      nowMs = Date.now();
      load();
    }, 1000);
    const statisticsTimer = setInterval(() => {
      if (trackingStateKey) loadGameStatistics();
    }, 30_000);
    return () => {
      unlisten();
      unlistenScreenshot();
      unlistenScreenshotError();
      clearInterval(timer);
      clearInterval(statisticsTimer);
      if (toastTimer) clearTimeout(toastTimer);
    };
  });
  function resetSessionEditor() {
    sessionEditOpen = false;
    sessionStart = '';
    sessionEnd = '';
    sessionFormDirty = false;
    sessionFormError = '';
    sessionActionError = '';
    intervalListError = '';
    newIntervalOpen = false;
    newIntervalStart = '';
    newIntervalEnd = '';
    newIntervalError = '';
    editingIntervalId = null;
    intervalEditStart = '';
    intervalEditEnd = '';
    intervalEditError = '';
  }
  function closeSessionEditor() {
    selected = null;
    intervals = [];
    resetSessionEditor();
  }
  async function beginSessionDetail(s: Session) {
    selected = s;
    intervals = [];
    resetSessionEditor();
    try {
      const nextIntervals = await api.intervals(s.id);
      if (selected?.id === s.id) intervals = nextIntervals;
    } catch {
      if (selected?.id === s.id) {
        intervalListError =
          'バックグラウンド区間を読み込めませんでした。モーダルを閉じて、もう一度お試しください。';
      }
    }
  }
  function beginSessionEdit() {
    if (!selected) return;
    sessionStart = inputTime(selected.launched_at);
    sessionEnd = inputTime(selected.exited_at);
    sessionFormDirty = false;
    sessionFormError = '';
    sessionEditOpen = true;
  }
  function cancelSessionEdit() {
    sessionEditOpen = false;
    sessionStart = '';
    sessionEnd = '';
    sessionFormDirty = false;
    sessionFormError = '';
  }
  async function addExe() {
    if (!newPath) return;
    try {
      await api.addExecutable(gameId, newPath);
      newPath = '';
      await load(true);
      showToast('実行ファイルを登録しました');
    } catch (e) {
      showToast(userErrorMessage(e, '実行ファイルを登録できませんでした。'), true);
    }
  }
  async function selectExe() {
    const selected = await open({
      title: 'ゲームの実行ファイルを選択',
      multiple: false,
      directory: false,
      filters: [
        { name: 'ゲーム実行ファイル', extensions: ['exe', 'bin'] },
        { name: 'すべてのファイル', extensions: ['*'] },
      ],
    });
    if (selected) newPath = selected;
  }
  async function removeExe(id: number) {
    try {
      await api.removeExecutable(id);
      await load(true);
      showToast('実行ファイルの登録を削除しました');
    } catch (e) {
      showToast(userErrorMessage(e, '実行ファイルの登録を削除できませんでした。'), true);
    }
  }
  function beginManualSession() {
    manualStart = '';
    manualEnd = '';
    manualSessionError = '';
    manualSessionOpen = true;
  }
  function cancelManualSession() {
    manualSessionOpen = false;
    manualStart = '';
    manualEnd = '';
    manualSessionError = '';
  }
  async function addManual() {
    manualSessionError = validateManualSession(manualStart, manualEnd, sessionRanges());
    if (manualSessionError) return;
    manualSessionSaving = true;
    try {
      await api.manualSession(gameId, utc(manualStart), utc(manualEnd));
      cancelManualSession();
      await load(true);
    } catch (e) {
      manualSessionError = userErrorMessage(
        e,
        '手動セッションを追加できませんでした。入力内容を確認して、もう一度お試しください。',
      );
    } finally {
      manualSessionSaving = false;
    }
  }
  async function saveSession() {
    if (!selected) return;
    sessionFormError = selected.exited_at
      ? validateSessionEdit(
          sessionStart,
          sessionEnd,
          intervalRanges(),
          sessionRanges(),
          selected.id,
        )
      : validateRunningSessionEdit(sessionStart, intervalRanges(), sessionRanges(), selected.id);
    if (sessionFormError) return;
    sessionSaving = true;
    try {
      await api.updateSession(
        selected.id,
        utc(sessionStart),
        selected.exited_at ? utc(sessionEnd) : null,
      );
      sessionFormDirty = false;
      cancelSessionEdit();
      await load(true);
    } catch (e) {
      sessionFormError = userErrorMessage(
        e,
        'セッションを保存できませんでした。入力内容を確認して、もう一度お試しください。',
      );
    } finally {
      sessionSaving = false;
    }
  }
  async function removeSession() {
    if (!selected) return;
    const sessionId = selected.id;
    sessionActionError = '';
    try {
      await api.deleteSession(sessionId);
      closeSessionEditor();
      await load(true);
    } catch {
      sessionActionError = 'セッションを削除できませんでした。もう一度お試しください。';
    }
  }
  async function confirmSessionReview() {
    if (!selected?.needs_review || sessionReviewSaving) return;
    sessionReviewSaving = true;
    sessionActionError = '';
    try {
      await api.confirmSessionReview(selected.id);
      await load(true);
      showToast('セッションを確認済みにしました');
    } catch {
      sessionActionError = '要確認を解除できませんでした。もう一度お試しください。';
    } finally {
      sessionReviewSaving = false;
    }
  }
  async function removeAllSessions() {
    if (!sessions.length) return;
    pageError = '';
    try {
      await api.deleteAllSessions(gameId);
      closeSessionEditor();
      await load(true);
    } catch {
      pageError =
        'すべてのセッションを削除できませんでした。しばらくしてからもう一度お試しください。';
    }
  }
  function intervalRanges() {
    return intervals.map((interval) => ({
      id: interval.id,
      start: interval.started_at,
      end: interval.ended_at,
    }));
  }
  function sessionRanges() {
    return sessions.map((session) => ({
      id: session.id,
      start: session.launched_at,
      end: session.exited_at,
    }));
  }
  function intervalDurationSeconds(interval: BackgroundInterval) {
    const start = new Date(interval.started_at).getTime();
    const end = interval.ended_at ? new Date(interval.ended_at).getTime() : nowMs;
    return Math.max(0, Math.floor((end - start) / 1000));
  }
  function beginEditInterval(interval: BackgroundInterval) {
    editingIntervalId = interval.id;
    intervalEditStart = inputTime(interval.started_at);
    intervalEditEnd = inputTime(interval.ended_at);
    intervalEditError = '';
  }
  function cancelEditInterval() {
    editingIntervalId = null;
    intervalEditStart = '';
    intervalEditEnd = '';
    intervalEditError = '';
  }
  async function saveInterval() {
    if (!selected || editingIntervalId === null) return;
    const interval = intervals.find((item) => item.id === editingIntervalId);
    if (!interval) return;
    const intervalEnd = interval.ended_at ? intervalEditEnd : null;
    const validationError = validateBackgroundInterval(
      intervalEditStart,
      intervalEnd,
      { start: selected.launched_at, end: selected.exited_at },
      intervalRanges(),
      interval.id,
    );
    if (validationError) {
      intervalEditError = validationError;
      return;
    }
    intervalEditError = '';
    savingIntervalId = interval.id;
    try {
      await api.updateInterval(
        interval.id,
        utc(intervalEditStart),
        intervalEnd ? utc(intervalEnd) : null,
      );
      intervals = await api.intervals(interval.play_session_id);
      cancelEditInterval();
      await load(true);
    } catch (e) {
      intervalEditError = userErrorMessage(
        e,
        'バックグラウンド区間を保存できませんでした。入力内容を確認して、もう一度お試しください。',
      );
    } finally {
      savingIntervalId = null;
    }
  }
  async function deleteEditingInterval() {
    const interval = intervalBeingEdited;
    if (!interval?.ended_at) return;
    intervalEditError = '';
    savingIntervalId = interval.id;
    try {
      await api.deleteInterval(interval.id);
      intervals = await api.intervals(interval.play_session_id);
      cancelEditInterval();
      await load(true);
    } catch {
      intervalEditError = 'バックグラウンド区間を削除できませんでした。もう一度お試しください。';
    } finally {
      savingIntervalId = null;
    }
  }
  function beginAddInterval() {
    newIntervalStart = '';
    newIntervalEnd = '';
    newIntervalError = '';
    newIntervalOpen = true;
  }
  function cancelAddInterval() {
    newIntervalOpen = false;
    newIntervalStart = '';
    newIntervalEnd = '';
    newIntervalError = '';
  }
  async function addInterval() {
    if (!selected) return;
    newIntervalError = validateBackgroundInterval(
      newIntervalStart,
      newIntervalEnd,
      { start: selected.launched_at, end: selected.exited_at },
      intervalRanges(),
    );
    if (newIntervalError) return;
    intervalCreating = true;
    try {
      await api.createInterval(selected.id, utc(newIntervalStart), utc(newIntervalEnd));
      intervals = await api.intervals(selected.id);
      cancelAddInterval();
      await load(true);
    } catch (e) {
      newIntervalError = userErrorMessage(
        e,
        'バックグラウンド区間を追加できませんでした。入力内容を確認して、もう一度お試しください。',
      );
    } finally {
      intervalCreating = false;
    }
  }
  function showToast(message: string, isError = false) {
    toast = message;
    toastError = isError;
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = window.setTimeout(() => (toast = ''), 4000);
  }
  function beginGameEdit() {
    if (!game) return;
    editTitle = game.title;
    editBrand = game.brand ?? '';
    editReleaseDate = game.release_date ?? '';
    editReleaseDateComplete = true;
    editReleaseDateError = '';
    editSourceUrl = game.source_url ?? '';
    editingGame = true;
  }
  function cancelGameEdit() {
    editingGame = false;
    editReleaseDateError = '';
  }
  async function saveGameInfo() {
    if (!game || !editTitle.trim()) return showToast('タイトルを入力してください', true);
    if (!editReleaseDateComplete) {
      editReleaseDateError = '発売日は年・月・日をすべて選択してください。';
      return;
    }
    try {
      await api.updateGame(game.id, {
        title: editTitle.trim(),
        brand: editBrand.trim() || undefined,
        release_date: editReleaseDate || undefined,
        source_url: editSourceUrl.trim() || undefined,
      });
      editingGame = false;
      await load(true);
      showToast('ゲーム情報を保存しました');
    } catch (e) {
      showToast(userErrorMessage(e, 'ゲーム情報を保存できませんでした。'), true);
    }
  }
  function beginLaunchCommandEdit() {
    if (!game) return;
    launchCommandDraft = game.launch_command ?? '';
    launchCommandEditing = true;
  }
  function cancelLaunchCommandEdit() {
    launchCommandEditing = false;
  }
  async function saveLaunchCommand() {
    if (!game) return;
    launchCommandSaving = true;
    try {
      await api.setLaunchCommand(game.id, launchCommandDraft.trim() || null);
      launchCommandEditing = false;
      await load(true);
      showToast('起動コマンドを保存しました');
    } catch (e) {
      showToast(userErrorMessage(e, '起動コマンドを保存できませんでした。'), true);
    } finally {
      launchCommandSaving = false;
    }
  }
  async function openSourceUrl() {
    if (!game?.source_url) return;
    try {
      await api.openExternalUrl(game.source_url);
    } catch (e) {
      showToast(userErrorMessage(e, 'URLを開けませんでした。'), true);
    }
  }
  async function refreshMeta() {
    if (refreshingMeta) return;
    refreshingMeta = true;
    pageError = '';
    try {
      const warnings = await api.refreshMetadata(gameId);
      await load(true);
      showToast(
        [
          `${metadataSourceLabel(game?.metadata_source)}からゲーム情報を更新しました`,
          ...warnings,
        ].join(' '),
      );
    } catch (e) {
      pageError = userErrorMessage(
        e,
        '情報を更新できませんでした。入力内容と通信状態を確認してください。',
      );
      showToast(pageError, true);
    } finally {
      refreshingMeta = false;
    }
  }
  function openThumbnailEditor() {
    if (!game) return;
    thumbnailOriginalPath = game.thumbnail_path;
    thumbnailDraftPath = game.thumbnail_path;
    thumbnailError = '';
    thumbnailEditorOpen = true;
  }
  function closeThumbnailEditor() {
    if (thumbnailImporting || thumbnailCropBusy || thumbnailSaving) return;
    thumbnailEditorOpen = false;
    thumbnailError = '';
  }
  async function selectThumbnail() {
    const selected = await open({
      title: 'サムネイル画像を選択',
      multiple: false,
      directory: false,
      filters: [{ name: '画像ファイル', extensions: ['jpg', 'jpeg', 'png', 'webp'] }],
    });
    if (!selected) return;
    thumbnailImporting = true;
    thumbnailError = '';
    try {
      thumbnailDraftPath = await api.importThumbnail(selected);
    } catch (e) {
      thumbnailError = userErrorMessage(e, 'サムネイル画像を取り込めませんでした。');
    } finally {
      thumbnailImporting = false;
    }
  }
  async function saveThumbnail(path: string | null = thumbnailDraftPath) {
    if (!game || thumbnailImporting || thumbnailSaving) return;
    thumbnailSaving = true;
    thumbnailError = '';
    try {
      await api.updateGameThumbnail(game.id, path);
      thumbnailEditorOpen = false;
      await load(true);
      showToast('サムネイルを保存しました');
    } catch (e) {
      thumbnailDraftPath = path;
      thumbnailError = userErrorMessage(e, 'サムネイル画像を保存できませんでした。');
    } finally {
      thumbnailSaving = false;
    }
  }
  async function removeGame() {
    pageError = '';
    try {
      await api.deleteGame(gameId);
      onback();
    } catch {
      pageError = 'ゲームを削除できませんでした。しばらくしてからもう一度お試しください。';
    }
  }
  async function launch() {
    try {
      await api.launchGame(gameId);
    } catch (e) {
      pageError = userErrorMessage(e, 'ゲームを起動できませんでした。');
    }
  }
  async function updatePlayStatus(status: PlayStatus) {
    if (!game || game.play_status === status) return;
    try {
      await api.updateGamePlayStatus(game.id, status);
      await load(true);
      showToast(`プレイ状況を「${playStatusLabel(status)}」に変更しました`);
    } catch (e) {
      showToast(userErrorMessage(e, 'プレイ状況を変更できませんでした。'), true);
    }
  }
  async function createTimestamp() {
    const name = timestampName.trim();
    if (!name) return;
    creatingTimestamp = true;
    try {
      await api.createTimestamp(gameId, name);
      timestampName = '';
      await load(true);
      showToast(`「${name}」を記録しました`);
    } catch (e) {
      showToast(userErrorMessage(e, 'タイムスタンプを追加できませんでした。'), true);
    } finally {
      creatingTimestamp = false;
    }
  }
  function beginTimestampEdit(point: GameTimestamp) {
    editingTimestampId = point.id;
    editingTimestampName = point.name;
    editingTimestampTime = inputTime(point.marked_at);
    editingTimestampTimeComplete = true;
    timestampEditError = '';
  }
  function cancelTimestampEdit() {
    editingTimestampId = null;
    editingTimestampName = '';
    editingTimestampTime = '';
    editingTimestampTimeComplete = true;
    timestampEditError = '';
  }
  async function saveTimestamp(point: GameTimestamp) {
    const name = editingTimestampName.trim();
    if (!name) {
      timestampEditError = 'タイムスタンプのタイトルを入力してください。';
      return;
    }
    if (!editingTimestampTimeComplete || !editingTimestampTime) {
      timestampEditError = '記録日時をすべて選択してください。';
      return;
    }
    const timeChanged = editingTimestampTime !== inputTime(point.marked_at);
    if (name === point.name && !timeChanged) {
      cancelTimestampEdit();
      return;
    }
    if (savingTimestampId !== null) return;
    savingTimestampId = point.id;
    timestampEditError = '';
    try {
      await api.updateTimestamp(
        point.id,
        name,
        timeChanged ? utc(editingTimestampTime) : point.marked_at,
      );
      await load(true);
      cancelTimestampEdit();
      showToast('タイムスタンプを変更しました');
    } catch (e) {
      timestampEditError = userErrorMessage(e, 'タイムスタンプを変更できませんでした。');
    } finally {
      savingTimestampId = null;
    }
  }
  async function deleteTimestamp(id: number) {
    try {
      await api.deleteTimestamp(id);
      await load(true);
      showToast('タイムスタンプを削除しました');
    } catch (e) {
      showToast(userErrorMessage(e, 'タイムスタンプを削除できませんでした。'), true);
    }
  }
  async function deleteScreenshot(id: number) {
    try {
      await api.deleteScreenshot(id);
      if (selectedScreenshot?.id === id) closeScreenshotViewer();
      await load(true);
      showToast('スクリーンショットを削除しました');
    } catch (e) {
      showToast(userErrorMessage(e, 'スクリーンショットを削除できませんでした。'), true);
    }
  }
  async function openScreenshotDirectory() {
    try {
      await api.openScreenshotDirectory(gameId);
    } catch (e) {
      showToast(userErrorMessage(e, 'スクリーンショットの保存先を開けませんでした。'), true);
    }
  }
  function resetScreenshotOcrResult() {
    screenshotOcrRequestId += 1;
    screenshotOcrAttempted = false;
    screenshotOcrLoading = false;
    screenshotOcrText = '';
    screenshotOcrError = '';
  }
  function resetScreenshotOcr() {
    resetScreenshotOcrResult();
    screenshotOcrRegion = null;
    screenshotSelectionDraft = null;
  }
  function regionFromPoints(
    startX: number,
    startY: number,
    endX: number,
    endY: number,
  ): ScreenshotOcrRegion {
    return {
      x: Math.min(startX, endX),
      y: Math.min(startY, endY),
      width: Math.abs(endX - startX),
      height: Math.abs(endY - startY),
    };
  }
  function screenshotPoint(event: PointerEvent) {
    const image = event.currentTarget as HTMLImageElement;
    const bounds = image.getBoundingClientRect();
    return {
      x: Math.min(1, Math.max(0, (event.clientX - bounds.left) / bounds.width)),
      y: Math.min(1, Math.max(0, (event.clientY - bounds.top) / bounds.height)),
    };
  }
  function beginScreenshotSelection(event: PointerEvent) {
    if (event.button !== 0 || screenshotOcrLoading) return;
    const point = screenshotPoint(event);
    (event.currentTarget as HTMLImageElement).setPointerCapture(event.pointerId);
    screenshotSelectionDraft = {
      pointerId: event.pointerId,
      startX: point.x,
      startY: point.y,
      currentX: point.x,
      currentY: point.y,
    };
  }
  function moveScreenshotSelection(event: PointerEvent) {
    if (screenshotSelectionDraft?.pointerId !== event.pointerId) return;
    const point = screenshotPoint(event);
    screenshotSelectionDraft = {
      ...screenshotSelectionDraft,
      currentX: point.x,
      currentY: point.y,
    };
  }
  function finishScreenshotSelection(event: PointerEvent) {
    if (screenshotSelectionDraft?.pointerId !== event.pointerId) return;
    const image = event.currentTarget as HTMLImageElement;
    const point = screenshotPoint(event);
    const region = regionFromPoints(
      screenshotSelectionDraft.startX,
      screenshotSelectionDraft.startY,
      point.x,
      point.y,
    );
    if (image.hasPointerCapture(event.pointerId)) image.releasePointerCapture(event.pointerId);
    screenshotSelectionDraft = null;
    screenshotOcrRegion = region.width >= 0.005 && region.height >= 0.005 ? region : null;
    resetScreenshotOcrResult();
  }
  function cancelScreenshotSelection(event: PointerEvent) {
    if (screenshotSelectionDraft?.pointerId !== event.pointerId) return;
    const image = event.currentTarget as HTMLImageElement;
    if (image.hasPointerCapture(event.pointerId)) image.releasePointerCapture(event.pointerId);
    screenshotSelectionDraft = null;
  }
  function clearScreenshotSelection() {
    if (screenshotOcrLoading) return;
    screenshotOcrRegion = null;
    screenshotSelectionDraft = null;
    resetScreenshotOcrResult();
  }
  function openScreenshotViewer(screenshot: GameScreenshot) {
    resetScreenshotOcr();
    selectedScreenshot = screenshot;
  }
  function closeScreenshotViewer() {
    selectedScreenshot = null;
    resetScreenshotOcr();
  }
  function navigateScreenshot(direction: -1 | 1) {
    if (screenshotSelectionDraft || screenshotViewer?.querySelector('[role="alertdialog"]')) {
      return;
    }
    const neighbors = screenshotNavigation(screenshots, selectedScreenshot?.id ?? null);
    const next = direction === -1 ? neighbors.previous : neighbors.next;
    if (!next) return;
    openScreenshotViewer(next);
    screenshotPage = Math.floor((neighbors.index + direction) / screenshotPageSize) + 1;
  }
  function handleScreenshotKeydown(event: KeyboardEvent) {
    if (
      !selectedScreenshot ||
      event.defaultPrevented ||
      event.isComposing ||
      event.altKey ||
      event.ctrlKey ||
      event.metaKey ||
      event.shiftKey ||
      !['ArrowLeft', 'ArrowRight'].includes(event.key)
    ) {
      return;
    }
    const target = event.target;
    if (
      target instanceof HTMLElement &&
      (target.isContentEditable || target.closest('input, textarea, select, [role="textbox"]'))
    ) {
      return;
    }
    if (screenshotSelectionDraft || screenshotViewer?.querySelector('[role="alertdialog"]')) {
      return;
    }
    event.preventDefault();
    navigateScreenshot(event.key === 'ArrowLeft' ? -1 : 1);
  }
  async function recognizeScreenshotText() {
    const screenshotId = selectedScreenshot?.id;
    if (screenshotId === undefined || screenshotOcrLoading) return;
    const region = screenshotOcrRegion ? { ...screenshotOcrRegion } : null;
    const requestId = ++screenshotOcrRequestId;
    screenshotOcrAttempted = true;
    screenshotOcrLoading = true;
    screenshotOcrText = '';
    screenshotOcrError = '';
    try {
      const result = await api.recognizeScreenshotText(screenshotId, region);
      if (requestId !== screenshotOcrRequestId || selectedScreenshot?.id !== screenshotId) return;
      screenshotOcrText = result.text;
    } catch (e) {
      if (requestId !== screenshotOcrRequestId || selectedScreenshot?.id !== screenshotId) return;
      screenshotOcrError = userErrorMessage(e, '画像からテキストを文字起こしできませんでした。');
    } finally {
      if (requestId === screenshotOcrRequestId) screenshotOcrLoading = false;
    }
  }
  async function copyScreenshotOcrText() {
    if (!screenshotOcrText) return;
    try {
      await navigator.clipboard.writeText(screenshotOcrText);
      showToast('文字起こし結果をコピーしました');
    } catch {
      showToast('コピーできませんでした。テキスト欄から手動でコピーしてください。', true);
    }
  }
  async function searchScreenshotOcrText() {
    const query = screenshotOcrText.trim();
    if (!query || screenshotOcrLoading || screenshotOcrSearching) return;
    screenshotOcrSearching = true;
    try {
      const url = new URL('https://www.google.com/search');
      url.searchParams.set('q', query);
      await api.openExternalUrl(url.toString());
    } catch (e) {
      showToast(userErrorMessage(e, '検索結果をブラウザで開けませんでした。'), true);
    } finally {
      screenshotOcrSearching = false;
    }
  }
</script>

<svelte:window onkeydown={handleScreenshotKeydown} />

<button class="back-button" onclick={onback}>← 戻る</button>{#if game}{#if game.thumbnail_path}<div
      class="detail-backdrop"
      style:background-image={`url("${imageSrc(game.thumbnail_path)}")`}
    ></div>{/if}
  <section class="detail">
    <div class="hero">
      <div class="detail-image">
        {#if game.thumbnail_path}<img src={imageSrc(game.thumbnail_path)} alt="" />{:else}<div
            class="placeholder"
          >
            NO IMAGE
          </div>{/if}<button class="launch-overlay" onclick={launch}>▶ 起動</button><button
          type="button"
          class="detail-thumbnail-edit"
          aria-label="サムネイルを編集"
          onclick={openThumbnailEditor}
        >
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path
              d="M4 16.5V20h3.5L18.1 9.4l-3.5-3.5L4 16.5Zm16.7-9.8a1 1 0 0 0 0-1.4l-2-2a1 1 0 0 0-1.4 0l-1.6 1.6 3.5 3.5 1.5-1.7Z"
            />
          </svg>
        </button>
      </div>
      <div>
        <h1>{game.title}</h1>
        <p>{game.brand ?? 'ブランド未設定'} ・ {game.release_date ?? '発売日未設定'}</p>
        <span class="play-status status-{game.play_status}"
          >{playStatusLabel(game.play_status)}</span
        >
        <small>プレイ時間</small>
        <h2>{duration(game.total_playtime_seconds)}</h2>
        <p>{game.session_count} セッション</p>
      </div>
    </div>
    <section class="panel game-info">
      <div class="panel-heading">
        <h2>ゲーム情報</h2>
        {#if !editingGame}<div class="game-info-heading-actions">
            <button
              class="metadata-refresh"
              onclick={refreshMeta}
              disabled={refreshingMeta || !game.metadata_source}
              title={game.metadata_source ? undefined : '編集から情報取得元URLを設定してください'}
              >{#if refreshingMeta}<span class="spinner" aria-hidden="true"
                ></span>取得中…{:else}{metadataSourceLabel(
                  game.metadata_source,
                )}から情報を更新{/if}</button
            ><button onclick={beginGameEdit} disabled={refreshingMeta}>編集</button>
          </div>{/if}
      </div>
      {#if editingGame}<div class="game-info-form">
          <label>タイトル<input bind:value={editTitle} /></label><label
            >ブランド<input bind:value={editBrand} placeholder="未設定" /></label
          >
          <div class="game-info-form-field">
            <DateTimeSelect
              label="発売日"
              value={editReleaseDate}
              withTime={false}
              optional
              invalid={Boolean(editReleaseDateError)}
              onchange={(value, complete) => {
                editReleaseDate = value;
                editReleaseDateComplete = complete;
                editReleaseDateError = '';
              }}
            />
            {#if editReleaseDateError}<p class="form-error" role="alert">
                {editReleaseDateError}
              </p>{/if}
          </div>
          <label
            >情報取得元URL<input
              type="url"
              bind:value={editSourceUrl}
              placeholder="ErogameScape または VNDB のゲームURL"
            /></label
          >
          <div class="actions">
            <button class="primary" onclick={saveGameInfo}>保存</button><button
              onclick={cancelGameEdit}>キャンセル</button
            >
          </div>
        </div>{:else}<dl>
          <div>
            <dt>タイトル</dt>
            <dd>{game.title}</dd>
          </div>
          <div>
            <dt>ブランド</dt>
            <dd>{game.brand ?? '未設定'}</dd>
          </div>
          <div>
            <dt>発売日</dt>
            <dd>{game.release_date ?? '未設定'}</dd>
          </div>
          <div>
            <dt>プレイ状況</dt>
            <dd>
              <select
                class="play-status-select"
                value={game.play_status}
                onchange={(event) =>
                  updatePlayStatus((event.currentTarget as HTMLSelectElement).value as PlayStatus)}
              >
                {#each playStatusOptions as option}<option value={option.value}
                    >{option.label}</option
                  >{/each}
              </select>
            </dd>
          </div>
          <div>
            <dt>情報取得元URL</dt>
            <dd>
              {#if game.source_url}<button
                  class="external-link"
                  title="既定のブラウザで開く"
                  onclick={openSourceUrl}>{game.source_url}<span aria-hidden="true">↗</span></button
                >{:else}未設定{/if}
            </dd>
          </div>
        </dl>{/if}
      <div class="game-executable-section">
        <h3>実行ファイル</h3>
        {#each game.executables as x}<div class="listrow">
            <code>{x.path}</code><DeleteButton
              title="実行ファイル登録の削除"
              message={`実行ファイル「${x.path}」の登録を削除します。ファイル自体は削除されません。`}
              onconfirm={() => removeExe(x.id)}
            />
          </div>{/each}
        <div class="row executable-add-row">
          <input bind:value={newPath} placeholder="exeを選択してください" readonly /><button
            type="button"
            onclick={selectExe}>参照…</button
          ><button onclick={addExe}>追加</button>
        </div>
        <h3>起動コマンド</h3>
        {#if launchCommandEditing}
          <p class="hint">
            未入力なら上の実行ファイルを起動します。相対パスは実行ファイルと同じフォルダを基準にします。
          </p>
          <div class="row executable-add-row">
            <input
              bind:value={launchCommandDraft}
              placeholder="例: game.exe -w"
              disabled={launchCommandSaving}
            /><button type="button" onclick={saveLaunchCommand} disabled={launchCommandSaving}
              >保存</button
            ><button type="button" onclick={cancelLaunchCommandEdit} disabled={launchCommandSaving}
              >キャンセル</button
            >
          </div>
        {:else}
          <div class="listrow">
            <code>{game.launch_command ?? '未設定（実行ファイルを直接起動）'}</code><button
              type="button"
              onclick={beginLaunchCommandEdit}>編集</button
            >
          </div>
        {/if}
      </div>
    </section>
    <section class="panel game-statistics-panel">
      <div class="panel-heading">
        <h2>プレイ時間の推移</h2>
        {#if gameStatistics?.days.length}<small
            >{formatDateKey(gameStatistics.period.start_date)}〜{formatDateKey(
              gameStatistics.period.end_date,
            )}</small
          >{/if}
      </div>
      {#if gameStatisticsError}<p class="error game-statistics-error">
          {gameStatisticsError}
        </p>{/if}
      {#if gameStatistics?.days.length}
        <PlaytimeTrend
          days={gameStatistics.days}
          kind="all"
          {timestamps}
          expanded
          fixedRange
          showGameBreakdown={false}
        />
      {:else if gameStatisticsLoading}
        <p class="game-statistics-empty">プレイ時間を集計しています…</p>
      {:else if !gameStatisticsError}
        <p class="game-statistics-empty">まだプレイ記録がありません。</p>
      {/if}
    </section>
    <section class="panel timestamp-panel">
      <div class="panel-heading"><h2>タイムスタンプ</h2></div>
      <p class="hint">
        ルートクリアなどの節目を記録すると、到達までにかかったプレイ時間を確認できます。
      </p>
      <div class="row timestamp-create">
        <input
          aria-label="タイムスタンプのタイトル"
          maxlength="100"
          bind:value={timestampName}
          placeholder="例: ○○ルートクリア"
          onkeydown={(e) => {
            if (e.key === 'Enter') createTimestamp();
          }}
        /><button
          class="primary"
          disabled={creatingTimestamp || !timestampName.trim()}
          onclick={createTimestamp}>{creatingTimestamp ? '記録中…' : '現在時刻で記録'}</button
        >
      </div>
      {#if !timestamps.length}<p class="timestamp-empty">まだタイムスタンプはありません。</p>{/if}
      <div class="timestamp-list">
        {#each timestamps as point, index}<article class="timestamp-item">
            <div class="timestamp-marker" aria-hidden="true"></div>
            <div
              class="timestamp-content"
              class:timestamp-editing={editingTimestampId === point.id}
            >
              {#if editingTimestampId === point.id}<div class="timestamp-edit-fields">
                  <label class="timestamp-title-input"
                    ><span>タイトル</span><input
                      class="timestamp-name-input"
                      maxlength="100"
                      bind:value={editingTimestampName}
                      aria-invalid={Boolean(timestampEditError)}
                      disabled={savingTimestampId === point.id}
                      oninput={() => (timestampEditError = '')}
                      onkeydown={(event) => {
                        if (event.key === 'Enter') saveTimestamp(point);
                        if (event.key === 'Escape') cancelTimestampEdit();
                      }}
                    /></label
                  >
                  <div class="timestamp-time-input">
                    <DateTimeSelect
                      label="記録日時"
                      value={editingTimestampTime}
                      disabled={savingTimestampId === point.id}
                      invalid={Boolean(timestampEditError)}
                      onchange={(value, complete) => {
                        editingTimestampTime = value;
                        editingTimestampTimeComplete = complete;
                        timestampEditError = '';
                      }}
                    />
                  </div>
                  {#if timestampEditError}<p class="form-error timestamp-edit-error" role="alert">
                      {timestampEditError}
                    </p>{/if}
                </div>
              {:else}<div class="timestamp-title">
                  <h3>{point.name}</h3>
                  <small>{local(point.marked_at)}</small>
                </div>{/if}
              <div class="timestamp-times">
                <span
                  ><small>累計プレイ時間</small><strong>{duration(point.playtime_seconds)}</strong
                  ></span
                ><span
                  ><small>{index === 0 ? 'ゲーム開始から' : '前のポイントから'}</small><strong
                    >{duration(point.since_previous_seconds)}</strong
                  ></span
                >
              </div>
            </div>
            <div class="timestamp-item-actions">
              {#if editingTimestampId === point.id}<button
                  class="primary"
                  disabled={savingTimestampId === point.id || !editingTimestampName.trim()}
                  onclick={() => saveTimestamp(point)}
                  >{savingTimestampId === point.id ? '保存中…' : '保存'}</button
                ><button disabled={savingTimestampId === point.id} onclick={cancelTimestampEdit}
                  >キャンセル</button
                >{:else}<button onclick={() => beginTimestampEdit(point)}>編集</button><DeleteButton
                  title="タイムスタンプの削除"
                  message={`タイムスタンプ「${point.name}」を削除します。元に戻せません。`}
                  onconfirm={() => deleteTimestamp(point.id)}
                />{/if}
            </div>
          </article>{/each}
      </div>
    </section>
    <section class="panel session-history-panel">
      <div class="panel-heading">
        <h2>Session History</h2>
        <div class="session-heading-actions">
          <button class="primary" type="button" onclick={beginManualSession}
            >手動セッションを追加</button
          >
          <label class="page-size-control"
            >表示<select
              value={sessionPageSize}
              onchange={(event) => {
                sessionPageSize = Number((event.currentTarget as HTMLSelectElement).value);
                sessionPage = 1;
              }}
              >{#each pageSizeOptions as size}<option value={size}>{size}件</option>{/each}</select
            ></label
          >
        </div>
      </div>
      {#each pagedSessions as s}<HistoryDataRow
          label={`${local(s.launched_at)} → ${local(s.exited_at)}${s.needs_review ? ' ・ 要確認' : ''}`}
          seconds={s.playtime_seconds}
          selected={selected?.id === s.id}
          recording={!s.exited_at}
          onselect={() => beginSessionDetail(s)}
        />{/each}
      {#if sessionPageCount > 1}<div class="pagination" aria-label="セッション履歴のページ移動">
          <button disabled={sessionPage === 1} onclick={() => (sessionPage -= 1)}>← 前へ</button>
          <span>{sessionPage} / {sessionPageCount}</span>
          <button disabled={sessionPage === sessionPageCount} onclick={() => (sessionPage += 1)}
            >次へ →</button
          >
        </div>{/if}
    </section>
    <section class="panel screenshot-panel">
      <div class="panel-heading">
        <h2>スクリーンショット</h2>
        <div class="screenshot-heading-actions">
          <small>{screenshots.length}枚</small>
          <label class="page-size-control"
            >表示<select
              value={screenshotPageSize}
              onchange={(event) => {
                screenshotPageSize = Number((event.currentTarget as HTMLSelectElement).value);
                screenshotPage = 1;
              }}
              >{#each pageSizeOptions as size}<option value={size}>{size}件</option>{/each}</select
            ></label
          >
          <button onclick={openScreenshotDirectory}>保存先を開く</button>
        </div>
      </div>
      {#if screenshots.length}
        <div class="screenshot-grid">
          {#each pagedScreenshots as shot}
            <article class="screenshot-card">
              <button class="screenshot-preview" onclick={() => openScreenshotViewer(shot)}>
                <img
                  src={imageSrc(shot.path)}
                  alt={`${local(shot.captured_at)}のスクリーンショット`}
                />
              </button>
              <div>
                <small>{local(shot.captured_at)}</small>
              </div>
            </article>
          {/each}
        </div>
        {#if screenshotPageCount > 1}<div
            class="pagination"
            aria-label="スクリーンショットのページ移動"
          >
            <button disabled={screenshotPage === 1} onclick={() => (screenshotPage -= 1)}
              >← 前へ</button
            >
            <span>{screenshotPage} / {screenshotPageCount}</span>
            <button
              disabled={screenshotPage === screenshotPageCount}
              onclick={() => (screenshotPage += 1)}>次へ →</button
            >
          </div>{/if}
      {:else}
        <p class="hint">計測中のゲームがフォアグラウンドにあるとき、設定したキーで撮影できます。</p>
      {/if}
    </section>
    <section class="panel danger-zone">
      <div class="danger-zone-heading">
        <h2>危険な操作</h2>
        <p>ここで行った削除は元に戻せません。</p>
      </div>
      <div class="danger-zone-item">
        <div>
          <strong>すべてのセッションを削除</strong>
          <p>このゲームのセッションと、各セッションに含まれるバックグラウンド時間を削除します。</p>
        </div>
        <DeleteButton
          label="すべてのセッションを削除"
          title="すべてのセッションを削除"
          message={`${sessions.length}件のセッションとバックグラウンド時間の記録をすべて削除します。元に戻せません。`}
          disabled={!sessions.length}
          onconfirm={removeAllSessions}
        />
      </div>
      <div class="danger-zone-item">
        <div>
          <strong>ゲームを削除</strong>
          <p>ゲーム情報と、このゲームに関連するすべての記録を削除します。</p>
        </div>
        <DeleteButton
          label="ゲームを削除"
          title="ゲームの削除"
          message={`「${game.title}」のゲーム情報、すべてのプレイ履歴、タイムスタンプ、スクリーンショットを削除します。元に戻せません。`}
          onconfirm={removeGame}
        />
      </div>
    </section>
  </section>{/if}
{#if thumbnailEditorOpen}<div class="modal detail-thumbnail-modal">
    <div
      class="panel detail-thumbnail-dialog"
      role="dialog"
      tabindex="-1"
      aria-modal="true"
      aria-labelledby="detail-thumbnail-title"
    >
      <button
        class="close"
        aria-label="閉じる"
        disabled={thumbnailImporting || thumbnailCropBusy || thumbnailSaving}
        onclick={closeThumbnailEditor}>×</button
      >
      <h2 id="detail-thumbnail-title">サムネイルを編集</h2>
      {#if thumbnailError}<p class="error">{thumbnailError}</p>{/if}
      {#if thumbnailDraftPath}{#key thumbnailDraftPath}<ThumbnailCropEditor
            imagePath={thumbnailDraftPath}
            ondone={saveThumbnail}
            onremove={() => (thumbnailDraftPath = null)}
            onselect={selectThumbnail}
            onbusy={(busy) => (thumbnailCropBusy = busy)}
            selecting={thumbnailImporting}
            saveDisabled={thumbnailDraftPath === thumbnailOriginalPath}
          />{/key}{:else}<div class="thumbnail-preview detail-thumbnail-empty">
          <span>NO IMAGE</span>
        </div>
        <div class="actions detail-thumbnail-actions">
          <button
            type="button"
            disabled={thumbnailImporting ||
              thumbnailSaving ||
              thumbnailDraftPath === thumbnailOriginalPath}
            onclick={() => (thumbnailDraftPath = thumbnailOriginalPath)}>リセット</button
          >
          <button
            type="button"
            disabled={thumbnailImporting || thumbnailSaving}
            onclick={selectThumbnail}>{thumbnailImporting ? '取込中…' : '画像を選択…'}</button
          >
          <button
            type="button"
            class="primary"
            disabled={thumbnailImporting ||
              thumbnailSaving ||
              thumbnailDraftPath === thumbnailOriginalPath}
            onclick={() => saveThumbnail()}>{thumbnailSaving ? '保存中…' : '保存'}</button
          >
        </div>{/if}
    </div>
  </div>{/if}
{#if manualSessionOpen}<div class="modal manual-session-modal">
    <div
      class="panel editor manual-session-dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="manual-session-title"
    >
      <button
        class="close"
        aria-label="閉じる"
        disabled={manualSessionSaving}
        onclick={cancelManualSession}>×</button
      >
      <h2 id="manual-session-title">手動セッションを追加</h2>
      <form
        class="history-range-form"
        novalidate
        onsubmit={(event) => {
          event.preventDefault();
          addManual();
        }}
      >
        <div class="history-range-fields">
          <DateTimeSelect
            label="開始日時"
            value={manualStart}
            disabled={manualSessionSaving}
            invalid={Boolean(manualSessionError)}
            onchange={(value) => {
              manualStart = value;
              manualSessionError = '';
            }}
          />
          <DateTimeSelect
            label="終了日時"
            value={manualEnd}
            disabled={manualSessionSaving}
            invalid={Boolean(manualSessionError)}
            onchange={(value) => {
              manualEnd = value;
              manualSessionError = '';
            }}
          />
        </div>
        <div class="actions">
          <button type="button" disabled={manualSessionSaving} onclick={cancelManualSession}
            >キャンセル</button
          ><button class="primary" type="submit" disabled={manualSessionSaving}
            >{manualSessionSaving ? '追加中…' : '追加'}</button
          >
        </div>
        {#if manualSessionError}<p class="form-error" role="alert">{manualSessionError}</p>{/if}
      </form>
    </div>
  </div>{/if}
{#if selected}<div class="modal">
    <div
      class="panel editor"
      role="dialog"
      aria-modal="true"
      aria-labelledby="session-detail-title"
    >
      <button
        class="close"
        aria-label="閉じる"
        disabled={sessionSaving || sessionReviewSaving || savingIntervalId !== null}
        onclick={closeSessionEditor}>×</button
      >
      <h2 id="session-detail-title">セッション詳細</h2>
      {#if selected.needs_review}<div
          class="session-review-notice"
          aria-labelledby="session-review-title"
        >
          <strong id="session-review-title">このセッションは確認が必要です</strong>
          <p>
            前回、アプリがゲームの終了を確認する前に計測が中断されたため、最後に記録できた時刻を終了日時として復旧しています。アプリやPCが予期せず終了した場合などに表示されます。
          </p>
          <p>
            開始・終了日時とバックグラウンド時間を確認し、必要なら編集してください。記録に問題がなければ確認済みにできます。
          </p>
          <button
            class="primary"
            type="button"
            disabled={sessionReviewSaving || sessionSaving || savingIntervalId !== null}
            onclick={confirmSessionReview}
            >{sessionReviewSaving ? '確認中…' : '確認済みにする'}</button
          >
        </div>{/if}
      <div class="session-breakdown session-time-summary">
        <span
          ><small>起動時間</small><strong
            >{duration(
              selected.running_seconds ??
                Math.max(0, Math.floor((nowMs - new Date(selected.launched_at).getTime()) / 1000)),
            )}</strong
          ></span
        >
        <span><small>プレイ時間</small><strong>{duration(selected.playtime_seconds)}</strong></span>
        <span
          ><small>バックグラウンド時間</small><strong
            >{duration(selected.background_seconds)}</strong
          ></span
        >
        <span
          ><small title="バックグラウンドで既に除外された時間は含みません"
            >一時停止による計測除外時間</small
          ><strong
            >{typeof selected.pause_excluded_seconds === 'number'
              ? duration(selected.pause_excluded_seconds)
              : '—'}</strong
          ></span
        >
      </div>
      <div class="session-breakdown session-date-breakdown">
        <span><small>開始日時</small><strong>{local(selected.launched_at)}</strong></span>
        <span><small>終了日時</small><strong>{local(selected.exited_at)}</strong></span>
      </div>
      <div class="actions session-detail-actions">
        <button
          class="primary"
          type="button"
          disabled={sessionReviewSaving}
          onclick={beginSessionEdit}>セッションを編集</button
        ><DeleteButton
          title="セッションの削除"
          message={`${local(selected.launched_at)} から始まるセッションを削除します。バックグラウンド時間の記録も削除され、元に戻せません。`}
          disabled={sessionReviewSaving}
          onconfirm={removeSession}
        />
      </div>
      {#if sessionActionError}<p class="form-error" role="alert">{sessionActionError}</p>{/if}
      <h3>バックグラウンド区間</h3>
      <p class="hint">
        {backgroundIntervalDescription(excludeBackgroundTime)}
      </p>
      {#if intervalListError}<p class="form-error" role="alert">{intervalListError}</p>{/if}
      {#each intervals as i (i.id)}<HistoryDataRow
          label={`${local(i.started_at)} ～ ${
            i.ended_at ? local(i.ended_at) : 'バックグラウンド時間を記録中'
          }`}
          seconds={intervalDurationSeconds(i)}
          recording={!i.ended_at}
          disabled={sessionReviewSaving || savingIntervalId === i.id}
          onselect={() => beginEditInterval(i)}
        />{/each}
      <button type="button" disabled={sessionReviewSaving} onclick={beginAddInterval}
        >バックグラウンド区間を追加</button
      >
    </div>
  </div>{/if}
{#if sessionEditOpen && selected}<div class="modal history-entry-modal">
    <div
      class="panel editor history-entry-dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="session-editor-title"
    >
      <button class="close" aria-label="閉じる" disabled={sessionSaving} onclick={cancelSessionEdit}
        >×</button
      >
      <h2 id="session-editor-title">セッションを編集</h2>
      <form
        class="history-range-form"
        novalidate
        onsubmit={(event) => {
          event.preventDefault();
          saveSession();
        }}
      >
        <div class="history-range-fields">
          <DateTimeSelect
            label="開始日時"
            value={sessionStart}
            disabled={sessionSaving}
            invalid={Boolean(sessionFormError)}
            onchange={(value) => {
              sessionStart = value;
              sessionFormDirty = true;
              sessionFormError = '';
            }}
          />
          {#if selected.exited_at}<DateTimeSelect
              label="終了日時"
              value={sessionEnd}
              disabled={sessionSaving}
              invalid={Boolean(sessionFormError)}
              onchange={(value) => {
                sessionEnd = value;
                sessionFormDirty = true;
                sessionFormError = '';
              }}
            />{:else}<div class="running-end-note">
              <small>終了日時</small>
              <strong>実行中</strong>
              <span>終了日時は追跡終了時に自動で記録されます。</span>
            </div>{/if}
        </div>
        <div class="actions">
          <button type="button" disabled={sessionSaving} onclick={cancelSessionEdit}
            >キャンセル</button
          ><button class="primary" type="submit" disabled={sessionSaving}
            >{sessionSaving ? '保存中…' : '保存'}</button
          >
        </div>
        {#if sessionFormError}<p class="form-error" role="alert">{sessionFormError}</p>{/if}
      </form>
    </div>
  </div>{/if}
{#if editingIntervalId !== null && selected && intervalBeingEdited}<div
    class="modal history-entry-modal"
  >
    <div
      class="panel editor history-entry-dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="interval-editor-title"
    >
      <button
        class="close"
        aria-label="閉じる"
        disabled={savingIntervalId === editingIntervalId}
        onclick={cancelEditInterval}>×</button
      >
      <h2 id="interval-editor-title">バックグラウンド区間を編集</h2>
      <p class="hint">
        セッション範囲：{local(selected.launched_at)} ～ {local(selected.exited_at)}
      </p>
      <form
        class="history-range-form"
        novalidate
        onsubmit={(event) => {
          event.preventDefault();
          saveInterval();
        }}
      >
        <div class="history-range-fields">
          <DateTimeSelect
            label="開始日時"
            value={intervalEditStart}
            disabled={savingIntervalId === editingIntervalId}
            invalid={Boolean(intervalEditError)}
            onchange={(value) => {
              intervalEditStart = value;
              intervalEditError = '';
            }}
          />
          {#if intervalBeingEdited.ended_at}<DateTimeSelect
              label="終了日時"
              value={intervalEditEnd}
              disabled={savingIntervalId === editingIntervalId}
              invalid={Boolean(intervalEditError)}
              onchange={(value) => {
                intervalEditEnd = value;
                intervalEditError = '';
              }}
            />{:else}<div class="running-end-note">
              <small>終了日時</small>
              <strong>記録中</strong>
              <span>終了日時はフォアグラウンド復帰時に自動で記録されます。</span>
            </div>{/if}
        </div>
        <div class="actions interval-editor-actions">
          <button
            type="button"
            disabled={savingIntervalId === editingIntervalId}
            onclick={cancelEditInterval}>キャンセル</button
          ><button class="primary" type="submit" disabled={savingIntervalId === editingIntervalId}
            >{savingIntervalId === editingIntervalId ? '保存中…' : '保存'}</button
          >
          {#if intervalBeingEdited.ended_at}<DeleteButton
              className="interval-editor-delete"
              disabled={savingIntervalId === editingIntervalId}
              title="バックグラウンド区間の削除"
              message={`${local(intervalBeingEdited.started_at)} から ${local(intervalBeingEdited.ended_at)} までのバックグラウンド区間を削除します。区間の記録は元に戻せません。`}
              onconfirm={deleteEditingInterval}
            />{/if}
        </div>
        {#if intervalEditError}<p class="form-error" role="alert">{intervalEditError}</p>{/if}
      </form>
    </div>
  </div>{/if}
{#if newIntervalOpen && selected}<div class="modal history-entry-modal">
    <div
      class="panel editor history-entry-dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="new-interval-title"
    >
      <button
        class="close"
        aria-label="閉じる"
        disabled={intervalCreating}
        onclick={cancelAddInterval}>×</button
      >
      <h2 id="new-interval-title">バックグラウンド区間を追加</h2>
      <p class="hint">
        セッション範囲：{local(selected.launched_at)} ～ {local(selected.exited_at)}
      </p>
      <form
        class="history-range-form"
        novalidate
        onsubmit={(event) => {
          event.preventDefault();
          addInterval();
        }}
      >
        <div class="history-range-fields">
          <DateTimeSelect
            label="開始日時"
            value={newIntervalStart}
            disabled={intervalCreating}
            invalid={Boolean(newIntervalError)}
            onchange={(value) => {
              newIntervalStart = value;
              newIntervalError = '';
            }}
          />
          <DateTimeSelect
            label="終了日時"
            value={newIntervalEnd}
            disabled={intervalCreating}
            invalid={Boolean(newIntervalError)}
            onchange={(value) => {
              newIntervalEnd = value;
              newIntervalError = '';
            }}
          />
        </div>
        <div class="actions">
          <button type="button" disabled={intervalCreating} onclick={cancelAddInterval}
            >キャンセル</button
          ><button class="primary" type="submit" disabled={intervalCreating}
            >{intervalCreating ? '追加中…' : '追加'}</button
          >
        </div>
        {#if newIntervalError}<p class="form-error" role="alert">{newIntervalError}</p>{/if}
      </form>
    </div>
  </div>{/if}
{#if pageError && !selected}<p class="error">{pageError}</p>{/if}
{#if selectedScreenshot}<div
    class="modal screenshot-modal"
    role="presentation"
    onclick={closeScreenshotViewer}
  >
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <section
      class="screenshot-viewer"
      bind:this={screenshotViewer}
      onclick={(event) => event.stopPropagation()}
    >
      {#if !screenshotOcrAttempted}<button
          class="close"
          aria-label="閉じる"
          onclick={closeScreenshotViewer}>×</button
        >{/if}
      <div class:has-ocr={screenshotOcrAttempted} class="screenshot-viewer-content">
        <div class="screenshot-image-stage">
          <div class="screenshot-image-navigation">
            <button
              type="button"
              class="screenshot-navigation-button"
              aria-label="前のスクリーンショット"
              title="前のスクリーンショット（←）"
              disabled={!screenshotNeighbors.previous || Boolean(screenshotSelectionDraft)}
              onclick={() => navigateScreenshot(-1)}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m15 5-7 7 7 7" /></svg>
            </button>
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="screenshot-selection-surface">
              <img
                src={imageSrc(selectedScreenshot.path)}
                alt="スクリーンショット拡大表示"
                draggable="false"
                onpointerdown={beginScreenshotSelection}
                onpointermove={moveScreenshotSelection}
                onpointerup={finishScreenshotSelection}
                onpointercancel={cancelScreenshotSelection}
              />
              {#if visibleScreenshotOcrRegion}<div
                  class="screenshot-selection-box"
                  style:left={`${visibleScreenshotOcrRegion.x * 100}%`}
                  style:top={`${visibleScreenshotOcrRegion.y * 100}%`}
                  style:width={`${visibleScreenshotOcrRegion.width * 100}%`}
                  style:height={`${visibleScreenshotOcrRegion.height * 100}%`}
                ></div>{/if}
            </div>
            <button
              type="button"
              class="screenshot-navigation-button"
              aria-label="次のスクリーンショット"
              title="次のスクリーンショット（→）"
              disabled={!screenshotNeighbors.next || Boolean(screenshotSelectionDraft)}
              onclick={() => navigateScreenshot(1)}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m9 5 7 7-7 7" /></svg>
            </button>
          </div>
          <p class="screenshot-selection-hint">
            画像上をドラッグすると文字起こしの範囲を選択できます
          </p>
        </div>
        {#if screenshotOcrAttempted}<aside class="screenshot-ocr-panel" aria-live="polite">
            <header class="screenshot-ocr-header">
              <div>
                <h3>文字起こし</h3>
                <small>PaddleOCR・端末内処理</small>
              </div>
              <button
                type="button"
                class="screenshot-ocr-close"
                aria-label="閉じる"
                onclick={closeScreenshotViewer}>×</button
              >
            </header>
            {#if screenshotOcrLoading}
              <p class="screenshot-ocr-status" role="status">画像内のテキストを認識しています…</p>
            {:else if screenshotOcrError}
              <p class="form-error" role="alert">{screenshotOcrError}</p>
            {:else if screenshotOcrText}
              <textarea readonly aria-label="文字起こし結果" value={screenshotOcrText}></textarea>
              <div class="screenshot-ocr-actions">
                <button type="button" class="primary" onclick={copyScreenshotOcrText}
                  >結果をコピー</button
                >
                <button
                  type="button"
                  disabled={screenshotOcrSearching || !screenshotOcrText.trim()}
                  title="既定のブラウザで文字起こし結果をGoogle検索"
                  onclick={searchScreenshotOcrText}>Googleで検索</button
                >
              </div>
            {:else}
              <p class="screenshot-ocr-status">画像内に日本語のテキストを検出できませんでした。</p>
            {/if}
          </aside>{/if}
      </div>
      <footer>
        <span
          ><span class="screenshot-position" role="status"
            >{screenshotNeighbors.index + 1} / {screenshots.length}</span
          >{local(selectedScreenshot.captured_at)} ・ {selectedScreenshot.width}×{selectedScreenshot.height}</span
        >
        <div class="screenshot-footer-actions">
          {#if screenshotOcrRegion}<button
              type="button"
              disabled={screenshotOcrLoading}
              onclick={clearScreenshotSelection}>選択解除</button
            >{/if}
          <button type="button" disabled={screenshotOcrLoading} onclick={recognizeScreenshotText}
            >{screenshotOcrLoading
              ? '文字起こし中…'
              : screenshotOcrAttempted
                ? screenshotOcrRegion
                  ? '選択範囲をもう一度文字に起こす'
                  : '画像全体をもう一度文字に起こす'
                : screenshotOcrRegion
                  ? '選択範囲を文字に起こす'
                  : '画像全体を文字に起こす'}</button
          >
          <DeleteButton
            title="スクリーンショットの削除"
            message={`${local(selectedScreenshot.captured_at)} のスクリーンショットを削除します。画像ファイルも削除され、元に戻せません。`}
            onconfirm={() => deleteScreenshot(selectedScreenshot!.id)}
          />
        </div>
      </footer>
    </section>
  </div>{/if}
{#if toast}<div class:error-toast={toastError} class="toast" role="status">
    <span>{toastError ? '!' : '✓'}</span>
    <p>{toast}</p>
    <button aria-label="通知を閉じる" onclick={() => (toast = '')}>×</button>
  </div>{/if}
