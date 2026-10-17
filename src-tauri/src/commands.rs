use crate::{AppState, models::*};
use tauri::{State, ipc::Channel};
type Cmd<T> = Result<T, String>;

const USER_ERROR_PREFIX: &str = "ept:user-error:";
const GENERIC_ERROR: &str = "処理を完了できませんでした。しばらくしてからもう一度お試しください。";

fn user_error(message: impl std::fmt::Display) -> String {
    format!("{USER_ERROR_PREFIX}{message}")
}

fn err(e: impl std::fmt::Display) -> String {
    let detail = e.to_string();
    if let Some(message) = known_error_message(&detail) {
        return user_error(message);
    }
    log::error!("command failed: {detail}");
    user_error(GENERIC_ERROR)
}

fn known_error_message(detail: &str) -> Option<&str> {
    if detail.contains("UNIQUE constraint failed: game_executables.path") {
        return Some("この実行ファイルはすでに登録されています。");
    }
    if detail.contains("UNIQUE constraint failed: games.erogamescape_id") {
        return Some("このErogameScape IDのゲームはすでに登録されています。");
    }
    if detail.contains("UNIQUE constraint failed: games.vndb_id") {
        return Some("このVNDB IDのゲームはすでに登録されています。");
    }
    if detail.starts_with("未対応の修飾キーです:") || detail.starts_with("未対応のキーです:")
    {
        return Some("ショートカットキーの形式が正しくありません。");
    }
    match detail {
        "バックグラウンド区間がSession範囲外になります" => {
            return Some(
                "開始・終了日時には、すべてのバックグラウンド区間を含む範囲を指定してください。",
            );
        }
        "区間はSession範囲内にしてください" => {
            return Some(
                "バックグラウンド区間はセッションの開始・終了日時の範囲内で入力してください。",
            );
        }
        "区間が既存の区間と重複しています" => {
            return Some("既存のバックグラウンド区間と重複しない日時を入力してください。");
        }
        _ => {}
    }

    const KNOWN_MESSAGES: &[&str] = &[
        "タイトルは必須です",
        "ゲームが見つかりません",
        "起動する実行ファイルが登録されていません",
        "情報取得元のURL・IDがありません",
        "ErogameScapeまたはVNDBのURL・IDを入力してください",
        "VNDBのゲームが見つかりません",
        "VNDBへ接続できません",
        "VNDBがエラーを返しました",
        "VNDBの応答を解析できません",
        "VNDBの取得回数制限に達しました。しばらく待ってからお試しください。",
        "サムネイル画像が見つかりません",
        "サムネイル画像を読み取れません",
        "トリミング画像がPNG形式ではありません",
        "ショートカットキーを入力してください",
        "撮影用とOCR検索用には別のキーを設定してください。",
        "ショートカットキーを復元できませんでした。設定を確認してください。",
        "このキーは別のアプリで使用されています",
        "開始日時が不正です",
        "終了日時が不正です",
        "終了日時は開始日時以降にしてください",
        "セッションの実行状態は手動変更できません",
        "セッションが見つかりません",
        "バックグラウンド区間の記録状態は手動変更できません",
        "未対応のプレイ状況です",
        "タイムスタンプのタイトルを入力してください",
        "タイムスタンプのタイトルは100文字以内にしてください",
        "タイムスタンプが見つかりません",
        "記録日時が不正です",
        "実行ファイルパスが空です",
        "実行ファイル名を取得できません",
        "サムネイルはJPG、PNG、WebPを選択してください",
        "タイトルを解析できません",
        "ErogameScapeへ接続できません",
        "ErogameScapeがエラーを返しました",
        "ErogameScapeの応答が空です",
        "年を指定してください",
        "月を指定してください",
        "統計期間の年月が不正です",
        "未来の月は選択できません",
        "統計期間の年が不正です",
        "未来の年は選択できません",
        "統計期間の種類が不正です",
    ];
    KNOWN_MESSAGES
        .iter()
        .copied()
        .find(|message| detail == *message)
}
#[tauri::command]
pub fn list_games(
    state: State<AppState>,
    search: String,
    brand: Option<String>,
    play_status: Option<String>,
    sort: String,
    descending: bool,
) -> Cmd<Vec<GameSummary>> {
    state
        .db
        .list_games(
            &search,
            brand.as_deref(),
            play_status.as_deref(),
            &sort,
            descending,
        )
        .map_err(err)
}
#[tauri::command]
pub fn list_brands(state: State<AppState>) -> Cmd<Vec<String>> {
    state.db.list_brands().map_err(err)
}
#[tauri::command]
pub fn get_game(state: State<AppState>, id: i64) -> Cmd<GameDetail> {
    state.db.get_game(id).map_err(err)
}
#[tauri::command]
pub fn create_game(state: State<AppState>, input: CreateGameInput) -> Cmd<i64> {
    let local_thumbnail = input
        .thumbnail_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty());
    let thumb = local_thumbnail
        .map(|path| {
            crate::thumbnail::import_local(path, &state.thumbnails)
                .map(|path| path.to_string_lossy().to_string())
        })
        .transpose()
        .map_err(err)?;
    state.db.create_game(&input, thumb.as_deref()).map_err(err)
}
#[tauri::command]
pub fn update_game(state: State<AppState>, id: i64, input: UpdateGameInput) -> Cmd<()> {
    state.db.update_game(id, &input).map_err(err)
}
#[tauri::command]
pub fn update_game_thumbnail(
    state: State<AppState>,
    game_id: i64,
    thumbnail_path: Option<String>,
) -> Cmd<()> {
    let local_thumbnail = thumbnail_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty());
    let stored_thumbnail = local_thumbnail
        .map(|path| {
            crate::thumbnail::import_local(path, &state.thumbnails)
                .map(|path| path.to_string_lossy().to_string())
        })
        .transpose()
        .map_err(err)?;
    state
        .db
        .update_game_thumbnail(game_id, stored_thumbnail.as_deref())
        .map_err(err)
}
#[tauri::command]
pub fn update_game_play_status(state: State<AppState>, id: i64, status: String) -> Cmd<()> {
    state.db.update_play_status(id, &status).map_err(err)
}
#[tauri::command]
pub fn open_external_url(url: String) -> Cmd<()> {
    let parsed = url::Url::parse(&url).map_err(|_| user_error("URLの形式が正しくありません。"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(user_error("HTTPまたはHTTPSのURLのみ開けます。"));
    }
    let operation: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
    let target: Vec<u16> = parsed
        .as_str()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    use windows::{
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
        core::PCWSTR,
    };
    // ShellExecuteW uses the user's Windows URL association, which opens HTTP(S)
    // targets in the configured default browser. The UTF-16 buffers live for the call.
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(operation.as_ptr()),
            PCWSTR(target.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        log::error!("ShellExecuteW failed with code {}", result.0 as isize);
        Err(user_error("既定のブラウザを開けませんでした。"))
    } else {
        Ok(())
    }
}
#[tauri::command]
pub fn delete_game(state: State<AppState>, id: i64) -> Cmd<()> {
    state.db.delete_game(id).map_err(err)?;
    let directory = state.screenshots.join(id.to_string());
    if directory.exists() {
        std::fs::remove_dir_all(directory).map_err(err)?;
    }
    Ok(())
}
#[tauri::command]
pub fn add_game_executable(state: State<AppState>, game_id: i64, path: String) -> Cmd<()> {
    state.db.add_executable(game_id, &path).map_err(err)
}
#[tauri::command]
pub fn remove_game_executable(state: State<AppState>, id: i64) -> Cmd<()> {
    state.db.remove_executable(id).map_err(err)
}
#[tauri::command]
pub fn launch_game(state: State<AppState>, game_id: i64) -> Cmd<()> {
    let path = state.db.launcher_path(game_id).map_err(err)?;
    let executable = std::path::Path::new(&path);
    if !executable.is_file() {
        log::warn!("registered executable not found: {path}");
        return Err(user_error(
            "実行ファイルが見つかりません。登録内容を確認してください。",
        ));
    }
    let mut command = std::process::Command::new(executable);
    if let Some(directory) = executable.parent() {
        command.current_dir(directory);
    }
    command.spawn().map(|_| ()).map_err(|e| {
        log::error!("failed to launch registered game executable: {e}");
        user_error("ゲームを起動できませんでした。実行ファイルを確認してください。")
    })
}
#[tauri::command]
pub fn list_sessions(state: State<AppState>, game_id: i64) -> Cmd<Vec<PlaySession>> {
    state.db.list_sessions(game_id).map_err(err)
}
#[tauri::command]
pub fn get_statistics(
    state: State<AppState>,
    period: StatisticsPeriodInput,
) -> Cmd<StatisticsReport> {
    state.db.statistics(&period).map_err(err)
}
#[tauri::command]
pub fn get_game_statistics(state: State<AppState>, game_id: i64) -> Cmd<StatisticsReport> {
    state.db.game_statistics(game_id).map_err(err)
}
#[tauri::command]
pub fn list_background_intervals(
    state: State<AppState>,
    session_id: i64,
) -> Cmd<Vec<BackgroundInterval>> {
    state.db.intervals(session_id).map_err(err)
}
#[tauri::command]
pub fn create_manual_session(
    state: State<AppState>,
    game_id: i64,
    start: String,
    end: String,
) -> Cmd<i64> {
    state.db.manual_session(game_id, &start, &end).map_err(err)
}
#[tauri::command]
pub fn update_session(
    state: State<AppState>,
    id: i64,
    start: String,
    end: Option<String>,
) -> Cmd<()> {
    state
        .db
        .update_session(id, &start, end.as_deref())
        .map_err(err)
}
#[tauri::command]
pub fn confirm_session_review(state: State<AppState>, id: i64) -> Cmd<()> {
    state.db.confirm_session_review(id).map_err(err)
}
#[tauri::command]
pub fn delete_session(state: State<AppState>, id: i64) -> Cmd<()> {
    state.db.delete_session(id).map_err(err)
}
#[tauri::command]
pub fn delete_all_sessions(state: State<AppState>, game_id: i64) -> Cmd<usize> {
    state.db.delete_game_sessions(game_id).map_err(err)
}
#[tauri::command]
pub fn create_background_interval(
    state: State<AppState>,
    session_id: i64,
    start: String,
    end: String,
) -> Cmd<i64> {
    state
        .db
        .create_interval(session_id, &start, &end)
        .map_err(err)
}
#[tauri::command]
pub fn update_background_interval(
    state: State<AppState>,
    id: i64,
    start: String,
    end: Option<String>,
) -> Cmd<()> {
    state
        .db
        .update_interval(id, &start, end.as_deref())
        .map_err(err)
}
#[tauri::command]
pub fn delete_background_interval(state: State<AppState>, id: i64) -> Cmd<()> {
    state.db.delete_interval(id).map_err(err)
}
#[tauri::command]
pub fn list_game_timestamps(state: State<AppState>, game_id: i64) -> Cmd<Vec<GameTimestamp>> {
    state.db.timestamps(game_id).map_err(err)
}
#[tauri::command]
pub fn create_game_timestamp(state: State<AppState>, game_id: i64, name: String) -> Cmd<i64> {
    state
        .db
        .create_timestamp(game_id, &name, &chrono::Utc::now().to_rfc3339())
        .map_err(err)
}
#[tauri::command]
pub fn update_game_timestamp(
    state: State<AppState>,
    id: i64,
    name: String,
    marked_at: String,
) -> Cmd<()> {
    state
        .db
        .update_timestamp(id, &name, &marked_at)
        .map_err(err)
}
#[tauri::command]
pub fn delete_game_timestamp(state: State<AppState>, id: i64) -> Cmd<()> {
    state.db.delete_timestamp(id).map_err(err)
}
#[tauri::command]
pub async fn fetch_game_metadata(
    state: State<'_, AppState>,
    value: String,
) -> Cmd<MetadataPreview> {
    metadata_preview(&state, &value).await.map_err(err)
}

async fn metadata_preview(state: &AppState, value: &str) -> anyhow::Result<MetadataPreview> {
    let mut metadata = crate::metadata::fetch_game(value).await.inspect_err(|e| {
        log::warn!("metadata fetch/parse failed: {e:#}");
    })?;
    let thumbnail_path = if let Some(url) = &metadata.thumbnail_url {
        match crate::thumbnail::download(&state.http, url, &state.thumbnails).await {
            Ok(path) => Some(path.to_string_lossy().to_string()),
            Err(error) => {
                log::warn!("thumbnail preview download failed: {error:#}");
                metadata.warnings.push(
                    "サムネイルを取得できませんでした。画像以外の情報は利用できます。".into(),
                );
                None
            }
        }
    } else {
        None
    };
    Ok(MetadataPreview {
        metadata,
        thumbnail_path,
    })
}
#[tauri::command]
pub fn import_thumbnail(state: State<AppState>, path: String) -> Cmd<String> {
    crate::thumbnail::import_local(path.trim(), &state.thumbnails)
        .map(|path| path.to_string_lossy().to_string())
        .map_err(err)
}
#[tauri::command]
pub fn save_cropped_thumbnail(state: State<AppState>, png_base64: String) -> Cmd<String> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let bytes = STANDARD.decode(png_base64).map_err(err)?;
    crate::thumbnail::store_png(&bytes, &state.thumbnails)
        .map(|path| path.to_string_lossy().to_string())
        .map_err(err)
}
#[tauri::command]
pub async fn refresh_game_metadata(state: State<'_, AppState>, game_id: i64) -> Cmd<Vec<String>> {
    let source = state.db.metadata_identity(game_id).map_err(err)?;
    let preview = metadata_preview(&state, &source.url()).await.map_err(err)?;
    state
        .db
        .apply_metadata(
            game_id,
            &preview.metadata,
            preview.thumbnail_path.as_deref(),
        )
        .map_err(err)?;
    Ok(preview.metadata.warnings)
}
#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Cmd<AppSettings> {
    Ok(state.settings())
}
#[tauri::command]
pub fn get_update_completion_notice(state: State<AppState>) -> Cmd<Option<UpdateCompletionNotice>> {
    Ok(state
        .update_notices
        .lock()
        .as_ref()
        .and_then(|notices| notices.pending()))
}
#[tauri::command]
pub fn acknowledge_update_completion(state: State<AppState>, version: String) -> Cmd<()> {
    if let Some(notices) = state.update_notices.lock().as_mut() {
        notices.acknowledge(&version).map_err(err)?;
    }
    Ok(())
}
#[tauri::command]
pub fn update_settings(
    app: tauri::AppHandle,
    state: State<AppState>,
    mut settings: AppSettings,
) -> Cmd<()> {
    if !matches!(settings.theme.as_str(), "dark" | "light" | "pink" | "blue") {
        return Err(user_error("未対応のカラーテーマです。"));
    }
    let previous = crate::hotkeys::bindings(&state.settings());
    let hotkeys = crate::hotkeys::bindings(&settings);
    state
        .hotkey_service
        .check_hotkeys(hotkeys.clone())
        .map_err(err)?;
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    let autostart_enabled = manager.is_enabled().map_err(err)?;
    if settings.autostart != autostart_enabled {
        if settings.autostart {
            manager.enable().map_err(err)?
        } else {
            manager.disable().map_err(err)?
        }
    }
    state.hotkey_service.set_hotkeys(hotkeys).map_err(err)?;
    // A skip can be saved by the update prompt while the settings screen is open.
    // Preserve the latest value instead of overwriting it with the screen's snapshot.
    settings.skipped_update_version = state.settings().skipped_update_version;
    let result = state
        .db
        .set_setting("app", &serde_json::to_string(&settings).map_err(err)?)
        .map_err(err);
    if result.is_err() {
        state.hotkey_service.set_hotkeys(previous).map_err(err)?;
    }
    result
}
#[tauri::command]
pub fn skip_update_version(state: State<AppState>, version: String) -> Cmd<()> {
    let mut settings = state.settings();
    settings.skipped_update_version = Some(version);
    state
        .db
        .set_setting("app", &serde_json::to_string(&settings).map_err(err)?)
        .map_err(err)
}
#[tauri::command]
pub fn validate_hotkeys(
    state: State<AppState>,
    screenshot_hotkey: String,
    ocr_search_hotkey: String,
) -> Cmd<()> {
    state
        .hotkey_service
        .check_hotkeys([screenshot_hotkey, ocr_search_hotkey])
        .map_err(err)
}
#[tauri::command]
pub fn suspend_hotkeys(state: State<AppState>) -> Cmd<()> {
    state
        .hotkey_service
        .set_hotkeys(Default::default())
        .map_err(err)
}
#[tauri::command]
pub fn resume_hotkeys(state: State<AppState>) -> Cmd<()> {
    state
        .hotkey_service
        .set_hotkeys(crate::hotkeys::bindings(&state.settings()))
        .map_err(err)
}

