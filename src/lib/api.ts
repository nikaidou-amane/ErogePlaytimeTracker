import { Channel, invoke } from '@tauri-apps/api/core';
import { normalizeCommandError } from './errors';
import type {
  BackgroundInterval,
  BackupExportProgress,
  BackupExportResult,
  BackupImportNotice,
  BackupImportPreview,
  GameDetail,
  GameSummary,
  GameTimestamp,
  GameScreenshot,
  ScreenshotOcrResult,
  ScreenshotOcrRegion,
  Metadata,
  MetadataIdentity,
  Session,
  Settings,
  PlayStatus,
  SortKey,
  StatisticsPeriodInput,
  StatisticsReport,
  TrackingStatus,
  TrackingPauseInterval,
  UpdateCompletionNotice,
} from './types';

function command<T>(name: string, args?: Record<string, unknown>) {
  return invoke<T>(name, args).catch((cause) => {
    throw normalizeCommandError(cause);
  });
}

export const api = {
  setTrackingPaused: (paused: boolean) =>
    command<TrackingStatus>('set_tracking_paused', { paused }),
  pauseIntervals: (beforeId: number | null = null) =>
    command<TrackingPauseInterval[]>('list_tracking_pause_intervals', { beforeId, limit: 50 }),
  updatePauseInterval: (id: number, start: string, end: string | null, expectedUpdatedAt: string) =>
    command<void>('update_tracking_pause_interval', { id, start, end, expectedUpdatedAt }),
  deletePauseInterval: (id: number, expectedUpdatedAt: string) =>
    command<void>('delete_tracking_pause_interval', { id, expectedUpdatedAt }),
  listGames: (
    search = '',
    brand = '',
    playStatus = '',
    sort: SortKey = 'last_played',
    descending = true,
  ) =>
    command<GameSummary[]>('list_games', {
      search,
      brand: brand || null,
      playStatus: playStatus || null,
      sort,
      descending,
    }),
  listBrands: () => command<string[]>('list_brands'),
  getGame: (id: number) => command<GameDetail>('get_game', { id }),
  createGame: (input: {
    title: string;
    brand?: string;
    release_date?: string;
    thumbnail_path?: string;
    source?: MetadataIdentity;
    executable_paths: string[];
  }) => command<number>('create_game', { input }),
  updateGame: (
    id: number,
    input: { title: string; brand?: string; release_date?: string; source_url?: string },
  ) => command<void>('update_game', { id, input }),
  updateGameThumbnail: (gameId: number, thumbnailPath: string | null) =>
    command<void>('update_game_thumbnail', { gameId, thumbnailPath }),
  updateGamePlayStatus: (id: number, status: PlayStatus) =>
    command<void>('update_game_play_status', { id, status }),
  openExternalUrl: (url: string) => command<void>('open_external_url', { url }),
  deleteGame: (id: number) => command<void>('delete_game', { id }),
  addExecutable: (gameId: number, path: string) =>
    command<void>('add_game_executable', { gameId, path }),
  removeExecutable: (id: number) => command<void>('remove_game_executable', { id }),
  launchGame: (gameId: number) => command<void>('launch_game', { gameId }),
  setLaunchCommand: (gameId: number, launchCommand: string | null) =>
    command<void>('set_launch_command', { gameId, command: launchCommand }),
  sessions: (gameId: number) => command<Session[]>('list_sessions', { gameId }),
  statistics: (period: StatisticsPeriodInput) =>
    command<StatisticsReport>('get_statistics', { period }),
  gameStatistics: (gameId: number) => command<StatisticsReport>('get_game_statistics', { gameId }),
  intervals: (sessionId: number) =>
    command<BackgroundInterval[]>('list_background_intervals', { sessionId }),
  manualSession: (gameId: number, start: string, end: string) =>
    command<number>('create_manual_session', { gameId, start, end }),
  updateSession: (id: number, start: string, end: string | null) =>
    command<void>('update_session', { id, start, end }),
  confirmSessionReview: (id: number) => command<void>('confirm_session_review', { id }),
  deleteSession: (id: number) => command<void>('delete_session', { id }),
  deleteAllSessions: (gameId: number) => command<number>('delete_all_sessions', { gameId }),
  createInterval: (sessionId: number, start: string, end: string) =>
    command<number>('create_background_interval', { sessionId, start, end }),
  updateInterval: (id: number, start: string, end: string | null) =>
    command<void>('update_background_interval', { id, start, end }),
  deleteInterval: (id: number) => command<void>('delete_background_interval', { id }),
  timestamps: (gameId: number) => command<GameTimestamp[]>('list_game_timestamps', { gameId }),
  createTimestamp: (gameId: number, name: string) =>
    command<number>('create_game_timestamp', { gameId, name }),
  updateTimestamp: (id: number, name: string, markedAt: string) =>
    command<void>('update_game_timestamp', { id, name, markedAt }),
  deleteTimestamp: (id: number) => command<void>('delete_game_timestamp', { id }),
  screenshots: (gameId: number) => command<GameScreenshot[]>('list_game_screenshots', { gameId }),
  recognizeScreenshotText: (id: number, region: ScreenshotOcrRegion | null = null) =>
    command<ScreenshotOcrResult>('recognize_screenshot_text', { id, region }),
  deleteScreenshot: (id: number) => command<void>('delete_game_screenshot', { id }),
  openScreenshotDirectory: (gameId: number) =>
    command<void>('open_screenshot_directory', { gameId }),
  fetchMetadata: (value: string) => command<Metadata>('fetch_game_metadata', { value }),
  importThumbnail: (path: string) => command<string>('import_thumbnail', { path }),
  saveCroppedThumbnail: (pngBase64: string) =>
    command<string>('save_cropped_thumbnail', { pngBase64 }),
  refreshMetadata: (gameId: number) => command<string[]>('refresh_game_metadata', { gameId }),
  settings: () => command<Settings>('get_settings'),
  updateSettings: (settings: Settings) => command<void>('update_settings', { settings }),
  skipUpdateVersion: (version: string) => command<void>('skip_update_version', { version }),
  updateCompletionNotice: () =>
    command<UpdateCompletionNotice | null>('get_update_completion_notice'),
  acknowledgeUpdateCompletion: (version: string) =>
    command<void>('acknowledge_update_completion', { version }),
  validateHotkeys: (
    settings: Pick<Settings, 'screenshot_hotkey' | 'ocr_search_hotkey' | 'tracking_pause_hotkey'>,
  ) =>
    command<void>('validate_hotkeys', {
      screenshotHotkey: settings.screenshot_hotkey,
      ocrSearchHotkey: settings.ocr_search_hotkey,
      trackingPauseHotkey: settings.tracking_pause_hotkey,
    }),
  suspendHotkeys: () => command<void>('suspend_hotkeys'),
  resumeHotkeys: () => command<void>('resume_hotkeys'),
  exportBackup: (
    destination: string,
    includeScreenshots: boolean,
    onProgress: (progress: BackupExportProgress) => void,
  ) => {
    const progressChannel = new Channel<BackupExportProgress>();
    progressChannel.onmessage = onProgress;
    return command<BackupExportResult>('export_backup', {
      destination,
      includeScreenshots,
      onProgress: progressChannel,
    });
  },
  prepareBackupImport: (source: string) =>
    command<BackupImportPreview>('prepare_backup_import', { source }),
  confirmBackupImport: (importId: string, onProgress: (progress: BackupExportProgress) => void) => {
    const progressChannel = new Channel<BackupExportProgress>();
    progressChannel.onmessage = onProgress;
    return command<void>('confirm_backup_import', { importId, onProgress: progressChannel });
  },
  cancelBackupImport: (importId: string) => command<void>('cancel_backup_import', { importId }),
  takeBackupImportNotice: () => command<BackupImportNotice | null>('take_backup_import_notice'),
  status: () => command<TrackingStatus>('get_tracking_status'),
};
