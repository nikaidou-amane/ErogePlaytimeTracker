export type SortKey =
  | 'title'
  | 'brand'
  | 'release_date'
  | 'created_at'
  | 'total_playtime'
  | 'last_played'
  | 'session_count';
export type PlayStatus = 'unplayed' | 'playing' | 'completed' | 'retired';
export interface GameSummary {
  id: number;
  title: string;
  brand: string | null;
  release_date: string | null;
  thumbnail_path: string | null;
  created_at: string;
  total_playtime_seconds: number;
  total_running_seconds: number;
  last_played: string | null;
  session_count: number;
  play_status: PlayStatus;
}
export interface Executable {
  id: number;
  game_id: number;
  path: string;
  file_name: string;
  created_at: string;
}
export interface GameDetail extends GameSummary {
  erogamescape_id: number | null;
  vndb_id: string | null;
  metadata_source: MetadataProvider | null;
  source_url: string | null;
  executables: Executable[];
  launch_command: string | null;
}
export interface Session {
  id: number;
  game_id: number;
  launched_at: string;
  exited_at: string | null;
  needs_review: boolean;
  playtime_seconds: number;
  background_seconds: number;
  running_seconds: number | null;
  pause_excluded_seconds: number;
}
export type StatisticsPeriodKind = 'recent' | 'month' | 'year' | 'all';
export interface StatisticsPeriodInput {
  kind: StatisticsPeriodKind;
  year?: number;
  month?: number;
  days?: number;
}
export interface StatisticsSessionHighlight {
  session_id: number;
  game_id: number;
  title: string;
  launched_at: string;
  exited_at: string | null;
  playtime_seconds: number;
  needs_review: boolean;
}
export interface StatisticsDayHighlight {
  date: string;
  playtime_seconds: number;
}
export interface StatisticsSummary {
  total_playtime_seconds: number;
  active_day_count: number;
  average_per_day_seconds: number;
  game_count: number;
  session_count: number;
  average_session_seconds: number;
  longest_session: StatisticsSessionHighlight | null;
  busiest_day: StatisticsDayHighlight | null;
  needs_review_session_count: number;
}
export interface StatisticsDayGame {
  game_id: number;
  title: string;
  thumbnail_path: string | null;
  playtime_seconds: number;
}
export interface StatisticsDay {
  date: string;
  playtime_seconds: number;
  games: StatisticsDayGame[];
}
export interface StatisticsGame {
  game_id: number;
  title: string;
  brand: string | null;
  thumbnail_path: string | null;
  playtime_seconds: number;
  session_count: number;
  active_day_count: number;
}
export interface StatisticsReport {
  period: {
    kind: StatisticsPeriodKind;
    year: number | null;
    month: number | null;
    days: number | null;
    start_date: string;
    end_date: string;
    generated_at: string;
  };
  summary: StatisticsSummary;
  days: StatisticsDay[];
  games: StatisticsGame[];
  available_years: number[];
}
export interface BackgroundInterval {
  id: number;
  play_session_id: number;
  started_at: string;
  ended_at: string | null;
}
export interface GameTimestamp {
  id: number;
  game_id: number;
  name: string;
  marked_at: string;
  playtime_seconds: number;
  since_previous_seconds: number;
}
export interface GameScreenshot {
  id: number;
  game_id: number;
  play_session_id: number | null;
  path: string;
  captured_at: string;
  width: number;
  height: number;
}
export interface ScreenshotOcrResult {
  text: string;
}
export interface ScreenshotOcrRegion {
  x: number;
  y: number;
  width: number;
  height: number;
}
export type MetadataProvider = 'erogamescape' | 'vndb';
export type MetadataIdentity =
  { provider: 'erogamescape'; id: number } | { provider: 'vndb'; id: string };
export interface Metadata {
  source: MetadataIdentity;
  title: string;
  brand: string | null;
  release_date: string | null;
  thumbnail_url: string | null;
  thumbnail_path: string | null;
  warnings: string[];
}
export type Theme = 'dark' | 'light' | 'pink' | 'blue';
export interface Settings {
  autostart: boolean;
  auto_check_updates: boolean;
  skipped_update_version: string | null;
  close_to_tray: boolean;
  exclude_background_time: boolean;
  exclude_pause_time: boolean;
  tracking_pause_hotkey: string;
  theme: Theme;
  screenshot_hotkey: string;
  ocr_search_hotkey: string;
}
export interface TrackingStatus {
  is_paused: boolean;
  pause_started_at: string | null;
  games: Array<{
    game_id: number;
    title: string;
    session_id: number | null;
    phase: 'starting' | 'foreground' | 'background' | 'window_transition';
  }>;
}

export interface BackupDataSummary {
  game_count: number;
  session_count: number;
  timestamp_count: number;
  screenshot_count: number;
  thumbnail_count: number;
}

export interface BackupExportResult {
  destination: string;
  summary: BackupDataSummary;
  includes_screenshots: boolean;
  missing_media_count: number;
  file_size: number;
}

export interface BackupExportProgress {
  phase: 'preparing' | 'archiving' | 'finalizing';
  processed_bytes: number;
  total_bytes: number;
  file_count: number;
}

export interface BackupImportPreview {
  import_id: string;
  exported_at: string;
  app_version: string;
  summary: BackupDataSummary;
  current_summary: BackupDataSummary;
  includes_screenshots: boolean;
  missing_executable_count: number;
  file_size: number;
}

export interface BackupImportNotice {
  success: boolean;
  message: string;
  auto_backup_path: string;
}

export interface ReleaseChanges {
  version: string;
  changes: string[];
}

export interface UpdateCompletionNotice {
  version: string;
  releases: ReleaseChanges[];
}

export interface TrackingPauseInterval {
  id: number;
  started_at: string;
  ended_at: string | null;
  updated_at: string;
}
