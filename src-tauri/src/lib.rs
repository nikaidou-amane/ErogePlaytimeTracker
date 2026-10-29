mod backup;
mod commands;
mod database;
mod game_ocr;
mod hotkeys;
mod launch;
mod metadata;
mod models;
mod ocr;
mod pause_icons;
mod screenshot;
mod thumbnail;
mod tracking;
mod update_notice;
use crate::{database::Database, models::AppSettings, tracking::TrackingService};
use chrono::Utc;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{
    Manager,
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
};
pub struct AppState {
    db: Database,
    tracker: TrackingService,
    data_root: PathBuf,
    thumbnails: PathBuf,
    screenshots: PathBuf,
    hotkey_service: hotkeys::HotkeyService,
    backup_operations: Arc<parking_lot::Mutex<()>>,
    quitting: AtomicBool,
    update_notices: parking_lot::Mutex<Option<update_notice::UpdateNotices>>,
}
impl AppState {
    fn settings(&self) -> AppSettings {
        self.db.settings().unwrap_or_default()
    }
}
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args(["--autostart"])
                .build(),
        )
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .setup(|app| {
            let dirs = directories::BaseDirs::new()
                .ok_or_else(|| anyhow::anyhow!("Application Data directoryを取得できません"))?;
            let root = dirs.data_local_dir().join("ErogePlaytimeTracker");
            std::fs::create_dir_all(&root)?;
            let existing_installation = root.join("app.db").exists();
            let applied_import = backup::apply_pending_import(&root)?;
            let initialized = (|| -> anyhow::Result<_> {
                let thumbs = root.join("thumbnails");
                std::fs::create_dir_all(&thumbs)?;
                let screenshots = root.join("screenshots");
                std::fs::create_dir_all(&screenshots)?;
                let db = Database::open(&root.join("app.db"))?;
                let last = db
                    .get_setting("last_seen")?
                    .unwrap_or_else(|| Utc::now().to_rfc3339());
                let recovered = db.recover_orphans(&last)?;
                if recovered > 0 {
                    log::warn!(
                        "recovered {recovered} orphan focus intervals; histories require review"
                    )
                };
                let migrated = db.migrate_focus_intervals()?;
                if migrated > 0 {
                    log::info!("migrated {migrated} sessions to background intervals")
                }
                db.set_setting("last_seen", &Utc::now().to_rfc3339())?;
                let mut settings = db.settings()?;
                // Dropping the skip for an already installed release keeps the update
                // prompt from silently ignoring a newer one after an app update.
                let skipped = settings.skipped_update_version.clone();
                settings.prune_skipped_update_version(&app.package_info().version);
                if settings.skipped_update_version != skipped {
                    db.set_setting("app", &serde_json::to_string(&settings)?)?;
                }
                Ok((db, thumbs, screenshots, settings))
            })();
            let (db, thumbs, screenshots, settings) = match initialized {
                Ok(initialized) => initialized,
                Err(error) => {
                    if let Some(applied) = applied_import {
                        backup::rollback_import(&root, applied);
                    }
                    return Err(error.into());
                }
            };
            if let Some(applied) = applied_import {
                use tauri_plugin_autostart::ManagerExt;
                let manager = app.autolaunch();
                let warning = (|| -> anyhow::Result<()> {
                    let enabled = manager.is_enabled()?;
                    if settings.autostart != enabled {
                        if settings.autostart {
                            manager.enable()?;
                        } else {
                            manager.disable()?;
                        }
                    }
                    Ok(())
                })()
                .err()
                .map(|error| {
                    log::warn!("failed to apply imported autostart setting: {error:#}");
                    "Windowsログイン時の自動起動設定だけは反映できなかったため、設定画面で確認してください。"
                });
                backup::finish_import(&root, applied, warning);
            }
            let tracker = TrackingService::start(db.clone(), app.handle().clone());
            let hotkey_service = hotkeys::HotkeyService::start(
                app.handle().clone(),
                db.clone(),
                tracker.clone(),
                screenshots.clone(),
                hotkeys::bindings(&settings),
            );
            let show = MenuItemBuilder::with_id("show", "メインウィンドウを開く").build(app)?;
            let status = MenuItemBuilder::with_id("status", "追跡状態: 待機中")
                .enabled(false)
                .build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "終了").build(app)?;
            let menu = MenuBuilder::new(app)
                .items(&[&show, &status, &quit])
                .build()?;
            TrayIconBuilder::with_id("tracker")
                .icon(tauri::include_image!("icons/32x32.png"))
                .menu(&menu)
                .tooltip("Eroge Playtime Tracker")
                .on_menu_event(|app, e| match e.id().as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => {
                        if let Some(s) = app.try_state::<AppState>() {
                            s.quitting.store(true, Ordering::Relaxed);
                            s.tracker.shutdown();
                        }
                        app.exit(0)
                    }
                    _ => {}
                })
                .build(app)?;
            // Only successful installed-app starts may create/advance notice state.
            // Development runs must not acknowledge the user's installed release.
            let update_notices = if cfg!(debug_assertions) {
                None
            } else {
                update_notice::UpdateNotices::open(
                    &root,
                    app.package_info().version.clone(),
                    existing_installation,
                )
                .map_err(|error| log::warn!("could not initialize update notice: {error:#}"))
                .ok()
            };
            app.manage(AppState {
                db,
                tracker,
                data_root: root,
                thumbnails: thumbs,
                screenshots,
                hotkey_service,
                backup_operations: Arc::new(parking_lot::Mutex::new(())),
                quitting: AtomicBool::new(false),
                update_notices: parking_lot::Mutex::new(update_notices),
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if matches!(event,tauri::WindowEvent::Focused(true)) && let Some(state)=window.try_state::<AppState>() {
                state.tracker.refresh_pause_icons();
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && let Some(s) = window.try_state::<AppState>()
                && !s.quitting.load(Ordering::Relaxed)
                && s.settings().close_to_tray
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_games,
            commands::list_brands,
            commands::get_game,
            commands::create_game,
            commands::update_game,
            commands::update_game_thumbnail,
            commands::update_game_play_status,
            commands::open_external_url,
            commands::delete_game,
            commands::add_game_executable,
            commands::remove_game_executable,
            commands::launch_game,
            commands::set_launch_command,
            commands::list_sessions,
            commands::get_statistics,
            commands::get_game_statistics,
            commands::list_background_intervals,
            commands::create_manual_session,
            commands::update_session,
            commands::confirm_session_review,
            commands::delete_session,
            commands::delete_all_sessions,
            commands::create_background_interval,
            commands::update_background_interval,
            commands::delete_background_interval,
            commands::list_game_timestamps,
            commands::create_game_timestamp,
            commands::update_game_timestamp,
            commands::delete_game_timestamp,
            commands::fetch_game_metadata,
            commands::import_thumbnail,
            commands::save_cropped_thumbnail,
            commands::refresh_game_metadata,
            commands::get_settings,
            commands::update_settings,
            commands::skip_update_version,
            commands::get_update_completion_notice,
            commands::acknowledge_update_completion,
            commands::validate_hotkeys,
            commands::suspend_hotkeys,
            commands::resume_hotkeys,
            commands::export_backup,
            commands::prepare_backup_import,
            commands::confirm_backup_import,
            commands::cancel_backup_import,
            commands::take_backup_import_notice,
            commands::get_tracking_status,
            commands::set_tracking_paused,
            commands::list_tracking_pause_intervals,
            commands::update_tracking_pause_interval,
            commands::delete_tracking_pause_interval,
            commands::list_game_screenshots,
            commands::recognize_screenshot_text,
            commands::delete_game_screenshot,
            commands::open_screenshot_directory
        ])
        .build(tauri::generate_context!())
        .expect("Tauri application failed")
        .run(|app,event| {
            if matches!(event,tauri::RunEvent::Exit) && let Some(state)=app.try_state::<AppState>() {
                state.tracker.shutdown();
            }
        });
}