fn ensure_backup_idle(state: &AppState) -> Cmd<()> {
    if state.tracker.status().games.is_empty() {
        Ok(())
    } else {
        Err(user_error(
            "起動中のゲームを終了してからバックアップ操作を行ってください。",
        ))
    }
}

fn backup_operation_error(error: anyhow::Error, message: &str) -> String {
    let detail = error.to_string();
    log::error!("backup operation failed: {error:#}");
    if detail.contains("新しいバージョンで作成") {
        return user_error(
            "このバックアップは新しいバージョンのアプリで作成されています。アプリを更新してからお試しください。",
        );
    }
    if detail.contains("データフォルダー内にはバックアップを保存できません")
    {
        return user_error("アプリの内部データフォルダー以外の場所を保存先に選んでください。");
    }
    user_error(message)
}

#[tauri::command]
pub async fn export_backup(
    state: State<'_, AppState>,
    destination: String,
    include_screenshots: bool,
    on_progress: Channel<BackupExportProgress>,
) -> Cmd<BackupExportResult> {
    ensure_backup_idle(&state)?;
    let database = state.db.clone();
    let data_root = state.data_root.clone();
    let tracker = state.tracker.clone();
    let operations = state.backup_operations.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(_operation) = operations.try_lock() else {
            return Err(user_error("別のバックアップ操作を実行中です。"));
        };
        if !tracker.status().games.is_empty() {
            return Err(user_error(
                "起動中のゲームを終了してからバックアップ操作を行ってください。",
            ));
        }
        crate::backup::export_backup(
            &database,
            &data_root,
            std::path::Path::new(destination.trim()),
            false,
            include_screenshots,
            |progress| {
                let _ = on_progress.send(progress);
            },
        )
        .map_err(|error| backup_operation_error(error, "バックアップを作成できませんでした。"))
    })
    .await
    .map_err(err)?
}

