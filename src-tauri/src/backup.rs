use crate::{
    database::{CURRENT_SCHEMA_VERSION, Database, close_open_pauses, validate_pause_records},
    models::{
        AppSettings, BackupDataSummary, BackupExportProgress, BackupExportResult,
        BackupImportNotice, BackupImportPreview,
    },
};
use anyhow::{Context, Result, ensure};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BACKUP_FORMAT: &str = "eroge-playtime-tracker-backup";
const BACKUP_FORMAT_VERSION: u32 = 1;
const MANIFEST_PATH: &str = "manifest.json";
const DATABASE_PATH: &str = "app.db";
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_BACKUP_BYTES: u64 = 256 * 1024 * 1024 * 1024;
const PENDING_IMPORT_FILE: &str = "pending-import.json";
const IMPORT_NOTICE_FILE: &str = "import-notice.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackupManifest {
    format: String,
    format_version: u32,
    app_version: String,
    schema_version: i64,
    exported_at: String,
    #[serde(default = "default_true")]
    includes_screenshots: bool,
    summary: BackupDataSummary,
    files: Vec<ManifestFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestFile {
    path: String,
    size: u64,
    sha256: String,
}

struct ExportFile {
    path: String,
    size: u64,
    source: PathBuf,
}

struct PreparedSnapshot {
    summary: BackupDataSummary,
    media: Vec<(String, PathBuf)>,
    missing_media_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PendingPhase {
    Prepared,
    OldMoved,
    NewMoved,
    RollingBack,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingImport {
    import_id: String,
    phase: PendingPhase,
    auto_backup_path: String,
}

pub(crate) struct AppliedImport {
    pending: PendingImport,
}

pub(crate) fn export_backup(
    database: &Database,
    data_root: &Path,
    requested_destination: &Path,
    allow_inside_data_root: bool,
    includes_screenshots: bool,
    mut on_progress: impl FnMut(BackupExportProgress),
) -> Result<BackupExportResult> {
    on_progress(export_progress("preparing", 0, 0, 0));
    let destination = backup_destination(requested_destination);
    let parent = destination
        .parent()
        .context("バックアップの保存先フォルダーを特定できません")?;
    ensure!(
        parent.is_dir(),
        "バックアップの保存先フォルダーがありません"
    );
    if !allow_inside_data_root {
        let data_root = data_root.canonicalize()?;
        let parent = parent.canonicalize()?;
        ensure!(
            !parent.starts_with(data_root),
            "アプリのデータフォルダー内にはバックアップを保存できません"
        );
    }

    let operation_id = operation_id();
    let work = data_root.join("backup-work").join(&operation_id);
    let partial = parent.join(format!(".eptbackup-{operation_id}.partial"));
    fs::create_dir_all(&work)?;
    let result = (|| {
        let snapshot = work.join(DATABASE_PATH);
        let snapshot_at = database.snapshot_to(&snapshot)?;
        let prepared =
            prepare_snapshot_for_export(&snapshot, data_root, includes_screenshots, &snapshot_at)?;

        let mut files = Vec::with_capacity(prepared.media.len() + 1);
        files.push(export_file(DATABASE_PATH.into(), snapshot)?);
        for (archive_path, source) in prepared.media {
            files.push(export_file(archive_path, source)?);
        }
        files.sort_by(|left, right| left.path.cmp(&right.path));
        let total_bytes = files.iter().try_fold(0_u64, |total, file| {
            total
                .checked_add(file.size)
                .context("バックアップサイズが不正です")
        })?;
        on_progress(export_progress("archiving", 0, total_bytes, files.len()));

        let mut manifest = BackupManifest {
            format: BACKUP_FORMAT.into(),
            format_version: BACKUP_FORMAT_VERSION,
            app_version: env!("CARGO_PKG_VERSION").into(),
            schema_version: CURRENT_SCHEMA_VERSION,
            exported_at: snapshot_at,
            includes_screenshots,
            summary: prepared.summary.clone(),
            files: Vec::with_capacity(files.len()),
        };
        write_archive(
            &partial,
            &mut manifest,
            &files,
            total_bytes,
            &mut on_progress,
        )?;
        on_progress(export_progress(
            "finalizing",
            total_bytes,
            total_bytes,
            files.len(),
        ));
        replace_file(&partial, &destination, &operation_id)?;
        let file_size = destination.metadata()?.len();
        Ok(BackupExportResult {
            destination: destination.to_string_lossy().into_owned(),
            summary: prepared.summary,
            includes_screenshots,
            missing_media_count: prepared.missing_media_count,
            file_size,
        })
    })();
    let _ = fs::remove_file(&partial);
    let _ = fs::remove_dir_all(&work);
    result
}

pub(crate) fn prepare_import(
    database: &Database,
    data_root: &Path,
    source: &Path,
) -> Result<BackupImportPreview> {
    ensure!(source.is_file(), "バックアップファイルがありません");
    let file_size = source.metadata()?.len();
    ensure!(
        file_size <= MAX_BACKUP_BYTES,
        "バックアップファイルが大きすぎます"
    );
    let import_id = operation_id();
    let stage = stage_root(data_root, &import_id);
    let payload = stage.join("payload");
    fs::create_dir_all(&payload)?;

    let result = (|| {
        let manifest = extract_and_validate_archive(source, &payload)?;
        fs::create_dir_all(payload.join("thumbnails"))?;
        fs::create_dir_all(payload.join("screenshots"))?;
        ensure!(
            manifest.schema_version <= CURRENT_SCHEMA_VERSION,
            "このアプリより新しいバージョンで作成されたバックアップです"
        );
        let database_path = payload.join(DATABASE_PATH);
        let connection = Connection::open(&database_path)?;
        validate_database(&connection)?;
        let schema_version: i64 = connection.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?;
        ensure!(
            schema_version == manifest.schema_version,
            "バックアップのスキーマ情報が一致しません"
        );
        drop(connection);

        // Opening through Database applies all supported numbered migrations.
        let migrated = Database::open(&database_path)?;
        drop(migrated);
        let connection = Connection::open(&database_path)?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        let manifest_paths: HashSet<&str> = manifest
            .files
            .iter()
            .map(|entry| entry.path.as_str())
            .collect();
        validate_pause_records(&connection)?;
        rewrite_imported_paths(&connection, &payload, data_root, &manifest_paths)?;
        reset_machine_specific_state(&connection)?;
        validate_database(&connection)?;
        let summary = database_summary(&connection)?;
        ensure!(
            manifest.includes_screenshots || summary.screenshot_count == 0,
            "スクリーンショット除外の指定とデータが一致しません"
        );
        ensure!(
            summary == manifest.summary,
            "バックアップ内のデータ件数が一致しません"
        );
        let missing_executable_count = missing_executable_count(&connection)?;
        connection.pragma_update(None, "journal_mode", "DELETE")?;
        drop(connection);

        let preview = BackupImportPreview {
            import_id: import_id.clone(),
            exported_at: manifest.exported_at,
            app_version: manifest.app_version,
            summary,
            current_summary: database.backup_summary()?,
            includes_screenshots: manifest.includes_screenshots,
            missing_executable_count,
            file_size,
        };
        write_json_atomic(&stage.join("preview.json"), &preview)?;
        Ok(preview)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}

pub(crate) fn confirm_import(
    database: &Database,
    data_root: &Path,
    import_id: &str,
    on_progress: impl FnMut(BackupExportProgress),
) -> Result<()> {
    validate_import_id(import_id)?;
    ensure!(
        !data_root.join(PENDING_IMPORT_FILE).exists()
            && !data_root
                .join(PENDING_IMPORT_FILE)
                .with_extension("previous")
                .exists(),
        "別のインポートがすでに予約されています"
    );
    let stage = stage_root(data_root, import_id);
    let _: BackupImportPreview = read_json(&stage.join("preview.json"))?;
    ensure!(
        stage.join("payload").join(DATABASE_PATH).is_file(),
        "インポート準備データがありません"
    );

    let backups = data_root.join("backups");
    fs::create_dir_all(&backups)?;
    let filename = format!(
        "before-import-{}.eptbackup",
        Utc::now().format("%Y%m%d-%H%M%S-%3f")
    );
    let auto_backup = backups.join(filename);
    export_backup(database, data_root, &auto_backup, true, true, on_progress)?;
    write_json_atomic(
        &data_root.join(PENDING_IMPORT_FILE),
        &PendingImport {
            import_id: import_id.into(),
            phase: PendingPhase::Prepared,
            auto_backup_path: auto_backup.to_string_lossy().into_owned(),
        },
    )
}

pub(crate) fn cancel_import(data_root: &Path, import_id: &str) -> Result<()> {
    validate_import_id(import_id)?;
    let pending_path = data_root.join(PENDING_IMPORT_FILE);
    if pending_path.exists() {
        let pending: PendingImport = read_json(&pending_path)?;
        ensure!(
            pending.import_id == import_id,
            "別のインポートが予約されています"
        );
        ensure!(
            matches!(pending.phase, PendingPhase::Prepared),
            "インポート処理はすでに開始されています"
        );
        fs::remove_file(&pending_path)?;
        let previous = pending_path.with_extension("previous");
        if previous.exists() {
            fs::remove_file(previous)?;
        }
    }
    let stage = stage_root(data_root, import_id);
    if stage.exists() {
        fs::remove_dir_all(stage)?;
    }
    Ok(())
}

pub(crate) fn apply_pending_import(data_root: &Path) -> Result<Option<AppliedImport>> {
    let pending_path = data_root.join(PENDING_IMPORT_FILE);
    let previous_pending = pending_path.with_extension("previous");
    if !pending_path.exists() && previous_pending.exists() {
        fs::rename(&previous_pending, &pending_path)?;
    }
    if !pending_path.exists() {
        return Ok(None);
    }
    let mut pending: PendingImport = read_json(&pending_path)?;
    validate_import_id(&pending.import_id)?;
    let rollback = rollback_root(data_root, &pending.import_id);

    if rollback.exists() || !matches!(pending.phase, PendingPhase::Prepared) {
        if !rollback_has_data(&rollback) && data_root.join(DATABASE_PATH).is_file() {
            cleanup_import_files(data_root, &pending.import_id, true);
            write_import_notice(
                data_root,
                false,
                "前回中断されたインポートを取り消しました。元のデータは変更されていません。",
                &pending.auto_backup_path,
            );
            return Ok(None);
        }
        let remove_new = matches!(pending.phase, PendingPhase::NewMoved);
        rollback_active_data(data_root, &rollback, remove_new)?;
        cleanup_import_files(data_root, &pending.import_id, true);
        write_import_notice(
            data_root,
            false,
            "前回中断されたインポートを取り消し、元のデータへ戻しました。",
            &pending.auto_backup_path,
        );
        return Ok(None);
    }

    let payload = stage_root(data_root, &pending.import_id).join("payload");
    if !payload.join(DATABASE_PATH).is_file() {
        cleanup_import_files(data_root, &pending.import_id, true);
        write_import_notice(
            data_root,
            false,
            "インポートの準備データが見つからなかったため、元のデータは変更していません。",
            &pending.auto_backup_path,
        );
        return Ok(None);
    }
    fs::create_dir_all(&rollback)?;
    pending.phase = PendingPhase::OldMoved;
    write_json_atomic(&pending_path, &pending)?;
    if let Err(error) = move_active_data(data_root, &rollback) {
        if let Err(rollback_error) = rollback_active_data(data_root, &rollback, false) {
            return Err(error.context(format!(
                "元のデータへのロールバックにも失敗しました: {rollback_error:#}"
            )));
        }
        cleanup_import_files(data_root, &pending.import_id, true);
        return Err(error);
    }

    pending.phase = PendingPhase::NewMoved;
    if let Err(error) = write_json_atomic(&pending_path, &pending) {
        if let Err(rollback_error) = rollback_active_data(data_root, &rollback, false) {
            return Err(error.context(format!(
                "元のデータへのロールバックにも失敗しました: {rollback_error:#}"
            )));
        }
        cleanup_import_files(data_root, &pending.import_id, true);
        return Err(error);
    }
    if let Err(error) = move_staged_data(&payload, data_root) {
        pending.phase = PendingPhase::RollingBack;
        write_json_atomic(&pending_path, &pending)?;
        if let Err(rollback_error) = rollback_active_data(data_root, &rollback, true) {
            return Err(error.context(format!(
                "元のデータへのロールバックにも失敗しました: {rollback_error:#}"
            )));
        }
        cleanup_import_files(data_root, &pending.import_id, true);
        return Err(error);
    }
    Ok(Some(AppliedImport { pending }))
}

pub(crate) fn finish_import(data_root: &Path, applied: AppliedImport, warning: Option<&str>) {
    let message = warning.map_or_else(
        || "バックアップをインポートしました。インポート前のデータも自動保存されています。".into(),
        |warning| {
            format!(
                "バックアップをインポートしました。インポート前のデータも自動保存されています。{warning}"
            )
        },
    );
    let pending_path = data_root.join(PENDING_IMPORT_FILE);
    let _ = fs::remove_file(&pending_path);
    let _ = fs::remove_file(pending_path.with_extension("previous"));
    let _ = fs::remove_dir_all(rollback_root(data_root, &applied.pending.import_id));
    let _ = fs::remove_dir_all(stage_root(data_root, &applied.pending.import_id));
    write_import_notice(data_root, true, &message, &applied.pending.auto_backup_path);
}

pub(crate) fn rollback_import(data_root: &Path, mut applied: AppliedImport) {
    let rollback = rollback_root(data_root, &applied.pending.import_id);
    applied.pending.phase = PendingPhase::RollingBack;
    if let Err(error) = write_json_atomic(&data_root.join(PENDING_IMPORT_FILE), &applied.pending) {
        log::error!("failed to mark imported data for rollback: {error:#}");
        return;
    }
    if let Err(error) = rollback_active_data(data_root, &rollback, true) {
        log::error!("failed to roll back imported data: {error:#}");
        return;
    }
    cleanup_import_files(data_root, &applied.pending.import_id, true);
    write_import_notice(
        data_root,
        false,
        "バックアップを適用できなかったため、元のデータへ戻しました。",
        &applied.pending.auto_backup_path,
    );
}

pub(crate) fn take_import_notice(data_root: &Path) -> Result<Option<BackupImportNotice>> {
    let path = data_root.join(IMPORT_NOTICE_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let notice = read_json(&path)?;
    fs::remove_file(path)?;
    Ok(Some(notice))
}

fn prepare_snapshot_for_export(
    database_path: &Path,
    data_root: &Path,
    includes_screenshots: bool,
    snapshot_at: &str,
) -> Result<PreparedSnapshot> {
    let mut connection = Connection::open(database_path)?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    let transaction = connection.transaction()?;
    close_open_pauses(&transaction, snapshot_at)?;
    validate_pause_records(&transaction)?;
    let thumbnail_rows = {
        let mut statement = transaction.prepare(
            "SELECT id, thumbnail_path FROM games WHERE thumbnail_path IS NOT NULL ORDER BY id",
        )?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let screenshot_rows = if includes_screenshots {
        let mut statement =
            transaction.prepare("SELECT id, game_id, path FROM game_screenshots ORDER BY id")?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        transaction.execute("DELETE FROM game_screenshots", [])?;
        Vec::new()
    };

    let mut media = Vec::new();
    let mut missing = 0;
    for (game_id, stored_path) in thumbnail_rows {
        if let Some(source) = managed_file(&stored_path, &data_root.join("thumbnails")) {
            let archive_path = format!("thumbnails/{game_id}.{}", safe_extension(&source, "image"));
            transaction.execute(
                "UPDATE games SET thumbnail_path=?1 WHERE id=?2",
                params![archive_path, game_id],
            )?;
            media.push((archive_path, source));
        } else {
            transaction.execute(
                "UPDATE games SET thumbnail_path=NULL WHERE id=?1",
                [game_id],
            )?;
            missing += 1;
        }
    }
    for (screenshot_id, game_id, stored_path) in screenshot_rows {
        if let Some(source) = managed_file(&stored_path, &data_root.join("screenshots")) {
            let archive_path = format!(
                "screenshots/{game_id}/{screenshot_id}.{}",
                safe_extension(&source, "png")
            );
            transaction.execute(
                "UPDATE game_screenshots SET path=?1 WHERE id=?2",
                params![archive_path, screenshot_id],
            )?;
            media.push((archive_path, source));
        } else {
            transaction.execute("DELETE FROM game_screenshots WHERE id=?1", [screenshot_id])?;
            missing += 1;
        }
    }
    transaction.commit()?;
    validate_database(&connection)?;
    let summary = database_summary(&connection)?;
    connection.pragma_update(None, "journal_mode", "DELETE")?;
    drop(connection);
    Ok(PreparedSnapshot {
        summary,
        media,
        missing_media_count: missing,
    })
}

fn rewrite_imported_paths(
    connection: &Connection,
    payload: &Path,
    data_root: &Path,
    manifest_paths: &HashSet<&str>,
) -> Result<()> {
    let thumbnails = {
        let mut statement = connection.prepare(
            "SELECT id, thumbnail_path FROM games WHERE thumbnail_path IS NOT NULL ORDER BY id",
        )?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let screenshots = {
        let mut statement =
            connection.prepare("SELECT id, path FROM game_screenshots ORDER BY id")?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut referenced_media = HashSet::new();
    for (id, archive_path) in thumbnails {
        validate_media_reference(&archive_path, "thumbnails/", payload, manifest_paths)?;
        referenced_media.insert(archive_path.clone());
        connection.execute(
            "UPDATE games SET thumbnail_path=?1 WHERE id=?2",
            params![
                data_root
                    .join(archive_path_to_path(&archive_path)?)
                    .to_string_lossy(),
                id
            ],
        )?;
    }
    for (id, archive_path) in screenshots {
        validate_media_reference(&archive_path, "screenshots/", payload, manifest_paths)?;
        referenced_media.insert(archive_path.clone());
        connection.execute(
            "UPDATE game_screenshots SET path=?1 WHERE id=?2",
            params![
                data_root
                    .join(archive_path_to_path(&archive_path)?)
                    .to_string_lossy(),
                id
            ],
        )?;
    }
    let archived_media: HashSet<String> = manifest_paths
        .iter()
        .filter(|path| path.starts_with("thumbnails/") || path.starts_with("screenshots/"))
        .map(|path| (*path).to_owned())
        .collect();
    ensure!(
        referenced_media == archived_media,
        "バックアップに参照されていない画像が含まれています"
    );
    Ok(())
}

fn reset_machine_specific_state(connection: &Connection) -> Result<()> {
    let stored: Option<String> = connection
        .query_row("SELECT value FROM settings WHERE key='app'", [], |row| {
            row.get(0)
        })
        .optional()?;
    let mut settings = stored
        .as_deref()
        .map(serde_json::from_str::<AppSettings>)
        .transpose()?
        .unwrap_or_default();
    settings.skipped_update_version = None;
    let now = Utc::now().to_rfc3339();
    connection.execute(
        "INSERT INTO settings(key,value,updated_at) VALUES('app',?1,?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",
        params![serde_json::to_string(&settings)?, now],
    )?;
    connection.execute(
        "INSERT INTO settings(key,value,updated_at) VALUES('last_seen',?1,?1)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",
        [now],
    )?;
    Ok(())
}

fn extract_and_validate_archive(source: &Path, payload: &Path) -> Result<BackupManifest> {
    let file = File::open(source)?;
    let mut archive = ZipArchive::new(file).context("ZIPとして読み込めません")?;
    ensure!(
        archive.len() <= 1_000_001,
        "バックアップ内のファイル数が多すぎます"
    );
    let mut names = HashSet::new();
    let mut case_insensitive_names = HashSet::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let name = std::str::from_utf8(entry.name_raw())
            .context("UTF-8ではないファイル名が含まれています")?;
        ensure!(
            is_safe_archive_path(name),
            "安全でないファイル名が含まれています"
        );
        ensure!(!entry.is_dir(), "フォルダー項目は使用できません");
        ensure!(!entry.is_symlink(), "シンボリックリンクは使用できません");
        ensure!(
            names.insert(name.to_owned()),
            "同名のファイルが重複しています"
        );
        ensure!(
            case_insensitive_names.insert(name.to_ascii_lowercase()),
            "大文字と小文字だけが異なるファイル名が重複しています"
        );
    }
    ensure!(names.contains(MANIFEST_PATH), "manifest.jsonがありません");
    let manifest = {
        let entry = archive.by_name(MANIFEST_PATH)?;
        ensure!(
            entry.size() <= MAX_MANIFEST_BYTES,
            "manifest.jsonが大きすぎます"
        );
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_MANIFEST_BYTES,
            "manifest.jsonが大きすぎます"
        );
        serde_json::from_slice::<BackupManifest>(&bytes).context("manifest.jsonが不正です")?
    };
    validate_manifest(&manifest)?;
    let expected_names: HashSet<&str> = manifest
        .files
        .iter()
        .map(|entry| entry.path.as_str())
        .chain(std::iter::once(MANIFEST_PATH))
        .collect();
    ensure!(
        names.len() == expected_names.len()
            && names
                .iter()
                .all(|name| expected_names.contains(name.as_str())),
        "manifest.jsonにないファイルが含まれています"
    );

    for expected in &manifest.files {
        let entry = archive.by_name(&expected.path)?;
        ensure!(
            entry.size() == expected.size,
            "ファイルサイズが一致しません"
        );
        let target = payload.join(archive_path_to_path(&expected.path)?);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = File::create(&target)?;
        let copied = std::io::copy(&mut entry.take(expected.size + 1), &mut output)?;
        output.sync_all()?;
        ensure!(copied == expected.size, "ファイルサイズが一致しません");
        ensure!(
            sha256_file(&target)? == expected.sha256,
            "チェックサムが一致しません"
        );
    }
    Ok(manifest)
}

fn validate_manifest(manifest: &BackupManifest) -> Result<()> {
    ensure!(
        manifest.format == BACKUP_FORMAT,
        "対応していないバックアップ形式です"
    );
    ensure!(
        manifest.format_version == BACKUP_FORMAT_VERSION,
        "対応していないバックアップ形式のバージョンです"
    );
    ensure!(
        !manifest.app_version.trim().is_empty(),
        "アプリバージョンがありません"
    );
    chrono::DateTime::parse_from_rfc3339(&manifest.exported_at)
        .context("バックアップ作成日時が不正です")?;
    ensure!(
        manifest.schema_version >= 1,
        "対応していないデータベースバージョンです"
    );
    ensure!(
        manifest.schema_version <= CURRENT_SCHEMA_VERSION,
        "このアプリより新しいバージョンで作成されたバックアップです"
    );
    ensure!(
        !manifest.files.is_empty(),
        "バックアップにデータベースがありません"
    );
    let mut paths = HashSet::new();
    let mut case_insensitive_paths = HashSet::new();
    let mut total_size = 0_u64;
    for file in &manifest.files {
        ensure!(
            is_safe_archive_path(&file.path),
            "安全でないファイル名があります"
        );
        ensure!(file.path != MANIFEST_PATH, "manifest.jsonが重複しています");
        ensure!(
            paths.insert(file.path.as_str()),
            "ファイル名が重複しています"
        );
        ensure!(
            case_insensitive_paths.insert(file.path.to_ascii_lowercase()),
            "大文字と小文字だけが異なるファイル名が重複しています"
        );
        ensure!(
            file.path == DATABASE_PATH
                || file.path.starts_with("thumbnails/")
                || file.path.starts_with("screenshots/"),
            "対応していないファイルが含まれています"
        );
        ensure!(
            file.sha256.len() == 64 && file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "チェックサムの形式が不正です"
        );
        total_size = total_size
            .checked_add(file.size)
            .context("バックアップサイズが不正です")?;
        ensure!(total_size <= MAX_BACKUP_BYTES, "バックアップが大きすぎます");
    }
    ensure!(paths.contains(DATABASE_PATH), "app.dbがありません");
    Ok(())
}

fn write_archive(
    path: &Path,
    manifest: &mut BackupManifest,
    files: &[ExportFile],
    total_bytes: u64,
    on_progress: &mut impl FnMut(BackupExportProgress),
) -> Result<()> {
    let output = File::create(path)?;
    let mut archive = ZipWriter::new(output);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o600);
    let mut processed_bytes = 0_u64;
    let mut last_reported_bytes = 0_u64;
    for file in files {
        archive.start_file(&file.path, options)?;
        let mut source = File::open(&file.source)?;
        let mut hasher = Sha256::new();
        let file_start = processed_bytes;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = source.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            archive.write_all(&buffer[..read])?;
            hasher.update(&buffer[..read]);
            processed_bytes += read as u64;
            if processed_bytes.saturating_sub(last_reported_bytes) >= 512 * 1024 {
                on_progress(export_progress(
                    "archiving",
                    processed_bytes,
                    total_bytes,
                    files.len(),
                ));
                last_reported_bytes = processed_bytes;
            }
        }
        ensure!(
            processed_bytes - file_start == file.size,
            "書き込み中にファイルサイズが変化しました"
        );
        let digest = hasher.finalize();
        manifest.files.push(ManifestFile {
            path: file.path.clone(),
            size: file.size,
            sha256: hex_digest(&digest),
        });
        if processed_bytes != last_reported_bytes {
            on_progress(export_progress(
                "archiving",
                processed_bytes,
                total_bytes,
                files.len(),
            ));
            last_reported_bytes = processed_bytes;
        }
    }
    archive.start_file(MANIFEST_PATH, options)?;
    archive.write_all(&serde_json::to_vec_pretty(manifest)?)?;
    let output = archive.finish()?;
    output.sync_all()?;
    Ok(())
}

fn export_file(path: String, source: PathBuf) -> Result<ExportFile> {
    let size = source.metadata()?.len();
    Ok(ExportFile { path, size, source })
}

fn export_progress(
    phase: &str,
    processed_bytes: u64,
    total_bytes: u64,
    file_count: usize,
) -> BackupExportProgress {
    BackupExportProgress {
        phase: phase.into(),
        processed_bytes,
        total_bytes,
        file_count,
    }
}

fn default_true() -> bool {
    true
}

fn validate_database(connection: &Connection) -> Result<()> {
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    ensure!(
        integrity == "ok",
        "データベースの整合性チェックに失敗しました"
    );
    let foreign_key_error: Option<i64> = connection
        .query_row(
            "SELECT 1 FROM pragma_foreign_key_check LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    ensure!(foreign_key_error.is_none(), "外部キーが破損しています");
    Ok(())
}

fn database_summary(connection: &Connection) -> Result<BackupDataSummary> {
    Ok(BackupDataSummary {
        game_count: count(connection, "games")?,
        session_count: count(connection, "play_sessions")?,
        timestamp_count: count(connection, "game_timestamps")?,
        screenshot_count: count(connection, "game_screenshots")?,
        thumbnail_count: connection.query_row(
            "SELECT COUNT(*) FROM games WHERE thumbnail_path IS NOT NULL",
            [],
            |row| row.get(0),
        )?,
    })
}

fn count(connection: &Connection, table: &str) -> Result<i64> {
    let allowed = [
        "games",
        "play_sessions",
        "game_timestamps",
        "game_screenshots",
    ];
    ensure!(allowed.contains(&table), "集計対象が不正です");
    Ok(
        connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })?,
    )
}

fn missing_executable_count(connection: &Connection) -> Result<i64> {
    let paths = {
        let mut statement = connection.prepare("SELECT path FROM game_executables")?;
        statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    Ok(paths
        .iter()
        .filter(|path| !Path::new(path.as_str()).is_file())
        .count() as i64)
}

fn validate_media_reference(
    archive_path: &str,
    expected_prefix: &str,
    payload: &Path,
    manifest_paths: &HashSet<&str>,
) -> Result<()> {
    ensure!(
        archive_path.starts_with(expected_prefix) && is_safe_archive_path(archive_path),
        "画像の保存先が不正です"
    );
    ensure!(
        manifest_paths.contains(archive_path),
        "画像ファイルがmanifestにありません"
    );
    ensure!(
        payload.join(archive_path_to_path(archive_path)?).is_file(),
        "画像ファイルがありません"
    );
    Ok(())
}

fn managed_file(stored_path: &str, managed_root: &Path) -> Option<PathBuf> {
    let root = managed_root.canonicalize().ok()?;
    let source = Path::new(stored_path).canonicalize().ok()?;
    let metadata = source.metadata().ok()?;
    (metadata.is_file() && source.starts_with(root)).then_some(source)
}

fn safe_extension(path: &Path, fallback: &str) -> String {
    path.extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| {
            !extension.is_empty()
                && extension.len() <= 10
                && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| fallback.into())
}

fn is_safe_archive_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', ':'])
        && !path.starts_with('/')
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn archive_path_to_path(path: &str) -> Result<PathBuf> {
    ensure!(is_safe_archive_path(path), "安全でないファイル名です");
    Ok(path.split('/').collect())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = hasher.finalize();
    Ok(hex_digest(&digest))
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn backup_destination(requested: &Path) -> PathBuf {
    if requested
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("eptbackup"))
    {
        requested.to_owned()
    } else {
        requested.with_extension("eptbackup")
    }
}

fn replace_file(partial: &Path, destination: &Path, operation_id: &str) -> Result<()> {
    if !destination.exists() {
        fs::rename(partial, destination)?;
        return Ok(());
    }
    let previous = destination.with_file_name(format!(".eptbackup-{operation_id}.previous"));
    fs::rename(destination, &previous)?;
    if let Err(error) = fs::rename(partial, destination) {
        let _ = fs::rename(previous, destination);
        return Err(error.into());
    }
    let _ = fs::remove_file(previous);
    Ok(())
}

fn move_active_data(data_root: &Path, rollback: &Path) -> Result<()> {
    for name in [
        DATABASE_PATH,
        "app.db-wal",
        "app.db-shm",
        "thumbnails",
        "screenshots",
    ] {
        let source = data_root.join(name);
        if source.exists() {
            fs::rename(&source, rollback.join(name))?;
        }
    }
    Ok(())
}

fn move_staged_data(payload: &Path, data_root: &Path) -> Result<()> {
    for name in [DATABASE_PATH, "thumbnails", "screenshots"] {
        let source = payload.join(name);
        ensure!(source.exists(), "インポートするデータが不足しています");
        fs::rename(source, data_root.join(name))?;
    }
    Ok(())
}

fn rollback_active_data(data_root: &Path, rollback: &Path, remove_new: bool) -> Result<()> {
    for name in [
        DATABASE_PATH,
        "app.db-wal",
        "app.db-shm",
        "thumbnails",
        "screenshots",
    ] {
        let active = data_root.join(name);
        let saved = rollback.join(name);
        if saved.exists() {
            remove_path(&active)?;
            fs::rename(saved, active)?;
        } else if remove_new {
            remove_path(&active)?;
        }
    }
    if rollback.exists() {
        fs::remove_dir_all(rollback)?;
    }
    Ok(())
}

fn rollback_has_data(rollback: &Path) -> bool {
    [
        DATABASE_PATH,
        "app.db-wal",
        "app.db-shm",
        "thumbnails",
        "screenshots",
    ]
    .iter()
    .any(|name| rollback.join(name).exists())
}

fn remove_path(path: &Path) -> Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)?;
    } else if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn cleanup_import_files(data_root: &Path, import_id: &str, remove_pending: bool) {
    if remove_pending {
        let _ = fs::remove_file(data_root.join(PENDING_IMPORT_FILE));
        let _ = fs::remove_file(
            data_root
                .join(PENDING_IMPORT_FILE)
                .with_extension("previous"),
        );
    }
    let _ = fs::remove_dir_all(stage_root(data_root, import_id));
    let _ = fs::remove_dir_all(rollback_root(data_root, import_id));
}

fn write_import_notice(data_root: &Path, success: bool, message: &str, auto_backup_path: &str) {
    let notice = BackupImportNotice {
        success,
        message: message.into(),
        auto_backup_path: auto_backup_path.into(),
    };
    if let Err(error) = write_json_atomic(&data_root.join(IMPORT_NOTICE_FILE), &notice) {
        log::warn!("failed to write import notice: {error:#}");
    }
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<()> {
    let temporary = path.with_extension("tmp");
    let previous = path.with_extension("previous");
    if temporary.exists() {
        fs::remove_file(&temporary)?;
    }
    let mut file = File::create(&temporary)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    if previous.exists() {
        fs::remove_file(&previous)?;
    }
    if path.exists() {
        fs::rename(path, &previous)?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        if previous.exists() {
            let _ = fs::rename(&previous, path);
        }
        return Err(error.into());
    }
    if previous.exists() {
        fs::remove_file(previous)?;
    }
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let bytes = fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn validate_import_id(import_id: &str) -> Result<()> {
    ensure!(
        !import_id.is_empty()
            && import_id.len() <= 80
            && import_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'),
        "インポートIDが不正です"
    );
    Ok(())
}

fn stage_root(data_root: &Path, import_id: &str) -> PathBuf {
    data_root.join("import-staging").join(import_id)
}

fn rollback_root(data_root: &Path, import_id: &str) -> PathBuf {
    data_root.join("import-rollback").join(import_id)
}

fn operation_id() -> String {
    format!(
        "{}-{}",
        Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        std::process::id()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::CreateGameInput;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!("ept-{label}-{}", operation_id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn create_data_root(parent: &Path, name: &str) -> (PathBuf, Database, i64) {
        let root = parent.join(name);
        fs::create_dir_all(root.join("thumbnails")).unwrap();
        fs::create_dir_all(root.join("screenshots")).unwrap();
        let database = Database::open(&root.join(DATABASE_PATH)).unwrap();
        let thumbnail = root.join("thumbnails").join("cover.png");
        fs::write(&thumbnail, b"thumbnail").unwrap();
        let game_id = database
            .create_game(
                &CreateGameInput {
                    title: format!("Game from {name}"),
                    brand: Some("Brand".into()),
                    release_date: None,
                    thumbnail_path: None,
                    source: None,
                    executable_paths: vec![
                        parent.join("missing.exe").to_string_lossy().into_owned(),
                    ],
                    launch_command: None,
                },
                Some(&thumbnail.to_string_lossy()),
            )
            .unwrap();
        let session_id = database
            .manual_session(game_id, "2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z")
            .unwrap();
        let screenshot_directory = root.join("screenshots").join(game_id.to_string());
        fs::create_dir_all(&screenshot_directory).unwrap();
        let screenshot = screenshot_directory.join("capture.png");
        fs::write(&screenshot, b"screenshot").unwrap();
        database
            .add_screenshot(
                game_id,
                Some(session_id),
                &screenshot.to_string_lossy(),
                "2026-01-01T00:30:00Z",
                1920,
                1080,
            )
            .unwrap();
        database
            .create_interval(session_id, "2026-01-01T00:15:00Z", "2026-01-01T00:30:00Z")
            .unwrap();
        (root, database, game_id)
    }

    #[test]
    fn pause_history_is_snapshotted_without_resuming_the_running_app() {
        let temporary = TestDirectory::new("backup-pause");
        let (source_root, source, game) = create_data_root(&temporary.0, "source");
        source.set_paused(true, "2026-01-01T00:20:00Z").unwrap();
        source.set_paused(false, "2026-01-01T00:40:00Z").unwrap();
        source.set_paused(true, "2026-01-01T02:00:00Z").unwrap();
        let mut settings = source.settings().unwrap();
        settings.tracking_pause_hotkey = "Ctrl+Shift+F11".into();
        source
            .set_setting("app", &serde_json::to_string(&settings).unwrap())
            .unwrap();
        let path = temporary.0.join("pauses.eptbackup");
        export_backup(&source, &source_root, &path, false, true, |_| {}).unwrap();
        assert!(source.active_pause().unwrap().is_some());
        let (target_root, target, _) = create_data_root(&temporary.0, "target");
        let preview = prepare_import(&target, &target_root, &path).unwrap();
        let staged = Database::open(
            &stage_root(&target_root, &preview.import_id)
                .join("payload")
                .join(DATABASE_PATH),
        )
        .unwrap();
        assert!(staged.active_pause().unwrap().is_none());
        assert_eq!(staged.pause_intervals(None, 100).unwrap().len(), 2);
        assert_eq!(
            staged.settings().unwrap().tracking_pause_hotkey,
            "Ctrl+Shift+F11"
        );
        assert_eq!(
            staged
                .get_game(game)
                .unwrap()
                .summary
                .total_playtime_seconds,
            2100
        );
    }

    fn rewrite_test_archive(
        source: &Path,
        destination: &Path,
        schema_version: Option<i64>,
        tamper_database: bool,
    ) {
        let mut source = ZipArchive::new(File::open(source).unwrap()).unwrap();
        let mut entries = Vec::new();
        for index in 0..source.len() {
            let mut entry = source.by_index(index).unwrap();
            let name = entry.name().to_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            if name == MANIFEST_PATH {
                let mut manifest: BackupManifest = serde_json::from_slice(&bytes).unwrap();
                if let Some(schema_version) = schema_version {
                    manifest.schema_version = schema_version;
                }
                bytes = serde_json::to_vec_pretty(&manifest).unwrap();
            } else if name == DATABASE_PATH && tamper_database {
                bytes[0] ^= 0xff;
            }
            entries.push((name, bytes));
        }
        let mut destination = ZipWriter::new(File::create(destination).unwrap());
        for (name, bytes) in entries {
            destination
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            destination.write_all(&bytes).unwrap();
        }
        destination.finish().unwrap();
    }

    fn rewrite_empty_archive_as_schema_4(source: &Path, destination: &Path, work: &Path) {
        let mut source = ZipArchive::new(File::open(source).unwrap()).unwrap();
        let mut manifest = None;
        let mut database_bytes = None;
        for index in 0..source.len() {
            let mut entry = source.by_index(index).unwrap();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            match entry.name() {
                MANIFEST_PATH => {
                    manifest = Some(serde_json::from_slice::<BackupManifest>(&bytes).unwrap())
                }
                DATABASE_PATH => database_bytes = Some(bytes),
                unexpected => panic!("unexpected file in empty backup: {unexpected}"),
            }
        }
        let database_path = work.join("schema-4.db");
        fs::write(&database_path, database_bytes.unwrap()).unwrap();
        let connection = Connection::open(&database_path).unwrap();
        connection
            .execute_batch(
                "DROP TABLE tracking_pause_intervals; DROP TABLE game_screenshots;
                 DROP INDEX idx_games_vndb_id;
                 ALTER TABLE games DROP COLUMN vndb_id;
                 ALTER TABLE games DROP COLUMN metadata_source;
                 ALTER TABLE games DROP COLUMN launch_command;
                 DELETE FROM schema_migrations WHERE version>=5;",
            )
            .unwrap();
        connection
            .pragma_update(None, "journal_mode", "DELETE")
            .unwrap();
        drop(connection);
        let database_bytes = fs::read(&database_path).unwrap();
        let mut manifest = manifest.unwrap();
        manifest.schema_version = 4;
        let database_entry = manifest
            .files
            .iter_mut()
            .find(|entry| entry.path == DATABASE_PATH)
            .unwrap();
        database_entry.size = database_bytes.len() as u64;
        database_entry.sha256 = hex_digest(&Sha256::digest(&database_bytes));

        let mut destination = ZipWriter::new(File::create(destination).unwrap());
        destination
            .start_file(MANIFEST_PATH, SimpleFileOptions::default())
            .unwrap();
        destination
            .write_all(&serde_json::to_vec_pretty(&manifest).unwrap())
            .unwrap();
        destination
            .start_file(DATABASE_PATH, SimpleFileOptions::default())
            .unwrap();
        destination.write_all(&database_bytes).unwrap();
        destination.finish().unwrap();
    }

    #[test]
    fn export_import_round_trip_rewrites_media_and_preserves_playtime() {
        use crate::update_notice::UpdateNotices;
        let temporary = TestDirectory::new("backup-round-trip");
        let (source_root, source_database, game_id) = create_data_root(&temporary.0, "source");
        source_database
            .update_game(
                game_id,
                &crate::models::UpdateGameInput {
                    title: "Game from source".into(),
                    brand: Some("Brand".into()),
                    release_date: None,
                    source_url: Some("https://vndb.org/v548".into()),
                },
            )
            .unwrap();
        UpdateNotices::open(&source_root, "0.1.10".parse().unwrap(), false).unwrap();
        source_database
            .set_setting(
                "app",
                &serde_json::to_string(&AppSettings {
                    autostart: true,
                    start_minimized: true,
                    auto_check_updates: false,
                    skipped_update_version: Some("0.1.9".into()),
                    close_to_tray: false,
                    exclude_background_time: false,
                    exclude_pause_time: true,
                    tracking_pause_hotkey: String::new(),
                    theme: "blue".into(),
                    screenshot_hotkey: "F10".into(),
                    ocr_search_hotkey: "Ctrl+F10".into(),
                })
                .unwrap(),
            )
            .unwrap();
        source_database
            .set_setting("last_seen", "2020-01-01T00:00:00Z")
            .unwrap();
        let backup_path = temporary.0.join("migration.eptbackup");
        let mut progress = Vec::new();
        let exported = export_backup(
            &source_database,
            &source_root,
            &backup_path,
            false,
            true,
            |event| progress.push(event),
        )
        .unwrap();
        assert_eq!(exported.summary.game_count, 1);
        assert_eq!(exported.summary.thumbnail_count, 1);
        assert_eq!(exported.summary.screenshot_count, 1);
        assert!(exported.includes_screenshots);
        assert_eq!(progress.first().unwrap().phase, "preparing");
        assert_eq!(progress.last().unwrap().phase, "finalizing");
        assert_eq!(
            progress.last().unwrap().processed_bytes,
            progress.last().unwrap().total_bytes
        );
        assert!(
            progress
                .windows(2)
                .all(|events| { events[0].processed_bytes <= events[1].processed_bytes })
        );

        let (destination_root, destination_database, _) =
            create_data_root(&temporary.0, "destination");
        UpdateNotices::open(&destination_root, "0.1.12".parse().unwrap(), false).unwrap();
        let preview =
            prepare_import(&destination_database, &destination_root, &backup_path).unwrap();
        assert_eq!(preview.summary, exported.summary);
        assert_eq!(preview.current_summary.game_count, 1);
        assert_eq!(preview.missing_executable_count, 1);
        confirm_import(
            &destination_database,
            &destination_root,
            &preview.import_id,
            |_| {},
        )
        .unwrap();
        drop(destination_database);

        let applied = apply_pending_import(&destination_root).unwrap().unwrap();
        assert!(
            UpdateNotices::open(&destination_root, "0.1.12".parse().unwrap(), true)
                .unwrap()
                .pending()
                .is_none()
        );
        let imported = Database::open(&destination_root.join(DATABASE_PATH)).unwrap();
        let game = imported.get_game(game_id).unwrap();
        assert_eq!(game.summary.title, "Game from source");
        assert_eq!(game.vndb_id.as_deref(), Some("v548"));
        assert_eq!(game.metadata_source.as_deref(), Some("vndb"));
        assert_eq!(
            imported.metadata_identity(game_id).unwrap().url(),
            "https://vndb.org/v548"
        );
        let thumbnail = PathBuf::from(game.summary.thumbnail_path.unwrap());
        assert!(thumbnail.starts_with(destination_root.join("thumbnails")));
        assert_eq!(fs::read(thumbnail).unwrap(), b"thumbnail");
        let screenshots = imported.screenshots(game_id).unwrap();
        let screenshot = PathBuf::from(&screenshots[0].path);
        assert!(screenshot.starts_with(destination_root.join("screenshots")));
        assert_eq!(fs::read(screenshot).unwrap(), b"screenshot");
        let sessions = imported.list_sessions(game_id).unwrap();
        assert_eq!(sessions[0].playtime_seconds, 3_600);
        assert_eq!(sessions[0].background_seconds, 900);
        let mut imported_settings: AppSettings =
            serde_json::from_str(&imported.get_setting("app").unwrap().unwrap()).unwrap();
        assert!(imported_settings.autostart);
        assert!(!imported_settings.auto_check_updates);
        assert!(!imported_settings.close_to_tray);
        assert!(!imported_settings.exclude_background_time);
        assert_eq!(imported_settings.theme, "blue");
        assert_eq!(imported_settings.screenshot_hotkey, "F10");
        assert_eq!(imported_settings.ocr_search_hotkey, "Ctrl+F10");
        assert_eq!(imported_settings.skipped_update_version, None);
        imported_settings.exclude_background_time = true;
        imported
            .set_setting("app", &serde_json::to_string(&imported_settings).unwrap())
            .unwrap();
        assert_eq!(
            imported.list_sessions(game_id).unwrap()[0].playtime_seconds,
            2_700
        );
        assert_ne!(
            imported.get_setting("last_seen").unwrap().unwrap(),
            "2020-01-01T00:00:00Z"
        );
        drop(imported);
        finish_import(&destination_root, applied, None);
        assert!(!destination_root.join(PENDING_IMPORT_FILE).exists());
        let notice = take_import_notice(&destination_root).unwrap().unwrap();
        assert!(notice.success);
        assert!(Path::new(&notice.auto_backup_path).is_file());
    }

    #[test]
    fn import_rejects_archive_path_traversal() {
        let temporary = TestDirectory::new("backup-path-traversal");
        let archive_path = temporary.0.join("unsafe.eptbackup");
        let file = File::create(&archive_path).unwrap();
        let mut archive = ZipWriter::new(file);
        archive
            .start_file("../manifest.json", SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"{}").unwrap();
        archive.finish().unwrap();
        let (root, database, _) = create_data_root(&temporary.0, "current");

        let error = prepare_import(&database, &root, &archive_path).unwrap_err();

        assert!(error.to_string().contains("安全でないファイル名"));
    }

    #[test]
    fn import_rejects_a_checksum_mismatch_before_touching_current_data() {
        let temporary = TestDirectory::new("backup-checksum");
        let (source_root, source_database, _) = create_data_root(&temporary.0, "source");
        let backup_path = temporary.0.join("valid.eptbackup");
        export_backup(
            &source_database,
            &source_root,
            &backup_path,
            false,
            true,
            |_| {},
        )
        .unwrap();
        let tampered_path = temporary.0.join("tampered.eptbackup");
        rewrite_test_archive(&backup_path, &tampered_path, None, true);
        let (current_root, current_database, current_game) =
            create_data_root(&temporary.0, "current");

        let error = prepare_import(&current_database, &current_root, &tampered_path).unwrap_err();

        assert!(error.to_string().contains("チェックサム"));
        assert!(current_database.get_game(current_game).is_ok());
        assert!(!current_root.join(PENDING_IMPORT_FILE).exists());
    }

    #[test]
    fn import_rejects_a_newer_schema_version() {
        let temporary = TestDirectory::new("backup-newer-schema");
        let (source_root, source_database, _) = create_data_root(&temporary.0, "source");
        let backup_path = temporary.0.join("valid.eptbackup");
        export_backup(
            &source_database,
            &source_root,
            &backup_path,
            false,
            true,
            |_| {},
        )
        .unwrap();
        let newer_path = temporary.0.join("newer.eptbackup");
        rewrite_test_archive(
            &backup_path,
            &newer_path,
            Some(CURRENT_SCHEMA_VERSION + 1),
            false,
        );
        let (current_root, current_database, _) = create_data_root(&temporary.0, "current");

        let error = prepare_import(&current_database, &current_root, &newer_path).unwrap_err();

        assert!(error.to_string().contains("新しいバージョン"));
    }

    #[test]
    fn empty_media_directories_are_valid_backup_data() {
        let temporary = TestDirectory::new("backup-empty-media");
        let source_root = temporary.0.join("source");
        fs::create_dir_all(source_root.join("thumbnails")).unwrap();
        fs::create_dir_all(source_root.join("screenshots")).unwrap();
        let source_database = Database::open(&source_root.join(DATABASE_PATH)).unwrap();
        let backup_path = temporary.0.join("empty.eptbackup");
        export_backup(
            &source_database,
            &source_root,
            &backup_path,
            false,
            true,
            |_| {},
        )
        .unwrap();
        let (current_root, current_database, _) = create_data_root(&temporary.0, "current");

        let preview = prepare_import(&current_database, &current_root, &backup_path).unwrap();

        assert_eq!(preview.summary.game_count, 0);
        assert_eq!(preview.summary.screenshot_count, 0);
        cancel_import(&current_root, &preview.import_id).unwrap();
    }

    #[test]
    fn export_can_intentionally_exclude_screenshots() {
        let temporary = TestDirectory::new("backup-without-screenshots");
        let (source_root, source_database, _) = create_data_root(&temporary.0, "source");
        let backup_path = temporary.0.join("without-screenshots.eptbackup");

        let exported = export_backup(
            &source_database,
            &source_root,
            &backup_path,
            false,
            false,
            |_| {},
        )
        .unwrap();

        assert!(!exported.includes_screenshots);
        assert_eq!(exported.summary.screenshot_count, 0);
        assert_eq!(exported.summary.thumbnail_count, 1);
        let mut archive = ZipArchive::new(File::open(&backup_path).unwrap()).unwrap();
        assert!((0..archive.len()).all(|index| {
            !archive
                .by_index(index)
                .unwrap()
                .name()
                .starts_with("screenshots/")
        }));
        let (current_root, current_database, _) = create_data_root(&temporary.0, "current");
        let preview = prepare_import(&current_database, &current_root, &backup_path).unwrap();
        assert!(!preview.includes_screenshots);
        assert_eq!(preview.summary.screenshot_count, 0);
        cancel_import(&current_root, &preview.import_id).unwrap();
    }

    #[test]
    fn version_one_manifests_without_the_screenshot_option_remain_compatible() {
        let manifest: BackupManifest = serde_json::from_value(serde_json::json!({
            "format": BACKUP_FORMAT,
            "format_version": BACKUP_FORMAT_VERSION,
            "app_version": "0.1.10",
            "schema_version": CURRENT_SCHEMA_VERSION,
            "exported_at": "2026-09-02T00:00:00Z",
            "summary": {
                "game_count": 0,
                "session_count": 0,
                "timestamp_count": 0,
                "screenshot_count": 0,
                "thumbnail_count": 0
            },
            "files": []
        }))
        .unwrap();

        assert!(manifest.includes_screenshots);
    }

    #[test]
    fn import_migrates_an_older_supported_schema_in_staging() {
        let temporary = TestDirectory::new("backup-old-schema");
        let source_root = temporary.0.join("source");
        fs::create_dir_all(source_root.join("thumbnails")).unwrap();
        fs::create_dir_all(source_root.join("screenshots")).unwrap();
        let source_database = Database::open(&source_root.join(DATABASE_PATH)).unwrap();
        let current_backup = temporary.0.join("current.eptbackup");
        export_backup(
            &source_database,
            &source_root,
            &current_backup,
            false,
            true,
            |_| {},
        )
        .unwrap();
        let older_backup = temporary.0.join("schema-4.eptbackup");
        rewrite_empty_archive_as_schema_4(&current_backup, &older_backup, &temporary.0);
        let (current_root, current_database, _) = create_data_root(&temporary.0, "current");

        let preview = prepare_import(&current_database, &current_root, &older_backup).unwrap();

        let staged = Connection::open(
            stage_root(&current_root, &preview.import_id)
                .join("payload")
                .join(DATABASE_PATH),
        )
        .unwrap();
        let schema_version: i64 = staged
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(schema_version, CURRENT_SCHEMA_VERSION);
        drop(staged);
        cancel_import(&current_root, &preview.import_id).unwrap();
    }

    #[test]
    fn interrupted_import_rolls_back_to_the_original_database() {
        let temporary = TestDirectory::new("backup-rollback");
        let (root, database, game_id) = create_data_root(&temporary.0, "current");
        drop(database);
        let rollback = rollback_root(&root, "123-456");
        fs::create_dir_all(&rollback).unwrap();
        fs::rename(root.join(DATABASE_PATH), rollback.join(DATABASE_PATH)).unwrap();
        fs::rename(root.join("thumbnails"), rollback.join("thumbnails")).unwrap();
        fs::rename(root.join("screenshots"), rollback.join("screenshots")).unwrap();
        fs::write(root.join(DATABASE_PATH), b"broken").unwrap();
        write_json_atomic(
            &root.join(PENDING_IMPORT_FILE),
            &PendingImport {
                import_id: "123-456".into(),
                phase: PendingPhase::NewMoved,
                auto_backup_path: "backup.eptbackup".into(),
            },
        )
        .unwrap();

        assert!(apply_pending_import(&root).unwrap().is_none());
        let restored = Database::open(&root.join(DATABASE_PATH)).unwrap();
        assert!(restored.get_game(game_id).is_ok());
    }

    #[test]
    fn interrupted_rollback_resumes_without_deleting_already_restored_data() {
        let temporary = TestDirectory::new("backup-partial-rollback");
        let (root, database, game_id) = create_data_root(&temporary.0, "current");
        drop(database);
        let rollback = rollback_root(&root, "123-789");
        fs::create_dir_all(&rollback).unwrap();
        fs::rename(root.join(DATABASE_PATH), rollback.join(DATABASE_PATH)).unwrap();
        fs::rename(root.join("thumbnails"), rollback.join("thumbnails")).unwrap();
        fs::rename(root.join("screenshots"), rollback.join("screenshots")).unwrap();
        // Simulate a retry after app.db was already restored but media was not.
        fs::rename(rollback.join(DATABASE_PATH), root.join(DATABASE_PATH)).unwrap();
        fs::create_dir_all(root.join("thumbnails")).unwrap();
        fs::create_dir_all(root.join("screenshots")).unwrap();
        fs::write(root.join("thumbnails").join("new.png"), b"new").unwrap();
        write_json_atomic(
            &root.join(PENDING_IMPORT_FILE),
            &PendingImport {
                import_id: "123-789".into(),
                phase: PendingPhase::RollingBack,
                auto_backup_path: "backup.eptbackup".into(),
            },
        )
        .unwrap();

        assert!(apply_pending_import(&root).unwrap().is_none());
        let restored = Database::open(&root.join(DATABASE_PATH)).unwrap();
        let game = restored.get_game(game_id).unwrap();
        assert_eq!(
            fs::read(game.summary.thumbnail_path.unwrap()).unwrap(),
            b"thumbnail"
        );
    }
}