#[tauri::command]
pub async fn prepare_backup_import(
    state: State<'_, AppState>,
    source: String,
) -> Cmd<BackupImportPreview> {
    ensure_backup_idle(&state)?;
    let database = state.db.clone();
    let data_root = state.data_root.clone();
    let tracker = state.tracker.clone();
    let operations = state.backup_operations.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(_operation) = operations.try_lock() else {
            return Err(user_error("別のバックアップ操作を実行中です。"));
        };
        if !tracker.status().games.is_empty() {
            return Err(user_error(
                "起動中のゲームを終了してからバックアップ操作を行ってください。",
            ));
        }
        crate::backup::prepare_import(
            &database,
            &data_root,
            std::path::Path::new(source.trim()),
        )
        .map_err(|error| {
            backup_operation_error(
                error,
                "バックアップを読み込めませんでした。ファイルが破損しているか、対応していない形式です。",
            )
        })
    })
    .await
    .map_err(err)?
}

#[tauri::command]
pub async fn confirm_backup_import(
    state: State<'_, AppState>,
    import_id: String,
    on_progress: Channel<BackupExportProgress>,
) -> Cmd<()> {
    ensure_backup_idle(&state)?;
    let database = state.db.clone();
    let data_root = state.data_root.clone();
    let tracker = state.tracker.clone();
    let operations = state.backup_operations.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(_operation) = operations.try_lock() else {
            return Err(user_error("別のバックアップ操作を実行中です。"));
        };
        if !tracker.status().games.is_empty() {
            return Err(user_error(
                "起動中のゲームを終了してからバックアップ操作を行ってください。",
            ));
        }
        crate::backup::confirm_import(&database, &data_root, &import_id, |progress| {
            let _ = on_progress.send(progress);
        })
            .map_err(|error| backup_operation_error(error, "インポートを開始できませんでした。"))?;
        if !tracker.status().games.is_empty() {
            let _ = crate::backup::cancel_import(&data_root, &import_id);
            return Err(user_error(
                "ゲームの起動を検出したため、インポートを取り消しました。ゲームを終了してからもう一度お試しください。",
            ));
        }
        Ok(())
    })
    .await
    .map_err(err)?
}

#[tauri::command]
pub async fn cancel_backup_import(state: State<'_, AppState>, import_id: String) -> Cmd<()> {
    let data_root = state.data_root.clone();
    let operations = state.backup_operations.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(_operation) = operations.try_lock() else {
            return Err(user_error("別のバックアップ操作を実行中です。"));
        };
        crate::backup::cancel_import(&data_root, &import_id)
            .map_err(|error| backup_operation_error(error, "インポートを取り消せませんでした。"))
    })
    .await
    .map_err(err)?
}

#[tauri::command]
pub fn take_backup_import_notice(state: State<AppState>) -> Cmd<Option<BackupImportNotice>> {
    crate::backup::take_import_notice(&state.data_root)
        .map_err(|error| backup_operation_error(error, "インポート結果を確認できませんでした。"))
}

#[tauri::command]
pub fn get_tracking_status(state: State<AppState>) -> TrackingStatus {
    state.tracker.status()
}

#[tauri::command]
pub fn list_game_screenshots(state: State<AppState>, game_id: i64) -> Cmd<Vec<GameScreenshot>> {
    state.db.screenshots(game_id).map_err(err)
}

#[tauri::command]
pub async fn recognize_screenshot_text(
    state: State<'_, AppState>,
    id: i64,
    region: Option<ScreenshotOcrRegion>,
) -> Cmd<ScreenshotOcrResult> {
    let path = state
        .db
        .screenshot_path(id)
        .map_err(err)?
        .ok_or_else(|| user_error("スクリーンショットが見つかりません。"))?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::ocr::recognize_japanese_text(std::path::Path::new(&path), region)
    })
    .await
    .map_err(err)?
    .map_err(|error| {
        log::error!("screenshot OCR failed: {error:#}");
        user_error(error.user_message())
    })
}

#[tauri::command]
pub fn delete_game_screenshot(state: State<AppState>, id: i64) -> Cmd<()> {
    if let Some(path) = state.db.remove_screenshot(id).map_err(err)? {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(err(e)),
        }
    } else {
        Ok(())
    }
}

#[tauri::command]
pub fn open_screenshot_directory(state: State<AppState>, game_id: i64) -> Cmd<()> {
    // Confirm the game exists before creating anything under its screenshot root.
    state.db.get_game(game_id).map_err(err)?;
    let directory = state.screenshots.join(game_id.to_string());
    std::fs::create_dir_all(&directory).map_err(err)?;
    let operation: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
    let target: Vec<u16> = directory
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
        core::PCWSTR,
    };
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(operation.as_ptr()),
            PCWSTR(target.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        log::error!("failed to open screenshot directory: ShellExecuteW returned {result:?}");
        Err(user_error("スクリーンショットの保存先を開けませんでした。"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_validation_and_unique_errors_to_public_messages() {
        assert_eq!(
            err("終了日時は開始日時以降にしてください"),
            user_error("終了日時は開始日時以降にしてください")
        );
        assert_eq!(
            err("UNIQUE constraint failed: game_executables.path"),
            user_error("この実行ファイルはすでに登録されています。")
        );
        assert_eq!(
            err("UNIQUE constraint failed: games.erogamescape_id"),
            user_error("このErogameScape IDのゲームはすでに登録されています。")
        );
        assert_eq!(
            err("UNIQUE constraint failed: games.vndb_id"),
            user_error("このVNDB IDのゲームはすでに登録されています。")
        );
        assert_eq!(
            err("区間が既存の区間と重複しています"),
            user_error("既存のバックグラウンド区間と重複しない日時を入力してください。")
        );
    }

    #[test]
    fn replaces_unexpected_internal_errors() {
        let message = err("database failure in play_sessions at a private path");
        assert_eq!(message, user_error(GENERIC_ERROR));
        assert!(!message.contains("play_sessions"));
        assert!(!message.contains("private path"));
    }
}
