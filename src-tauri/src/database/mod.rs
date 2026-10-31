mod pauses;
mod playtime;
use crate::metadata::MetadataIdentity;
use crate::models::*;
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Datelike, Duration, Local, LocalResult, NaiveDate, TimeZone, Utc};
use parking_lot::Mutex;
pub(crate) use pauses::{close_open_pauses, validate_pause_records};
use playtime::{Coverage, SessionTime};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::Arc,
};

#[derive(Clone)]
pub struct Database(Arc<Mutex<Connection>>, Arc<Mutex<playtime::Cache>>);
pub(crate) const CURRENT_SCHEMA_VERSION: i64 = 8;
const MIGRATION_1: &str = r#"
CREATE TABLE IF NOT EXISTS schema_migrations(version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);
CREATE TABLE brands(id INTEGER PRIMARY KEY, erogamescape_id INTEGER, name TEXT NOT NULL COLLATE NOCASE UNIQUE);
CREATE TABLE games(id INTEGER PRIMARY KEY, erogamescape_id INTEGER UNIQUE, title TEXT NOT NULL, brand_id INTEGER REFERENCES brands(id) ON DELETE SET NULL, release_date TEXT, thumbnail_path TEXT, source_url TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE game_executables(id INTEGER PRIMARY KEY, game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE, path TEXT NOT NULL COLLATE NOCASE UNIQUE, file_name TEXT NOT NULL COLLATE NOCASE, created_at TEXT NOT NULL);
CREATE TABLE play_sessions(id INTEGER PRIMARY KEY, game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE, launched_at TEXT NOT NULL, exited_at TEXT, needs_review INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, CHECK(exited_at IS NULL OR exited_at >= launched_at));
CREATE TABLE focus_intervals(id INTEGER PRIMARY KEY, play_session_id INTEGER NOT NULL REFERENCES play_sessions(id) ON DELETE CASCADE, started_at TEXT NOT NULL, ended_at TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, CHECK(ended_at IS NULL OR ended_at >= started_at));
CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE INDEX idx_games_brand ON games(brand_id);CREATE INDEX idx_exec_path ON game_executables(path);CREATE INDEX idx_session_game_time ON play_sessions(game_id,launched_at);CREATE INDEX idx_interval_session_time ON focus_intervals(play_session_id,started_at);
"#;
const MIGRATION_2: &str = r#"
CREATE TABLE game_timestamps(
    id INTEGER PRIMARY KEY,
    game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    marked_at TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_game_timestamps_game_time ON game_timestamps(game_id,marked_at);
"#;
const MIGRATION_3: &str = r#"
ALTER TABLE games ADD COLUMN play_status TEXT NOT NULL DEFAULT 'unplayed'
    CHECK(play_status IN ('unplayed','playing','completed','retired'));
CREATE INDEX idx_games_play_status ON games(play_status);
"#;
const MIGRATION_4: &str = r#"
CREATE TABLE background_intervals(
    id INTEGER PRIMARY KEY,
    play_session_id INTEGER NOT NULL REFERENCES play_sessions(id) ON DELETE CASCADE,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK(ended_at IS NULL OR ended_at >= started_at)
);
CREATE INDEX idx_background_interval_session_time
    ON background_intervals(play_session_id,started_at);
ALTER TABLE play_sessions ADD COLUMN background_migrated INTEGER NOT NULL DEFAULT 0;
"#;
const MIGRATION_5: &str = r#"
CREATE TABLE game_screenshots(
    id INTEGER PRIMARY KEY,
    game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    play_session_id INTEGER REFERENCES play_sessions(id) ON DELETE SET NULL,
    path TEXT NOT NULL UNIQUE,
    captured_at TEXT NOT NULL,
    width INTEGER NOT NULL,
    height INTEGER NOT NULL
);
CREATE INDEX idx_game_screenshots_game_time ON game_screenshots(game_id,captured_at);
"#;
const MIGRATION_6: &str = r#"
ALTER TABLE games ADD COLUMN vndb_id TEXT;
CREATE UNIQUE INDEX idx_games_vndb_id ON games(vndb_id);
ALTER TABLE games ADD COLUMN metadata_source TEXT
    CHECK(metadata_source IN ('erogamescape','vndb'));
UPDATE games SET metadata_source='erogamescape' WHERE erogamescape_id IS NOT NULL;
"#;

const MIGRATION_7: &str = r#"
CREATE TABLE tracking_pause_intervals(
 id INTEGER PRIMARY KEY, started_at TEXT NOT NULL, ended_at TEXT,
 created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
 CHECK(ended_at IS NULL OR ended_at >= started_at)
);
CREATE INDEX idx_tracking_pause_time ON tracking_pause_intervals(started_at);
CREATE UNIQUE INDEX idx_tracking_pause_open ON tracking_pause_intervals((1)) WHERE ended_at IS NULL;
"#;
// NULL keeps the registered executable as the launch target, so games that never
// opt into a custom command keep the previous behavior.
const MIGRATION_8: &str = r#"
ALTER TABLE games ADD COLUMN launch_command TEXT;
"#;

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("databaseを開けません: {}", path.display()))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let db = Self(Arc::new(Mutex::new(conn)), Default::default());
        db.migrate()?;
        Ok(db)
    }
    #[cfg(test)]
    pub fn memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Self(Arc::new(Mutex::new(conn)), Default::default());
        db.migrate()?;
        Ok(db)
    }
    pub(crate) fn snapshot_to(&self, destination: &Path) -> Result<String> {
        if destination.exists() {
            std::fs::remove_file(destination).with_context(|| {
                format!(
                    "既存の一時データベースを削除できません: {}",
                    destination.display()
                )
            })?;
        }
        let connection = self.0.lock();
        let snapshot_at = now();
        connection.execute("VACUUM INTO ?1", [destination.to_string_lossy().as_ref()])?;
        Ok(snapshot_at)
    }
    pub(crate) fn backup_summary(&self) -> Result<BackupDataSummary> {
        backup_summary(&self.0.lock())
    }
    fn migrate(&self) -> Result<()> {
        let mut c = self.0.lock();
        let tx = c.transaction()?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS schema_migrations(version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);")?;
        let done: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=1)",
            [],
            |r| r.get(0),
        )?;
        if !done {
            tx.execute_batch(MIGRATION_1)?;
            tx.execute("INSERT INTO schema_migrations VALUES(1,?)", [now()])?;
        }
        let done: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=2)",
            [],
            |r| r.get(0),
        )?;
        if !done {
            tx.execute_batch(MIGRATION_2)?;
            tx.execute("INSERT INTO schema_migrations VALUES(2,?)", [now()])?;
        }
        let done: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=3)",
            [],
            |r| r.get(0),
        )?;
        if !done {
            tx.execute_batch(MIGRATION_3)?;
            tx.execute("INSERT INTO schema_migrations VALUES(3,?)", [now()])?;
        }
        let done: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=4)",
            [],
            |r| r.get(0),
        )?;
        if !done {
            tx.execute_batch(MIGRATION_4)?;
            tx.execute("INSERT INTO schema_migrations VALUES(4,?)", [now()])?;
        }
        let done: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=5)",
            [],
            |r| r.get(0),
        )?;
        if !done {
            tx.execute_batch(MIGRATION_5)?;
            tx.execute("INSERT INTO schema_migrations VALUES(5,?)", [now()])?;
        }
        let done: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=6)",
            [],
            |r| r.get(0),
        )?;
        if !done {
            tx.execute_batch(MIGRATION_6)?;
            let urls = {
                let mut query =
                    tx.prepare("SELECT id,source_url FROM games WHERE source_url IS NOT NULL")?;
                query
                    .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            for (id, url) in urls {
                if let Ok(source) = MetadataIdentity::parse(&url) {
                    tx.execute(
                        "UPDATE games SET metadata_source=? WHERE id=?",
                        params![source.provider(), id],
                    )?;
                }
            }
            tx.execute("INSERT INTO schema_migrations VALUES(6,?)", [now()])?;
        }
        if !tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=7)",
            [],
            |r| r.get::<_, bool>(0),
        )? {
            tx.execute_batch(MIGRATION_7)?;
            tx.execute("INSERT INTO schema_migrations VALUES(7,?)", [now()])?;
        }
        if !tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=8)",
            [],
            |r| r.get::<_, bool>(0),
        )? {
            tx.execute_batch(MIGRATION_8)?;
            tx.execute("INSERT INTO schema_migrations VALUES(8,?)", [now()])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn create_game(&self, input: &CreateGameInput, thumbnail: Option<&str>) -> Result<i64> {
        if input.title.trim().is_empty() {
            bail!("タイトルは必須です")
        };
        let mut c = self.0.lock();
        let tx = c.transaction()?;
        let brand = brand_id(&tx, input.brand.as_deref())?;
        let source = input
            .source
            .as_ref()
            .map(MetadataIdentity::normalized)
            .transpose()?;
        let n = now();
        let launch_command = input
            .launch_command
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        tx.execute("INSERT INTO games(erogamescape_id,vndb_id,metadata_source,title,brand_id,release_date,thumbnail_path,source_url,launch_command,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?)",params![source.as_ref().and_then(MetadataIdentity::erogamescape_id),source.as_ref().and_then(MetadataIdentity::vndb_id),source.as_ref().map(MetadataIdentity::provider),input.title.trim(),brand,input.release_date,thumbnail,source.as_ref().map(MetadataIdentity::url),launch_command,n,n])?;
        let id = tx.last_insert_rowid();
        for p in &input.executable_paths {
            insert_executable(&tx, id, p)?
        }
        tx.commit()?;
        Ok(id)
    }
    pub fn update_game(&self, id: i64, input: &UpdateGameInput) -> Result<()> {
        if input.title.trim().is_empty() {
            bail!("タイトルは必須です")
        };
        let mut c = self.0.lock();
        let tx = c.transaction()?;
        let brand = brand_id(&tx, input.brand.as_deref())?;
        let source_url = input
            .source_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let source = source_url.and_then(|value| MetadataIdentity::parse(value).ok());
        let source_url = source
            .as_ref()
            .map(MetadataIdentity::url)
            .or_else(|| source_url.map(str::to_owned));
        tx.execute(
            "UPDATE games SET title=?,brand_id=?,release_date=?,source_url=?,erogamescape_id=COALESCE(?,erogamescape_id),vndb_id=COALESCE(?,vndb_id),metadata_source=?,updated_at=? WHERE id=?",
            params![input.title.trim(), brand, input.release_date, source_url, source.as_ref().and_then(MetadataIdentity::erogamescape_id),source.as_ref().and_then(MetadataIdentity::vndb_id),source.as_ref().map(MetadataIdentity::provider),now(), id],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn update_game_thumbnail(&self, id: i64, thumbnail: Option<&str>) -> Result<()> {
        let updated = self.0.lock().execute(
            "UPDATE games SET thumbnail_path=?,updated_at=? WHERE id=?",
            params![thumbnail, now(), id],
        )?;
        if updated == 0 {
            bail!("ゲームが見つかりません")
        }
        Ok(())
    }
    pub fn set_launch_command(&self, id: i64, command: Option<&str>) -> Result<()> {
        let command = command.map(str::trim).filter(|value| !value.is_empty());
        let updated = self.0.lock().execute(
            "UPDATE games SET launch_command=?,updated_at=? WHERE id=?",
            params![command, now(), id],
        )?;
        if updated == 0 {
            bail!("ゲームが見つかりません")
        }
        Ok(())
    }
    pub fn delete_game(&self, id: i64) -> Result<()> {
        self.0
            .lock()
            .execute("DELETE FROM games WHERE id=?", [id])?;
        Ok(())
    }
    pub fn add_executable(&self, game: i64, path: &str) -> Result<()> {
        insert_executable(&self.0.lock(), game, path)
    }
    pub fn remove_executable(&self, id: i64) -> Result<()> {
        self.0
            .lock()
            .execute("DELETE FROM game_executables WHERE id=?", [id])?;
        Ok(())
    }
    pub fn launcher_path(&self, game: i64) -> Result<String> {
        self.0
            .lock()
            .query_row(
                "SELECT path FROM game_executables WHERE game_id=? ORDER BY id LIMIT 1",
                [game],
                |row| row.get(0),
            )
            .optional()?
            .context("起動する実行ファイルが登録されていません")
    }
    pub fn launch_command(&self, game: i64) -> Result<Option<String>> {
        Ok(self
            .0
            .lock()
            .query_row(
                "SELECT launch_command FROM games WHERE id=?",
                [game],
                |row| row.get(0),
            )
            .optional()?
            .flatten())
    }
    pub fn registered_executables(&self) -> Result<Vec<(i64, String, String)>> {
        let c = self.0.lock();
        let mut q=c.prepare("SELECT e.game_id,g.title,e.path FROM game_executables e JOIN games g ON g.id=e.game_id")?;
        Ok(q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn list_games(
        &self,
        search: &str,
        brand: Option<&str>,
        play_status: Option<&str>,
        sort: &str,
        descending: bool,
    ) -> Result<Vec<GameSummary>> {
        let order = match sort {
            "title" => "g.title",
            "brand" => "b.name",
            "release_date" => "g.release_date",
            "created_at" => "g.created_at",
            "total_playtime" => "total_playtime_seconds",
            "session_count" => "session_count",
            _ => "last_played",
        };
        let sql = format!(
            "{} ORDER BY {} {} NULLS LAST,g.title COLLATE NOCASE",
            GAME_QUERY,
            order,
            if descending { "DESC" } else { "ASC" }
        );
        let like = format!("%{}%", search);
        let c = self.0.lock();
        let mut q = c.prepare(&sql)?;
        let rows = q.query_map(
            params![like, brand, brand, play_status, play_status],
            game_row,
        )?;
        let mut games = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        let index = self.time_index(&c)?;
        let settings = app_settings(&c)?;
        let at = Utc::now().timestamp();
        for game in &mut games {
            game.total_playtime_seconds = index.total(game.id, at, &settings);
        }
        if sort == "total_playtime" {
            games.sort_by(|a, b| {
                let order = a.total_playtime_seconds.cmp(&b.total_playtime_seconds);
                (if descending { order.reverse() } else { order }).then_with(|| {
                    a.title
                        .to_ascii_lowercase()
                        .cmp(&b.title.to_ascii_lowercase())
                })
            });
        }
        Ok(games)
    }
    pub fn list_brands(&self) -> Result<Vec<String>> {
        let connection = self.0.lock();
        let mut query = connection.prepare(
            "SELECT b.name
             FROM brands b
             WHERE EXISTS(SELECT 1 FROM games g WHERE g.brand_id=b.id)
             ORDER BY b.name COLLATE NOCASE",
        )?;
        Ok(query
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn get_game(&self, id: i64) -> Result<GameDetail> {
        let c = self.0.lock();
        let mut summary = c
            .query_row(
                &format!("{} AND g.id=?", GAME_QUERY),
                params![
                    "%",
                    Option::<String>::None,
                    Option::<String>::None,
                    Option::<String>::None,
                    Option::<String>::None,
                    id
                ],
                game_row,
            )
            .optional()?
            .context("ゲームが見つかりません")?;
        summary.total_playtime_seconds =
            self.time_index(&c)?
                .total(id, Utc::now().timestamp(), &app_settings(&c)?);
        let (eid, vid, provider, url, launch_command) = c.query_row(
            "SELECT erogamescape_id,vndb_id,metadata_source,source_url,launch_command FROM games WHERE id=?",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?;
        let mut q=c.prepare("SELECT id,game_id,path,file_name,created_at FROM game_executables WHERE game_id=? ORDER BY id")?;
        let executables = q
            .query_map([id], |r| {
                Ok(Executable {
                    id: r.get(0)?,
                    game_id: r.get(1)?,
                    path: r.get(2)?,
                    file_name: r.get(3)?,
                    created_at: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(GameDetail {
            summary,
            erogamescape_id: eid,
            vndb_id: vid,
            metadata_source: provider,
            source_url: url,
            executables,
            launch_command,
        })
    }
    pub fn list_sessions(&self, game: i64) -> Result<Vec<PlaySession>> {
        let c = self.0.lock();
        let settings = app_settings(&c)?;
        let index = self.time_index(&c)?;
        let at = Utc::now().timestamp();
        let mut q=c.prepare("SELECT id,game_id,launched_at,exited_at,needs_review FROM play_sessions WHERE game_id=? ORDER BY launched_at DESC")?;
        let rows = q.query_map([game], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, bool>(4)?,
            ))
        })?;
        rows.map(|r| {
            let (id, game_id, launched_at, exited_at, needs_review) = r?;
            let time = &index.sessions[&id];
            Ok(PlaySession {
                id,
                game_id,
                launched_at,
                exited_at,
                needs_review,
                playtime_seconds: time.playtime(i64::MIN, i64::MAX, at, &settings, &index.pause),
                background_seconds: time.background_seconds(at),
                running_seconds: time.end.map(|end| end - time.start),
                pause_excluded_seconds: time.pause_excluded_seconds(at, &settings, &index.pause),
            })
        })
        .collect()
    }
    pub fn intervals(&self, session: i64) -> Result<Vec<BackgroundInterval>> {
        let c = self.0.lock();
        let mut q=c.prepare("SELECT id,play_session_id,started_at,ended_at FROM background_intervals WHERE play_session_id=? ORDER BY started_at")?;
        Ok(q.query_map([session], |r| {
            Ok(BackgroundInterval {
                id: r.get(0)?,
                play_session_id: r.get(1)?,
                started_at: r.get(2)?,
                ended_at: r.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?)
    }
    pub fn start_session(&self, game: i64, at: &str) -> Result<i64> {
        let n = now();
        let c = self.0.lock();
        c.execute(
            "INSERT INTO play_sessions(game_id,launched_at,created_at,updated_at,background_migrated) VALUES(?,?,?,?,1)",
            params![game, at, n, n],
        )?;
        Ok(c.last_insert_rowid())
    }
    pub fn end_session(&self, id: i64, at: &str) -> Result<()> {
        let mut c = self.0.lock();
        let tx = c.transaction()?;
        tx.execute("UPDATE focus_intervals SET ended_at=?,updated_at=? WHERE play_session_id=? AND ended_at IS NULL",params![at,now(),id])?;
        tx.execute("UPDATE background_intervals SET ended_at=?,updated_at=? WHERE play_session_id=? AND ended_at IS NULL",params![at,now(),id])?;
        tx.execute(
            "UPDATE play_sessions SET exited_at=?,updated_at=? WHERE id=?",
            params![at, now(), id],
        )?;
        rebuild_focus_mirror(&tx, id)?;
        tx.commit()?;
        Ok(())
    }
    pub fn start_interval(&self, session: i64, at: &str) -> Result<i64> {
        validate_interval(
            &self.0.lock(),
            "background_intervals",
            session,
            None,
            at,
            None,
        )?;
        let n = now();
        let c = self.0.lock();
        c.execute("INSERT INTO background_intervals(play_session_id,started_at,created_at,updated_at) VALUES(?,?,?,?)",params![session,at,n,n])?;
        Ok(c.last_insert_rowid())
    }
    pub fn end_interval(&self, id: i64, at: &str) -> Result<()> {
        let c = self.0.lock();
        let (session, start): (i64, String) = c.query_row(
            "SELECT play_session_id,started_at FROM background_intervals WHERE id=?",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        validate_interval(
            &c,
            "background_intervals",
            session,
            Some(id),
            &start,
            Some(at),
        )?;
        if start == at {
            c.execute("DELETE FROM background_intervals WHERE id=?", [id])?;
        } else {
            c.execute(
                "UPDATE background_intervals SET ended_at=?,updated_at=? WHERE id=?",
                params![at, now(), id],
            )?;
        }
        Ok(())
    }
    pub fn start_legacy_focus_interval(&self, session: i64, at: &str) -> Result<i64> {
        validate_interval(&self.0.lock(), "focus_intervals", session, None, at, None)?;
        let n = now();
        let c = self.0.lock();
        c.execute("INSERT INTO focus_intervals(play_session_id,started_at,created_at,updated_at) VALUES(?,?,?,?)",params![session,at,n,n])?;
        Ok(c.last_insert_rowid())
    }
    pub fn end_legacy_focus_interval(&self, id: i64, at: &str) -> Result<()> {
        let c = self.0.lock();
        let (session, start): (i64, String) = c.query_row(
            "SELECT play_session_id,started_at FROM focus_intervals WHERE id=?",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        validate_interval(&c, "focus_intervals", session, Some(id), &start, Some(at))?;
        if start == at {
            c.execute("DELETE FROM focus_intervals WHERE id=?", [id])?;
        } else {
            c.execute(
                "UPDATE focus_intervals SET ended_at=?,updated_at=? WHERE id=?",
                params![at, now(), id],
            )?;
        }
        Ok(())
    }
    pub fn manual_session(&self, game: i64, start: &str, end: &str) -> Result<i64> {
        parse_range(start, end)?;
        let mut c = self.0.lock();
        let tx = c.transaction()?;
        validate_session_range(&tx, game, None, start, Some(end))?;
        let n = now();
        tx.execute("INSERT INTO play_sessions(game_id,launched_at,exited_at,created_at,updated_at,background_migrated) VALUES(?,?,?,?,?,1)",params![game,start,end,n,n])?;
        let id = tx.last_insert_rowid();
        tx.execute("INSERT INTO focus_intervals(play_session_id,started_at,ended_at,created_at,updated_at) VALUES(?,?,?,?,?)",params![id,start,end,n,n])?;
        tx.commit()?;
        Ok(id)
    }
    pub fn update_session(&self, id: i64, start: &str, end: Option<&str>) -> Result<()> {
        DateTime::parse_from_rfc3339(start).context("開始日時が不正です")?;
        let mut c = self.0.lock();
        let tx = c.transaction()?;
        let (game, existing_end): (i64, Option<String>) = tx.query_row(
            "SELECT game_id,exited_at FROM play_sessions WHERE id=?",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if existing_end.is_some() != end.is_some() {
            bail!("セッションの実行状態は手動変更できません")
        }
        if let Some(end) = end {
            parse_range(start, end)?;
        }
        validate_session_range(&tx, game, Some(id), start, end)?;
        let invalid:i64=tx.query_row("SELECT COUNT(*) FROM background_intervals WHERE play_session_id=? AND (started_at < ? OR (? IS NOT NULL AND (ended_at IS NULL OR ended_at > ?)))",params![id,start,end,end],|r|r.get(0))?;
        if invalid > 0 {
            bail!("バックグラウンド区間がSession範囲外になります")
        };
        tx.execute("UPDATE play_sessions SET launched_at=?,exited_at=?,needs_review=0,updated_at=? WHERE id=?",params![start,end,now(),id])?;
        rebuild_focus_mirror(&tx, id)?;
        tx.commit()?;
        Ok(())
    }
    pub fn confirm_session_review(&self, id: i64) -> Result<()> {
        let c = self.0.lock();
        let updated = c.execute(
            "UPDATE play_sessions SET needs_review=0,updated_at=? WHERE id=? AND needs_review<>0",
            params![now(), id],
        )?;
        if updated == 0 {
            let exists: bool = c.query_row(
                "SELECT EXISTS(SELECT 1 FROM play_sessions WHERE id=?)",
                [id],
                |row| row.get(0),
            )?;
            if !exists {
                bail!("セッションが見つかりません")
            }
        }
        Ok(())
    }
    pub fn delete_session(&self, id: i64) -> Result<()> {
        self.0
            .lock()
            .execute("DELETE FROM play_sessions WHERE id=?", [id])?;
        Ok(())
    }
    pub fn delete_game_sessions(&self, game: i64) -> Result<usize> {
        Ok(self
            .0
            .lock()
            .execute("DELETE FROM play_sessions WHERE game_id=?", [game])?)
    }
    pub fn create_interval(&self, session: i64, start: &str, end: &str) -> Result<i64> {
        parse_range(start, end)?;
        let mut c = self.0.lock();
        let tx = c.transaction()?;
        validate_interval(&tx, "background_intervals", session, None, start, Some(end))?;
        let n = now();
        tx.execute("INSERT INTO background_intervals(play_session_id,started_at,ended_at,created_at,updated_at) VALUES(?,?,?,?,?)",params![session,start,end,n,n])?;
        let id = tx.last_insert_rowid();
        rebuild_focus_mirror(&tx, session)?;
        tx.commit()?;
        Ok(id)
    }
    pub fn update_interval(&self, id: i64, start: &str, end: Option<&str>) -> Result<()> {
        DateTime::parse_from_rfc3339(start).context("開始日時が不正です")?;
        let mut c = self.0.lock();
        let tx = c.transaction()?;
        let (session, existing_end): (i64, Option<String>) = tx.query_row(
            "SELECT play_session_id,ended_at FROM background_intervals WHERE id=?",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if existing_end.is_some() != end.is_some() {
            bail!("バックグラウンド区間の記録状態は手動変更できません")
        }
        validate_interval(&tx, "background_intervals", session, Some(id), start, end)?;
        tx.execute(
            "UPDATE background_intervals SET started_at=?,ended_at=?,updated_at=? WHERE id=?",
            params![start, end, now(), id],
        )?;
        rebuild_focus_mirror(&tx, session)?;
        tx.commit()?;
        Ok(())
    }
    pub fn delete_interval(&self, id: i64) -> Result<()> {
        let mut c = self.0.lock();
        let tx = c.transaction()?;
        let session: i64 = tx.query_row(
            "SELECT play_session_id FROM background_intervals WHERE id=?",
            [id],
            |r| r.get(0),
        )?;
        tx.execute("DELETE FROM background_intervals WHERE id=?", [id])?;
        rebuild_focus_mirror(&tx, session)?;
        tx.commit()?;
        Ok(())
    }
    pub fn update_play_status(&self, game: i64, status: &str) -> Result<()> {
        if !matches!(status, "unplayed" | "playing" | "completed" | "retired") {
            bail!("未対応のプレイ状況です")
        }
        let changed = self.0.lock().execute(
            "UPDATE games SET play_status=?,updated_at=? WHERE id=?",
            params![status, now(), game],
        )?;
        if changed == 0 {
            bail!("ゲームが見つかりません")
        }
        Ok(())
    }
    pub fn create_timestamp(&self, game: i64, name: &str, marked_at: &str) -> Result<i64> {
        let name = validate_timestamp_name(name)?;
        DateTime::parse_from_rfc3339(marked_at).context("記録日時が不正です")?;
        let c = self.0.lock();
        let n = now();
        c.execute(
            "INSERT INTO game_timestamps(game_id,name,marked_at,created_at) VALUES(?,?,?,?)",
            params![game, name, marked_at, n],
        )?;
        Ok(c.last_insert_rowid())
    }
    pub fn update_timestamp(&self, id: i64, name: &str, marked_at: &str) -> Result<()> {
        let name = validate_timestamp_name(name)?;
        DateTime::parse_from_rfc3339(marked_at).context("記録日時が不正です")?;
        let changed = self.0.lock().execute(
            "UPDATE game_timestamps SET name=?,marked_at=? WHERE id=?",
            params![name, marked_at, id],
        )?;
        if changed == 0 {
            bail!("タイムスタンプが見つかりません")
        }
        Ok(())
    }
    pub fn timestamps(&self, game: i64) -> Result<Vec<GameTimestamp>> {
        let c = self.0.lock();
        let settings = app_settings(&c)?;
        let index = self.time_index(&c)?;
        let at = Utc::now().timestamp();
        let mut query = c.prepare(
            "SELECT id,game_id,name,marked_at FROM game_timestamps WHERE game_id=? ORDER BY marked_at,id",
        )?;
        let rows: Vec<(i64, i64, String, String)> = query
            .query_map([game], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let points = rows
            .iter()
            .map(|row| timestamp_seconds(&row.3))
            .collect::<Result<Vec<_>>>()?;
        let cumulative = index.cumulative(game, &points, at, &settings);
        let mut previous = 0;
        rows.into_iter()
            .zip(cumulative)
            .map(|((id, game_id, name, marked_at), playtime_seconds)| {
                let since_previous_seconds = (playtime_seconds - previous).max(0);
                previous = playtime_seconds;
                Ok(GameTimestamp {
                    id,
                    game_id,
                    name,
                    marked_at,
                    playtime_seconds,
                    since_previous_seconds,
                })
            })
            .collect()
    }
    pub fn delete_timestamp(&self, id: i64) -> Result<()> {
        self.0
            .lock()
            .execute("DELETE FROM game_timestamps WHERE id=?", [id])?;
        Ok(())
    }
    pub fn recover_orphans(&self, at: &str) -> Result<usize> {
        let mut c = self.0.lock();
        let tx = c.transaction()?;
        let n = tx.execute(
            "UPDATE focus_intervals SET ended_at=?,updated_at=? WHERE ended_at IS NULL",
            params![at, now()],
        )?;
        tx.execute(
            "UPDATE background_intervals SET ended_at=?,updated_at=? WHERE ended_at IS NULL",
            params![at, now()],
        )?;
        tx.execute("UPDATE play_sessions SET exited_at=?,needs_review=1,updated_at=? WHERE exited_at IS NULL",params![at,now()])?;
        close_open_pauses(&tx, at)?;
        tx.commit()?;
        Ok(n)
    }
    pub fn migrate_focus_intervals(&self) -> Result<usize> {
        let mut c = self.0.lock();
        let tx = c.transaction()?;
        let sessions: Vec<(i64, String, String)> = {
            let mut query = tx.prepare("SELECT id,launched_at,exited_at FROM play_sessions WHERE background_migrated=0 AND exited_at IS NOT NULL ORDER BY id")?;
            query
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<rusqlite::Result<_>>()?
        };
        for (session, start, end) in &sessions {
            let focus = load_ranges(&tx, "focus_intervals", *session)?;
            insert_complement(&tx, "background_intervals", *session, start, end, &focus)?;
            let background = load_ranges(&tx, "background_intervals", *session)?;
            let total = range_seconds(start, end)?;
            let foreground_seconds: i64 = focus
                .iter()
                .map(|(start, end)| range_seconds(start, end))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .sum();
            let background_seconds: i64 = background
                .iter()
                .map(|(start, end)| range_seconds(start, end))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .sum();
            if total - background_seconds != foreground_seconds {
                bail!("セッション {session} の旧プレイ時間を安全に移行できません")
            }
            tx.execute(
                "UPDATE play_sessions SET background_migrated=1,updated_at=? WHERE id=?",
                params![now(), session],
            )?;
        }
        tx.commit()?;
        Ok(sessions.len())
    }
    pub fn statistics(&self, input: &StatisticsPeriodInput) -> Result<StatisticsReport> {
        self.statistics_for(input, None)
    }
    pub fn game_statistics(&self, game_id: i64) -> Result<StatisticsReport> {
        let report = self.statistics_for(
            &StatisticsPeriodInput {
                kind: "all".into(),
                year: None,
                month: None,
                days: None,
            },
            Some(game_id),
        )?;
        Ok(trim_statistics_to_active_range(report))
    }
    fn statistics_for(
        &self,
        input: &StatisticsPeriodInput,
        game_id: Option<i64>,
    ) -> Result<StatisticsReport> {
        let snapshot = Utc::now();
        let snapshot_text = snapshot.to_rfc3339();
        let connection = self.0.lock();
        let calculation_settings = app_settings(&connection)?;
        let pause_coverage = self.time_index(&connection)?.pause.clone();
        if let Some(game_id) = game_id {
            let game_exists: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM games WHERE id=?)",
                params![game_id],
                |row| row.get(0),
            )?;
            if !game_exists {
                bail!("ゲームが見つかりません")
            }
        }
        let earliest_session: Option<String> = connection
            .query_row(
                "SELECT launched_at FROM play_sessions
                 WHERE (?1 IS NULL OR game_id=?1)
                 ORDER BY CAST(strftime('%s',launched_at) AS INTEGER),id LIMIT 1",
                params![game_id],
                |row| row.get(0),
            )
            .optional()?;
        let latest_session: Option<String> = if game_id.is_some() {
            connection
                .query_row(
                    "SELECT COALESCE(exited_at,?2) FROM play_sessions
                     WHERE game_id=?1
                     ORDER BY CAST(strftime('%s',COALESCE(exited_at,?2)) AS INTEGER) DESC,id DESC LIMIT 1",
                    params![game_id, snapshot_text],
                    |row| row.get(0),
                )
                .optional()?
        } else {
            None
        };
        let (period, days, available_years) = statistics_period(
            input,
            snapshot,
            earliest_session.as_deref(),
            latest_session.as_deref(),
        )?;
        let range_start = days
            .first()
            .map(|day| day.start)
            .context("統計期間の開始日を決定できません")?;
        let range_end = days
            .last()
            .map(|day| day.end)
            .context("統計期間の終了日を決定できません")?;
        let mut query = connection.prepare(
            "SELECT s.id,s.game_id,g.title,br.name,g.thumbnail_path,s.launched_at,s.exited_at,s.needs_review,b.started_at,b.ended_at
             FROM play_sessions s
             JOIN games g ON g.id=s.game_id
             LEFT JOIN brands br ON br.id=g.brand_id
             LEFT JOIN background_intervals b ON b.play_session_id=s.id
             WHERE CAST(strftime('%s',s.launched_at) AS INTEGER) < ?
               AND CAST(strftime('%s',COALESCE(s.exited_at,?)) AS INTEGER) > ?
               AND (?4 IS NULL OR s.game_id=?4)
             ORDER BY CAST(strftime('%s',s.launched_at) AS INTEGER),s.id,CAST(strftime('%s',b.started_at) AS INTEGER)",
        )?;
        let mut rows = query.query(params![range_end, snapshot_text, range_start, game_id])?;
        let mut sessions: Vec<RawStatisticsSession> = Vec::new();
        while let Some(row) = rows.next()? {
            let session_id: i64 = row.get(0)?;
            if sessions
                .last()
                .is_none_or(|session| session.id != session_id)
            {
                let launched_at: String = row.get(5)?;
                let exited_at: Option<String> = row.get(6)?;
                sessions.push(RawStatisticsSession {
                    id: session_id,
                    game_id: row.get(1)?,
                    title: row.get(2)?,
                    brand: row.get(3)?,
                    thumbnail_path: row.get(4)?,
                    start: timestamp_seconds(&launched_at)?,
                    end: exited_at
                        .as_deref()
                        .map(timestamp_seconds)
                        .transpose()?
                        .unwrap_or(snapshot.timestamp()),
                    launched_at,
                    exited_at,
                    needs_review: row.get::<_, i64>(7)? != 0,
                    background: Vec::new(),
                });
            }
            let background_start: Option<String> = row.get(8)?;
            if let Some(background_start) = background_start {
                let background_end: Option<String> = row.get(9)?;
                sessions
                    .last_mut()
                    .context("統計セッションを構築できません")?
                    .background
                    .push((
                        timestamp_seconds(&background_start)?,
                        background_end
                            .as_deref()
                            .map(timestamp_seconds)
                            .transpose()?
                            .unwrap_or(snapshot.timestamp()),
                    ));
            }
        }
        Ok(aggregate_statistics(
            period,
            days,
            sessions,
            available_years,
            calculation_settings.exclude_background_time,
            &pause_coverage,
            calculation_settings.exclude_pause_time,
        ))
    }
    pub fn settings(&self) -> Result<AppSettings> {
        app_settings(&self.0.lock())
    }
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .0
            .lock()
            .query_row("SELECT value FROM settings WHERE key=?", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let connection = self.0.lock();
        connection.execute("INSERT INTO settings(key,value,updated_at) VALUES(?,?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",params![key,value,now()])?;
        if key == "last_seen" {
            self.1.lock().ignored_changes += 1;
        }
        Ok(())
    }
    pub fn add_screenshot(
        &self,
        game_id: i64,
        session_id: Option<i64>,
        path: &str,
        captured_at: &str,
        width: i64,
        height: i64,
    ) -> Result<i64> {
        let c = self.0.lock();
        c.execute(
            "INSERT INTO game_screenshots(game_id,play_session_id,path,captured_at,width,height) VALUES(?,?,?,?,?,?)",
            params![game_id, session_id, path, captured_at, width, height],
        )?;
        Ok(c.last_insert_rowid())
    }
    pub fn screenshots(&self, game_id: i64) -> Result<Vec<GameScreenshot>> {
        let c = self.0.lock();
        let mut q = c.prepare("SELECT id,game_id,play_session_id,path,captured_at,width,height FROM game_screenshots WHERE game_id=? ORDER BY captured_at DESC,id DESC")?;
        Ok(q.query_map([game_id], |r| {
            Ok(GameScreenshot {
                id: r.get(0)?,
                game_id: r.get(1)?,
                play_session_id: r.get(2)?,
                path: r.get(3)?,
                captured_at: r.get(4)?,
                width: r.get(5)?,
                height: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn screenshot_path(&self, id: i64) -> Result<Option<String>> {
        Ok(self
            .0
            .lock()
            .query_row("SELECT path FROM game_screenshots WHERE id=?", [id], |r| {
                r.get(0)
            })
            .optional()?)
    }
    pub fn remove_screenshot(&self, id: i64) -> Result<Option<String>> {
        let mut c = self.0.lock();
        let tx = c.transaction()?;
        let path = tx
            .query_row("SELECT path FROM game_screenshots WHERE id=?", [id], |r| {
                r.get(0)
            })
            .optional()?;
        if path.is_some() {
            tx.execute("DELETE FROM game_screenshots WHERE id=?", [id])?;
        }
        tx.commit()?;
        Ok(path)
    }
    pub fn metadata_identity(&self, id: i64) -> Result<MetadataIdentity> {
        let (provider, eid, vid, url) = self
            .0
            .lock()
            .query_row(
                "SELECT metadata_source,erogamescape_id,vndb_id,source_url FROM games WHERE id=?",
                [id],
                |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, Option<i64>>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?
            .context("ゲームが見つかりません")?;
        let source = url
            .as_deref()
            .and_then(|url| MetadataIdentity::parse(url).ok())
            .filter(|source| Some(source.provider()) == provider.as_deref())
            .or_else(|| match provider.as_deref() {
                Some("erogamescape") => eid.map(MetadataIdentity::ErogameScape),
                Some("vndb") => vid.map(MetadataIdentity::Vndb),
                _ => None,
            });
        source
            .context("情報取得元のURL・IDがありません")?
            .normalized()
    }
    pub fn apply_metadata(
        &self,
        id: i64,
        m: &crate::metadata::GameMetadata,
        thumb: Option<&str>,
    ) -> Result<()> {
        let mut c = self.0.lock();
        let tx = c.transaction()?;
        let brand = brand_id(&tx, m.brand.as_deref())?;
        let source = m.source.normalized()?;
        tx.execute("UPDATE games SET erogamescape_id=COALESCE(?,erogamescape_id),vndb_id=COALESCE(?,vndb_id),metadata_source=?,title=?,brand_id=?,release_date=?,thumbnail_path=COALESCE(?,thumbnail_path),source_url=?,updated_at=? WHERE id=?",params![source.erogamescape_id(),source.vndb_id(),source.provider(),m.title,brand,m.release_date,thumb,source.url(),now(),id])?;
        tx.commit()?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct StatisticsDayBoundary {
    date: NaiveDate,
    start: i64,
    end: i64,
}

#[derive(Debug)]
struct RawStatisticsSession {
    id: i64,
    game_id: i64,
    title: String,
    brand: Option<String>,
    thumbnail_path: Option<String>,
    launched_at: String,
    exited_at: Option<String>,
    needs_review: bool,
    start: i64,
    end: i64,
    background: Vec<(i64, i64)>,
}

#[derive(Default)]
struct StatisticsDayAccum {
    playtime_seconds: i64,
    games: BTreeMap<i64, StatisticsDayGameAccum>,
}

struct StatisticsDayGameAccum {
    title: String,
    thumbnail_path: Option<String>,
    playtime_seconds: i64,
}

struct StatisticsGameAccum {
    title: String,
    brand: Option<String>,
    thumbnail_path: Option<String>,
    playtime_seconds: i64,
    session_count: i64,
}

fn statistics_period(
    input: &StatisticsPeriodInput,
    snapshot: DateTime<Utc>,
    earliest_session: Option<&str>,
    latest_session: Option<&str>,
) -> Result<(StatisticsPeriod, Vec<StatisticsDayBoundary>, Vec<i32>)> {
    let today = snapshot.with_timezone(&Local).date_naive();
    let earliest_date = earliest_session
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .context("最初のセッション日時が不正です")
                .map(|value| value.with_timezone(&Local).date_naive())
        })
        .transpose()?
        .unwrap_or(today)
        .min(today);
    let latest_date = latest_session
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .context("最後のセッション日時が不正です")
                .map(|value| value.with_timezone(&Local).date_naive())
        })
        .transpose()?
        .unwrap_or(today)
        .min(today)
        .max(earliest_date);
    let available_years = (earliest_date.year()..=today.year()).rev().collect();
    let (start_date, last_date, year, month, recent_days) = match input.kind.as_str() {
        "month" => {
            let year = input.year.context("年を指定してください")?;
            let month = input.month.context("月を指定してください")?;
            let start =
                NaiveDate::from_ymd_opt(year, month, 1).context("統計期間の年月が不正です")?;
            if start > today {
                bail!("未来の月は選択できません")
            }
            let next = next_month(start)?;
            (
                start,
                (next - Duration::days(1)).min(today),
                Some(year),
                Some(month),
                None,
            )
        }
        "year" => {
            let year = input.year.context("年を指定してください")?;
            let start = NaiveDate::from_ymd_opt(year, 1, 1).context("統計期間の年が不正です")?;
            if start > today {
                bail!("未来の年は選択できません")
            }
            let next = NaiveDate::from_ymd_opt(year + 1, 1, 1)
                .context("統計期間の翌年を決定できません")?;
            (
                start,
                (next - Duration::days(1)).min(today),
                Some(year),
                None,
                None,
            )
        }
        "recent" => {
            let days = input.days.context("日数を指定してください")?;
            if !RECENT_RANGE_DAYS.contains(&days) {
                bail!("統計期間の日数が不正です")
            }
            // 今日を最終日として days 日分を遡る。記録が無い日も 0 分として並べる。
            let start = today - Duration::days(i64::from(days) - 1);
            (start, today, None, None, Some(days))
        }
        "all" => (earliest_date, latest_date, None, None, None),
        _ => bail!("統計期間の種類が不正です"),
    };
    let mut days = Vec::new();
    let mut date = start_date;
    while date <= last_date {
        let start = local_day_start(date)?;
        let end = local_day_start(date + Duration::days(1))?.min(snapshot.timestamp());
        days.push(StatisticsDayBoundary { date, start, end });
        date += Duration::days(1);
    }
    Ok((
        StatisticsPeriod {
            kind: input.kind.clone(),
            year,
            month,
            days: recent_days,
            start_date: start_date.to_string(),
            end_date: last_date.to_string(),
            generated_at: snapshot.to_rfc3339(),
        },
        days,
        available_years,
    ))
}

fn next_month(date: NaiveDate) -> Result<NaiveDate> {
    let (year, month) = if date.month() == 12 {
        (date.year() + 1, 1)
    } else {
        (date.year(), date.month() + 1)
    };
    NaiveDate::from_ymd_opt(year, month, 1).context("統計期間の翌月を決定できません")
}

fn local_day_start(date: NaiveDate) -> Result<i64> {
    let midnight = date
        .and_hms_opt(0, 0, 0)
        .context("ローカル日付の開始時刻を決定できません")?;
    for minutes in 0..=24 * 60 {
        let candidate = midnight + Duration::minutes(minutes);
        match Local.from_local_datetime(&candidate) {
            LocalResult::Single(value) => return Ok(value.timestamp()),
            LocalResult::Ambiguous(first, second) => {
                return Ok(first.timestamp().min(second.timestamp()));
            }
            LocalResult::None => {}
        }
    }
    bail!("ローカル日付の開始時刻を決定できません")
}

// Share settings decoding across commands and queries without relocking the connection.
fn app_settings(connection: &Connection) -> Result<AppSettings> {
    let value: Option<String> = connection
        .query_row("SELECT value FROM settings WHERE key='app'", [], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(value
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default())
}

fn timestamp_seconds(value: &str) -> Result<i64> {
    Ok(DateTime::parse_from_rfc3339(value)
        .context("統計対象の日時が不正です")?
        .timestamp())
}

fn overlap_seconds(first_start: i64, first_end: i64, second_start: i64, second_end: i64) -> i64 {
    (first_end.min(second_end) - first_start.max(second_start)).max(0)
}

fn aggregate_statistics(
    period: StatisticsPeriod,
    day_boundaries: Vec<StatisticsDayBoundary>,
    sessions: Vec<RawStatisticsSession>,
    available_years: Vec<i32>,
    exclude_background_time: bool,
    pause: &Coverage,
    exclude_pause_time: bool,
) -> StatisticsReport {
    let mut day_values: Vec<StatisticsDayAccum> = day_boundaries
        .iter()
        .map(|_| StatisticsDayAccum::default())
        .collect();
    let mut games: HashMap<i64, StatisticsGameAccum> = HashMap::new();
    let mut session_count = 0_i64;
    let mut needs_review_session_count = 0_i64;
    let mut longest_session: Option<StatisticsSessionHighlight> = None;

    let settings = AppSettings {
        exclude_background_time,
        exclude_pause_time,
        ..Default::default()
    };
    for session in sessions {
        let calculation = SessionTime::new(
            session.start,
            Some(session.end),
            session.background.clone(),
            pause,
        );
        let mut session_playtime = 0_i64;
        let first_day = day_boundaries.partition_point(|day| day.end <= session.start);
        for (index, day) in day_boundaries.iter().enumerate().skip(first_day) {
            if day.start >= session.end {
                break;
            }
            let running = overlap_seconds(session.start, session.end, day.start, day.end);
            if running == 0 {
                continue;
            }
            let playtime = calculation.playtime(day.start, day.end, session.end, &settings, pause);
            if playtime == 0 {
                continue;
            }
            session_playtime += playtime;
            day_values[index].playtime_seconds += playtime;
            let day_game = day_values[index]
                .games
                .entry(session.game_id)
                .or_insert_with(|| StatisticsDayGameAccum {
                    title: session.title.clone(),
                    thumbnail_path: session.thumbnail_path.clone(),
                    playtime_seconds: 0,
                });
            day_game.playtime_seconds += playtime;
        }
        if session_playtime == 0 {
            continue;
        }
        session_count += 1;
        if session.needs_review {
            needs_review_session_count += 1;
        }
        let game = games
            .entry(session.game_id)
            .or_insert_with(|| StatisticsGameAccum {
                title: session.title.clone(),
                brand: session.brand.clone(),
                thumbnail_path: session.thumbnail_path.clone(),
                playtime_seconds: 0,
                session_count: 0,
            });
        game.playtime_seconds += session_playtime;
        game.session_count += 1;
        let highlight = StatisticsSessionHighlight {
            session_id: session.id,
            game_id: session.game_id,
            title: session.title,
            launched_at: session.launched_at,
            exited_at: session.exited_at,
            playtime_seconds: session_playtime,
            needs_review: session.needs_review,
        };
        if longest_session
            .as_ref()
            .is_none_or(|current| highlight.playtime_seconds > current.playtime_seconds)
        {
            longest_session = Some(highlight);
        }
    }

    let total_playtime_seconds: i64 = day_values.iter().map(|day| day.playtime_seconds).sum();
    let active_day_count = day_values
        .iter()
        .filter(|day| day.playtime_seconds > 0)
        .count() as i64;
    let mut busiest_day: Option<StatisticsDayHighlight> = None;
    let mut active_days_by_game: HashMap<i64, i64> = HashMap::new();
    for (boundary, day) in day_boundaries.iter().zip(&day_values) {
        if day.playtime_seconds > 0
            && busiest_day
                .as_ref()
                .is_none_or(|current| day.playtime_seconds > current.playtime_seconds)
        {
            busiest_day = Some(StatisticsDayHighlight {
                date: boundary.date.to_string(),
                playtime_seconds: day.playtime_seconds,
            });
        }
        for game_id in day.games.keys() {
            *active_days_by_game.entry(*game_id).or_default() += 1;
        }
    }

    let days: Vec<StatisticsDay> = day_boundaries
        .into_iter()
        .zip(day_values)
        .map(|(boundary, day)| {
            let mut day_games: Vec<StatisticsDayGame> = day
                .games
                .into_iter()
                .map(|(game_id, game)| StatisticsDayGame {
                    game_id,
                    title: game.title,
                    thumbnail_path: game.thumbnail_path,
                    playtime_seconds: game.playtime_seconds,
                })
                .collect();
            day_games.sort_by(|left, right| {
                right
                    .playtime_seconds
                    .cmp(&left.playtime_seconds)
                    .then_with(|| left.game_id.cmp(&right.game_id))
            });
            StatisticsDay {
                date: boundary.date.to_string(),
                playtime_seconds: day.playtime_seconds,
                games: day_games,
            }
        })
        .collect();
    let mut games: Vec<StatisticsGame> = games
        .into_iter()
        .map(|(game_id, game)| StatisticsGame {
            game_id,
            title: game.title,
            brand: game.brand,
            thumbnail_path: game.thumbnail_path,
            playtime_seconds: game.playtime_seconds,
            session_count: game.session_count,
            active_day_count: active_days_by_game.get(&game_id).copied().unwrap_or(0),
        })
        .collect();
    games.sort_by(|left, right| {
        right
            .playtime_seconds
            .cmp(&left.playtime_seconds)
            .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
    });
    let calendar_day_count = days.len() as i64;
    StatisticsReport {
        period,
        summary: StatisticsSummary {
            total_playtime_seconds,
            active_day_count,
            average_per_day_seconds: total_playtime_seconds / calendar_day_count.max(1),
            game_count: games.len() as i64,
            session_count,
            average_session_seconds: total_playtime_seconds / session_count.max(1),
            longest_session,
            busiest_day,
            needs_review_session_count,
        },
        days,
        games,
        available_years,
    }
}

fn trim_statistics_to_active_range(mut report: StatisticsReport) -> StatisticsReport {
    let Some(first) = report.days.iter().position(|day| day.playtime_seconds > 0) else {
        report.days.clear();
        report.summary.average_per_day_seconds = 0;
        return report;
    };
    let last = report
        .days
        .iter()
        .rposition(|day| day.playtime_seconds > 0)
        .unwrap_or(first);
    report.days = report.days.drain(first..=last).collect();
    report.period.start_date = report.days[0].date.clone();
    report.period.end_date = report.days[report.days.len() - 1].date.clone();
    report.summary.average_per_day_seconds =
        report.summary.total_playtime_seconds / report.days.len() as i64;
    report
}

/// 「最近」の集計で選べる日数。UI のドロップダウンと一致させる。
const RECENT_RANGE_DAYS: [u32; 4] = [7, 14, 30, 90];

const GAME_QUERY: &str = "SELECT g.id,g.title,b.name,g.release_date,g.thumbnail_path,g.created_at,
0 total_playtime_seconds,
COALESCE((SELECT SUM(MAX(0,CAST(strftime('%s',COALESCE(s.exited_at,'now')) AS INTEGER)-CAST(strftime('%s',s.launched_at) AS INTEGER))) FROM play_sessions s WHERE s.game_id=g.id),0) total_running_seconds,
(SELECT MAX(COALESCE(f.ended_at,f.started_at)) FROM focus_intervals f JOIN play_sessions fs ON fs.id=f.play_session_id WHERE fs.game_id=g.id) last_played,
(SELECT COUNT(*) FROM play_sessions s WHERE s.game_id=g.id) session_count,
g.play_status
FROM games g LEFT JOIN brands b ON b.id=g.brand_id WHERE g.title LIKE ? AND (? IS NULL OR b.name=?) AND (? IS NULL OR g.play_status=?)";
fn game_row(r: &rusqlite::Row) -> rusqlite::Result<GameSummary> {
    Ok(GameSummary {
        id: r.get(0)?,
        title: r.get(1)?,
        brand: r.get(2)?,
        release_date: r.get(3)?,
        thumbnail_path: r.get(4)?,
        created_at: r.get(5)?,
        total_playtime_seconds: r.get(6)?,
        total_running_seconds: r.get(7)?,
        last_played: r.get(8)?,
        session_count: r.get(9)?,
        play_status: r.get(10)?,
    })
}
fn backup_summary(connection: &Connection) -> Result<BackupDataSummary> {
    Ok(BackupDataSummary {
        game_count: connection.query_row("SELECT COUNT(*) FROM games", [], |row| row.get(0))?,
        session_count: connection
            .query_row("SELECT COUNT(*) FROM play_sessions", [], |row| row.get(0))?,
        timestamp_count: connection.query_row(
            "SELECT COUNT(*) FROM game_timestamps",
            [],
            |row| row.get(0),
        )?,
        screenshot_count: connection.query_row(
            "SELECT COUNT(*) FROM game_screenshots",
            [],
            |row| row.get(0),
        )?,
        thumbnail_count: connection.query_row(
            "SELECT COUNT(*) FROM games WHERE thumbnail_path IS NOT NULL",
            [],
            |row| row.get(0),
        )?,
    })
}

fn now() -> String {
    Utc::now().to_rfc3339()
}
fn parse_range(start: &str, end: &str) -> Result<()> {
    let s = DateTime::parse_from_rfc3339(start).context("開始日時が不正です")?;
    let e = DateTime::parse_from_rfc3339(end).context("終了日時が不正です")?;
    if e < s {
        bail!("終了日時は開始日時以降にしてください")
    };
    Ok(())
}
fn range_seconds(start: &str, end: &str) -> Result<i64> {
    let start = DateTime::parse_from_rfc3339(start).context("開始日時が不正です")?;
    let end = DateTime::parse_from_rfc3339(end).context("終了日時が不正です")?;
    // Match SQLite strftime('%s') exactly. Subtracting the chrono Duration and
    // truncating each interval independently loses fractional seconds and can
    // make a perfectly partitioned session fail migration validation.
    Ok(end.timestamp() - start.timestamp())
}
fn brand_id(c: &Connection, name: Option<&str>) -> Result<Option<i64>> {
    let Some(name) = name.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    c.execute(
        "INSERT INTO brands(name) VALUES(?) ON CONFLICT(name) DO NOTHING",
        [name],
    )?;
    Ok(Some(c.query_row(
        "SELECT id FROM brands WHERE name=?",
        [name],
        |r| r.get(0),
    )?))
}
fn insert_executable(c: &Connection, game: i64, path: &str) -> Result<()> {
    let p = normalize_path(path);
    if p.is_empty() {
        bail!("実行ファイルパスが空です")
    };
    let file = Path::new(&p)
        .file_name()
        .and_then(|x| x.to_str())
        .context("実行ファイル名を取得できません")?;
    c.execute(
        "INSERT INTO game_executables(game_id,path,file_name,created_at) VALUES(?,?,?,?)",
        params![game, p, file, now()],
    )?;
    Ok(())
}
fn validate_timestamp_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        bail!("タイムスタンプのタイトルを入力してください")
    }
    if name.chars().count() > 100 {
        bail!("タイムスタンプのタイトルは100文字以内にしてください")
    }
    Ok(name)
}
pub fn normalize_path(path: &str) -> String {
    let p = path.trim().trim_matches('"').replace('/', "\\");
    p.strip_prefix("\\\\?\\").unwrap_or(&p).to_lowercase()
}
fn validate_interval(
    c: &Connection,
    table: &str,
    session: i64,
    exclude: Option<i64>,
    start: &str,
    end: Option<&str>,
) -> Result<()> {
    DateTime::parse_from_rfc3339(start).context("開始日時が不正です")?;
    if let Some(e) = end {
        parse_range(start, e)?
    }
    let (ss, se): (String, Option<String>) = c.query_row(
        "SELECT launched_at,exited_at FROM play_sessions WHERE id=?",
        [session],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if start < ss.as_str() || se.as_deref().is_some_and(|x| end.is_none_or(|e| e > x)) {
        bail!("区間はSession範囲内にしてください")
    };
    let overlap:i64=c.query_row(&format!("SELECT COUNT(*) FROM {table} WHERE play_session_id=? AND (? IS NULL OR id<>?) AND COALESCE(ended_at,'9999') > ? AND COALESCE(?, '9999') > started_at"),params![session,exclude,exclude,start,end],|r|r.get(0))?;
    if overlap > 0 {
        bail!("区間が既存の区間と重複しています")
    };
    Ok(())
}

fn validate_session_range(
    c: &Connection,
    game: i64,
    exclude: Option<i64>,
    start: &str,
    end: Option<&str>,
) -> Result<()> {
    DateTime::parse_from_rfc3339(start).context("開始日時が不正です")?;
    if let Some(end) = end {
        parse_range(start, end)?;
    }
    let overlap: i64 = c.query_row(
        "SELECT COUNT(*) FROM play_sessions WHERE game_id=? AND (? IS NULL OR id<>?) AND COALESCE(exited_at,'9999') > ? AND COALESCE(?, '9999') > launched_at",
        params![game, exclude, exclude, start, end],
        |row| row.get(0),
    )?;
    if overlap > 0 {
        bail!("セッションが既存のセッションと重複しています")
    }
    Ok(())
}

fn load_ranges(c: &Connection, table: &str, session: i64) -> Result<Vec<(String, String)>> {
    let mut query = c.prepare(&format!("SELECT started_at,ended_at FROM {table} WHERE play_session_id=? AND ended_at IS NOT NULL ORDER BY started_at"))?;
    Ok(query
        .query_map([session], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?)
}

fn insert_complement(
    c: &Connection,
    table: &str,
    session: i64,
    start: &str,
    end: &str,
    excluded: &[(String, String)],
) -> Result<()> {
    let mut cursor = start.to_string();
    for (range_start, range_end) in excluded {
        let clamped_start = range_start.as_str().max(start).min(end);
        let clamped_end = range_end.as_str().max(start).min(end);
        if cursor.as_str() < clamped_start {
            insert_range(c, table, session, &cursor, clamped_start)?;
        }
        if clamped_end > cursor.as_str() {
            cursor = clamped_end.to_string();
        }
    }
    if cursor.as_str() < end {
        insert_range(c, table, session, &cursor, end)?;
    }
    Ok(())
}

fn insert_range(c: &Connection, table: &str, session: i64, start: &str, end: &str) -> Result<()> {
    let n = now();
    c.execute(&format!("INSERT INTO {table}(play_session_id,started_at,ended_at,created_at,updated_at) VALUES(?,?,?,?,?)"), params![session,start,end,n,n])?;
    Ok(())
}

fn rebuild_focus_mirror(c: &Connection, session: i64) -> Result<()> {
    let (start, end): (String, Option<String>) = c.query_row(
        "SELECT launched_at,exited_at FROM play_sessions WHERE id=?",
        [session],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let Some(end) = end else { return Ok(()) };
    let background = load_ranges(c, "background_intervals", session)?;
    c.execute(
        "DELETE FROM focus_intervals WHERE play_session_id=?",
        [session],
    )?;
    insert_complement(c, "focus_intervals", session, &start, &end, &background)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn set_background_exclusion(db: &Database, exclude: bool) {
        let settings = AppSettings {
            exclude_background_time: exclude,
            ..db.settings().unwrap()
        };
        db.set_setting("app", &serde_json::to_string(&settings).unwrap())
            .unwrap();
    }
    fn game(db: &Database) -> i64 {
        db.create_game(
            &CreateGameInput {
                title: "A".into(),
                brand: Some("B".into()),
                release_date: None,
                thumbnail_path: None,
                source: None,
                executable_paths: vec![r#"C:\G\a.exe"#.into(), r#"C:\G\launcher.exe"#.into()],
                launch_command: None,
            },
            None,
        )
        .unwrap()
    }
    #[test]
    fn aggregate_and_sort() {
        let d = Database::memory().unwrap();
        let g = game(&d);
        d.manual_session(g, "2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z")
            .unwrap();
        let x = d
            .list_games("", None, None, "total_playtime", true)
            .unwrap();
        assert_eq!(x[0].total_playtime_seconds, 3600);
        assert_eq!(x[0].session_count, 1);
    }
    #[test]
    fn calculation_setting_recalculates_history_sorting_timestamps_and_statistics() {
        let d = Database::memory().unwrap();
        // Missing and older settings must keep the existing calculation.
        assert!(d.settings().unwrap().exclude_background_time);
        d.set_setting("app", r#"{"theme":"blue"}"#).unwrap();
        assert!(d.settings().unwrap().exclude_background_time);
        let g = game(&d);
        let session = d
            .manual_session(g, "2026-01-15T00:00:00Z", "2026-01-15T01:00:00Z")
            .unwrap();
        d.create_interval(session, "2026-01-15T00:15:00Z", "2026-01-15T00:45:00Z")
            .unwrap();
        d.create_timestamp(g, "途中", "2026-01-15T00:30:00Z")
            .unwrap();
        d.create_timestamp(g, "終盤", "2026-01-15T00:50:00Z")
            .unwrap();
        let other = d
            .create_game(
                &CreateGameInput {
                    title: "Other".into(),
                    brand: None,
                    release_date: None,
                    thumbnail_path: None,
                    source: None,
                    executable_paths: vec![],
                    launch_command: None,
                },
                None,
            )
            .unwrap();
        d.manual_session(other, "2026-01-15T02:00:00Z", "2026-01-15T02:45:00Z")
            .unwrap();
        let original_intervals = serde_json::to_string(&d.intervals(session).unwrap()).unwrap();
        let last_played = d.get_game(g).unwrap().summary.last_played;

        for exclude in [true, false, true] {
            set_background_exclusion(&d, exclude);
            let expected = if exclude { 1_800 } else { 3_600 };
            let detail = d.get_game(g).unwrap().summary;
            assert_eq!(detail.total_playtime_seconds, expected);
            assert_eq!(detail.total_running_seconds, 3_600);
            assert_eq!(detail.last_played, last_played);
            assert_eq!(detail.session_count, 1);
            let sessions = d.list_sessions(g).unwrap();
            assert_eq!(sessions[0].playtime_seconds, expected);
            assert_eq!(sessions[0].background_seconds, 1_800);
            assert_eq!(sessions[0].running_seconds, Some(3_600));
            let games = d
                .list_games("", None, None, "total_playtime", true)
                .unwrap();
            assert_eq!(games[0].id, if exclude { other } else { g });
            let filtered = d
                .list_games("A", Some("B"), Some("unplayed"), "total_playtime", false)
                .unwrap();
            assert_eq!(filtered.len(), 1);
            assert_eq!(filtered[0].total_playtime_seconds, expected);
            let timestamps = d.timestamps(g).unwrap();
            assert_eq!(
                timestamps[0].playtime_seconds,
                if exclude { 900 } else { 1_800 }
            );
            assert_eq!(
                timestamps[0].since_previous_seconds,
                timestamps[0].playtime_seconds
            );
            assert_eq!(
                timestamps[1].playtime_seconds,
                if exclude { 1_200 } else { 3_000 }
            );
            assert_eq!(
                timestamps[1].since_previous_seconds,
                if exclude { 300 } else { 1_200 }
            );
            for kind in ["month", "year", "all"] {
                let report = d
                    .statistics(&StatisticsPeriodInput {
                        kind: kind.into(),
                        year: (kind != "all").then_some(2026),
                        month: (kind == "month").then_some(1),
                        days: None,
                    })
                    .unwrap();
                assert_eq!(report.summary.total_playtime_seconds, expected + 2_700);
                assert_eq!(report.summary.session_count, 2);
                assert_eq!(report.summary.game_count, 2);
                assert_eq!(
                    report.summary.longest_session.unwrap().game_id,
                    if exclude { other } else { g }
                );
                assert_eq!(
                    report
                        .days
                        .iter()
                        .map(|day| day.playtime_seconds)
                        .sum::<i64>(),
                    expected + 2_700
                );
                assert_eq!(
                    report
                        .games
                        .iter()
                        .map(|game| game.playtime_seconds)
                        .sum::<i64>(),
                    expected + 2_700
                );
            }
            let game_report = d.game_statistics(g).unwrap();
            assert_eq!(game_report.summary.total_playtime_seconds, expected);
            assert_eq!(game_report.games.len(), 1);
            assert_eq!(
                serde_json::to_string(&d.intervals(session).unwrap()).unwrap(),
                original_intervals
            );
            assert_eq!(d.settings().unwrap().theme, "blue");
        }
    }

    #[test]
    fn active_background_recording_and_timestamp_clipping_survive_setting_changes() {
        let d = Database::memory().unwrap();
        let g = game(&d);
        let snapshot = Utc::now();
        let start = (snapshot - chrono::Duration::minutes(10)).to_rfc3339();
        let background_start = (snapshot - chrono::Duration::minutes(5)).to_rfc3339();
        let marked_at = (snapshot - chrono::Duration::minutes(1)).to_rfc3339();
        set_background_exclusion(&d, false);
        let session = d.start_session(g, &start).unwrap();
        let interval = d.start_interval(session, &background_start).unwrap();
        d.create_timestamp(g, "記録中", &marked_at).unwrap();

        for exclude in [false, true, false] {
            set_background_exclusion(&d, exclude);
            let recorded = d.list_sessions(g).unwrap().remove(0);
            assert!(recorded.exited_at.is_none());
            assert_eq!(recorded.running_seconds, None);
            assert!(recorded.background_seconds >= 300);
            assert_eq!(
                recorded.playtime_seconds,
                300 + if exclude {
                    0
                } else {
                    recorded.background_seconds
                }
            );
            let point = d.timestamps(g).unwrap().remove(0);
            assert_eq!(point.playtime_seconds, if exclude { 300 } else { 540 });
            let intervals = d.intervals(session).unwrap();
            assert_eq!(intervals.len(), 1);
            assert_eq!(intervals[0].id, interval);
            assert!(intervals[0].ended_at.is_none());
        }
        d.end_session(session, &snapshot.to_rfc3339()).unwrap();
        set_background_exclusion(&d, true);
        let recorded = d.list_sessions(g).unwrap().remove(0);
        assert_eq!(recorded.playtime_seconds, 300);
        assert_eq!(recorded.background_seconds, 300);
    }

    #[test]
    fn thumbnail_can_be_replaced_and_cleared() {
        let d = Database::memory().unwrap();
        let g = game(&d);

        d.update_game_thumbnail(g, Some(r#"C:\thumbnails\custom.png"#))
            .unwrap();
        assert_eq!(
            d.get_game(g).unwrap().summary.thumbnail_path.as_deref(),
            Some(r#"C:\thumbnails\custom.png"#)
        );

        d.update_game_thumbnail(g, None).unwrap();
        assert!(d.get_game(g).unwrap().summary.thumbnail_path.is_none());
        assert!(d.update_game_thumbnail(i64::MAX, None).is_err());
    }
    #[test]
    fn erogamescape_metadata_replaces_an_existing_thumbnail() {
        let d = Database::memory().unwrap();
        let g = game(&d);
        d.update_game_thumbnail(g, Some(r#"C:\thumbnails\manual.png"#))
            .unwrap();
        let metadata = crate::metadata::GameMetadata {
            source: MetadataIdentity::ErogameScape(42),
            title: "Updated".into(),
            brand: Some("Updated Brand".into()),
            release_date: Some("2026-08-29".into()),
            thumbnail_url: Some("https://example.test/cover.jpg".into()),
            warnings: Vec::new(),
        };

        d.apply_metadata(g, &metadata, Some(r#"C:\thumbnails\erogamescape.jpg"#))
            .unwrap();

        assert_eq!(
            d.get_game(g).unwrap().summary.thumbnail_path.as_deref(),
            Some(r#"C:\thumbnails\erogamescape.jpg"#)
        );
    }
    #[test]
    fn metadata_sources_round_trip_switch_and_preserve_history_and_thumbnail() {
        let d = Database::memory().unwrap();
        let input: CreateGameInput = serde_json::from_value(serde_json::json!({
            "title": "VNDB game", "brand": "First brand", "source": {"provider": "vndb", "id": "v00548"}
        })).unwrap();
        let id = d
            .create_game(&input, Some(r#"C:\thumbnails\manual.png"#))
            .unwrap();
        let game = d.get_game(id).unwrap();
        assert_eq!(game.vndb_id.as_deref(), Some("v548"));
        assert_eq!(game.metadata_source.as_deref(), Some("vndb"));
        assert_eq!(game.source_url.as_deref(), Some("https://vndb.org/v548"));
        assert_eq!(
            d.metadata_identity(id).unwrap(),
            MetadataIdentity::Vndb("v548".into())
        );
        let session = d
            .manual_session(id, "2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z")
            .unwrap();
        d.create_interval(session, "2026-01-01T00:10:00Z", "2026-01-01T00:25:00Z")
            .unwrap();
        let sessions = serde_json::to_string(&d.list_sessions(id).unwrap()).unwrap();
        let mut edit = UpdateGameInput {
            title: "Edited".into(),
            brand: None,
            release_date: None,
            source_url: Some(MetadataIdentity::ErogameScape(42).url()),
        };
        d.update_game(id, &edit).unwrap();
        assert_eq!(
            d.metadata_identity(id).unwrap(),
            MetadataIdentity::ErogameScape(42)
        );
        assert_eq!(d.get_game(id).unwrap().vndb_id.as_deref(), Some("v548"));
        let m = crate::metadata::GameMetadata {
            source: MetadataIdentity::Vndb("v548".into()),
            title: "Refreshed".into(),
            brand: Some("First brand".into()),
            release_date: None,
            thumbnail_url: None,
            warnings: Vec::new(),
        };
        d.apply_metadata(id, &m, None).unwrap();
        let game = d.get_game(id).unwrap();
        assert_eq!(game.summary.title, "Refreshed");
        assert_eq!(game.erogamescape_id, Some(42));
        assert_eq!(game.metadata_source.as_deref(), Some("vndb"));
        assert_eq!(
            game.summary.thumbnail_path.as_deref(),
            Some(r#"C:\thumbnails\manual.png"#)
        );
        assert_eq!(
            serde_json::to_string(&d.list_sessions(id).unwrap()).unwrap(),
            sessions
        );
        for url in [Some("https://example.test/game.php?game=999".into()), None] {
            edit.source_url = url;
            d.update_game(id, &edit).unwrap();
            assert!(d.metadata_identity(id).is_err());
            assert_eq!(d.get_game(id).unwrap().erogamescape_id, Some(42));
        }
    }

    #[test]
    fn duplicate_vndb_ids_roll_back_create_edit_and_metadata_updates() {
        let d = Database::memory().unwrap();
        let input = CreateGameInput {
            title: "VNDB game".into(),
            brand: None,
            release_date: None,
            thumbnail_path: None,
            source: Some(MetadataIdentity::Vndb("v548".into())),
            executable_paths: vec![],
            launch_command: None,
        };
        d.create_game(&input, None).unwrap();
        assert!(d.create_game(&input, None).is_err());
        assert_eq!(
            d.list_games("", None, None, "title", false).unwrap().len(),
            1
        );
        let other = game(&d);
        let edit = UpdateGameInput {
            title: "Should roll back".into(),
            brand: Some("Should roll back".into()),
            release_date: None,
            source_url: Some("https://vndb.org/v548".into()),
        };
        assert!(d.update_game(other, &edit).is_err());
        let metadata = crate::metadata::GameMetadata {
            source: MetadataIdentity::Vndb("v548".into()),
            title: "Should roll back".into(),
            brand: edit.brand,
            release_date: None,
            thumbnail_url: None,
            warnings: vec![],
        };
        assert!(d.apply_metadata(other, &metadata, None).is_err());
        assert_eq!(d.get_game(other).unwrap().summary.title, "A");
        assert!(d.get_game(other).unwrap().metadata_source.is_none());
        let brands: i64 =
            d.0.lock()
                .query_row("SELECT COUNT(*) FROM brands", [], |r| r.get(0))
                .unwrap();
        assert_eq!(brands, 1);
    }

    #[test]
    fn migration_6_preserves_legacy_sources_and_playtime_in_both_modes() {
        let c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        for (version, sql) in [
            MIGRATION_1,
            MIGRATION_2,
            MIGRATION_3,
            MIGRATION_4,
            MIGRATION_5,
        ]
        .into_iter()
        .enumerate()
        {
            c.execute_batch(sql).unwrap();
            c.execute(
                "INSERT INTO schema_migrations VALUES(?,?)",
                params![version as i64 + 1, now()],
            )
            .unwrap();
        }
        for (id, eid, url) in [
            (1, Some(42), None),
            (
                2,
                None,
                Some("https://erogamescape.dyndns.org/~ap2/ero/toukei_kaiseki/game.php?game=99"),
            ),
            (3, None, None),
            (4, None, Some("https://vndb.org/v548")),
        ] {
            c.execute("INSERT INTO games(id,title,erogamescape_id,source_url,created_at,updated_at) VALUES(?,?,?,?,?,?)", params![id, format!("Game {id}"), eid, url, now(), now()]).unwrap();
        }
        let d = Database(Arc::new(Mutex::new(c)), Default::default());
        let session = d
            .manual_session(1, "2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z")
            .unwrap();
        d.create_interval(session, "2026-01-01T00:10:00Z", "2026-01-01T00:25:00Z")
            .unwrap();
        d.create_timestamp(1, "Point", "2026-01-01T00:30:00Z")
            .unwrap();
        d.migrate().unwrap();
        d.migrate().unwrap();
        assert_eq!(
            d.metadata_identity(1).unwrap(),
            MetadataIdentity::ErogameScape(42)
        );
        assert_eq!(
            d.metadata_identity(2).unwrap(),
            MetadataIdentity::ErogameScape(99)
        );
        assert!(d.metadata_identity(3).is_err());
        assert_eq!(
            d.metadata_identity(4).unwrap(),
            MetadataIdentity::Vndb("v548".into())
        );
        assert!(d.get_game(3).unwrap().source_url.is_none());
        for exclude in [true, false, true] {
            set_background_exclusion(&d, exclude);
            let game = d.get_game(1).unwrap();
            assert_eq!(
                game.summary.total_playtime_seconds,
                if exclude { 2700 } else { 3600 }
            );
            assert_eq!(d.list_sessions(1).unwrap()[0].background_seconds, 900);
            assert_eq!(
                d.timestamps(1).unwrap()[0].playtime_seconds,
                if exclude { 900 } else { 1800 }
            );
            let focus: i64 = d.0.lock().query_row("SELECT SUM(strftime('%s',ended_at)-strftime('%s',started_at)) FROM focus_intervals WHERE play_session_id=?", [session], |r| r.get(0)).unwrap();
            assert_eq!(focus, 2700);
        }
    }
    #[test]
    fn rejects_overlap_and_outside() {
        let d = Database::memory().unwrap();
        let g = game(&d);
        let s = d
            .manual_session(g, "2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z")
            .unwrap();
        d.create_interval(s, "2026-01-01T00:30:00Z", "2026-01-01T00:40:00Z")
            .unwrap();
        assert!(
            d.create_interval(s, "2026-01-01T00:35:00Z", "2026-01-01T00:45:00Z")
                .is_err()
        );
        assert!(
            d.update_session(s, "2026-01-01T00:35:00Z", Some("2026-01-01T01:00:00Z"))
                .is_err()
        );
    }
    #[test]
    fn manual_session_and_session_edit_reject_overlap() {
        let d = Database::memory().unwrap();
        let game_id = game(&d);
        let first = d
            .manual_session(game_id, "2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z")
            .unwrap();
        assert_eq!(
            d.manual_session(game_id, "2026-01-01T00:30:00Z", "2026-01-01T01:30:00Z")
                .unwrap_err()
                .to_string(),
            "セッションが既存のセッションと重複しています"
        );

        let second = d
            .manual_session(game_id, "2026-01-01T01:00:00Z", "2026-01-01T02:00:00Z")
            .unwrap();
        assert_eq!(
            d.update_session(second, "2026-01-01T00:30:00Z", Some("2026-01-01T02:00:00Z"))
                .unwrap_err()
                .to_string(),
            "セッションが既存のセッションと重複しています"
        );
        d.update_session(first, "2026-01-01T00:00:00Z", Some("2026-01-01T01:00:00Z"))
            .unwrap();

        let other_game = d
            .create_game(
                &CreateGameInput {
                    title: "Other".into(),
                    brand: None,
                    release_date: None,
                    thumbnail_path: None,
                    source: None,
                    executable_paths: vec![],
                    launch_command: None,
                },
                None,
            )
            .unwrap();
        d.manual_session(other_game, "2026-01-01T00:30:00Z", "2026-01-01T01:30:00Z")
            .unwrap();
    }
    #[test]
    fn session_update_preserves_running_state() {
        let d = Database::memory().unwrap();
        let g = game(&d);
        let closed = d
            .manual_session(g, "2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z")
            .unwrap();
        assert!(
            d.update_session(closed, "2026-01-01T00:00:00Z", None)
                .is_err()
        );

        let running = d.start_session(g, "2026-01-01T02:00:00Z").unwrap();
        assert!(
            d.update_session(
                running,
                "2026-01-01T02:05:00Z",
                Some("2026-01-01T03:00:00Z")
            )
            .is_err()
        );
        d.update_session(running, "2026-01-01T02:05:00Z", None)
            .unwrap();

        let sessions = d.list_sessions(g).unwrap();
        assert_eq!(sessions[0].id, running);
        assert_eq!(sessions[0].launched_at, "2026-01-01T02:05:00Z");
        assert_eq!(sessions[0].exited_at, None);
        assert_eq!(
            sessions[1].exited_at.as_deref(),
            Some("2026-01-01T01:00:00Z")
        );
    }
    #[test]
    fn interval_update_preserves_recording_state() {
        let d = Database::memory().unwrap();
        let g = game(&d);
        let session = d.start_session(g, "2026-01-01T00:00:00Z").unwrap();
        let interval = d.start_interval(session, "2026-01-01T00:10:00Z").unwrap();

        assert!(
            d.update_interval(
                interval,
                "2026-01-01T00:15:00Z",
                Some("2026-01-01T00:20:00Z")
            )
            .is_err()
        );
        d.update_interval(interval, "2026-01-01T00:15:00Z", None)
            .unwrap();
        d.end_interval(interval, "2026-01-01T00:30:00Z").unwrap();

        let recorded = d.intervals(session).unwrap().remove(0);
        assert_eq!(recorded.started_at, "2026-01-01T00:15:00Z");
        assert_eq!(recorded.ended_at.as_deref(), Some("2026-01-01T00:30:00Z"));
        assert!(
            d.update_interval(interval, &recorded.started_at, None)
                .is_err()
        );
    }
    #[test]
    fn confirming_session_review_preserves_playtime_and_clears_review_counts() {
        let d = Database::memory().unwrap();
        let g = game(&d);
        let session = d.start_session(g, "2026-01-01T00:00:00Z").unwrap();
        d.start_interval(session, "2026-01-01T00:10:00Z").unwrap();
        d.recover_orphans("2026-01-01T00:30:00Z").unwrap();
        let period = StatisticsPeriodInput {
            kind: "year".into(),
            year: Some(2026),
            month: None,
            days: None,
        };

        let before = d.list_sessions(g).unwrap().remove(0);
        assert!(before.needs_review);
        assert_eq!(
            d.statistics(&period)
                .unwrap()
                .summary
                .needs_review_session_count,
            1
        );

        d.confirm_session_review(session).unwrap();
        d.confirm_session_review(session).unwrap();

        let after = d.list_sessions(g).unwrap().remove(0);
        assert!(!after.needs_review);
        assert_eq!(after.launched_at, before.launched_at);
        assert_eq!(after.exited_at, before.exited_at);
        assert_eq!(after.playtime_seconds, before.playtime_seconds);
        assert_eq!(after.background_seconds, before.background_seconds);
        assert_eq!(after.running_seconds, before.running_seconds);
        assert_eq!(
            d.statistics(&period)
                .unwrap()
                .summary
                .needs_review_session_count,
            0
        );
        assert!(d.confirm_session_review(i64::MAX).is_err());
    }
    #[test]
    fn multiple_executables_one_game() {
        let d = Database::memory().unwrap();
        let g = game(&d);
        assert_eq!(
            d.registered_executables()
                .unwrap()
                .iter()
                .filter(|x| x.0 == g)
                .count(),
            2
        );
    }
    #[test]
    fn launch_command_defaults_to_none_and_trims_blank_values() {
        let d = Database::memory().unwrap();
        let g = game(&d);
        assert_eq!(d.launch_command(g).unwrap(), None);
        assert_eq!(d.get_game(g).unwrap().launch_command, None);

        d.set_launch_command(g, Some("  game.exe -w  ")).unwrap();
        assert_eq!(d.launch_command(g).unwrap().as_deref(), Some("game.exe -w"));
        assert_eq!(
            d.get_game(g).unwrap().launch_command.as_deref(),
            Some("game.exe -w")
        );

        d.set_launch_command(g, Some("   ")).unwrap();
        assert_eq!(
            d.launch_command(g).unwrap(),
            None,
            "a blank command restores the registered executable"
        );
    }

    #[test]
    fn create_game_persists_a_launch_command() {
        let d = Database::memory().unwrap();
        let g = d
            .create_game(
                &CreateGameInput {
                    title: "Launcher game".into(),
                    brand: None,
                    release_date: None,
                    thumbnail_path: None,
                    source: None,
                    executable_paths: vec![r"D:\vn\game\game.exe".into()],
                    launch_command: Some(" wrapper.exe --fullscreen".into()),
                },
                None,
            )
            .unwrap();
        assert_eq!(
            d.get_game(g).unwrap().launch_command.as_deref(),
            Some("wrapper.exe --fullscreen"),
            "create trims and stores the custom launch command"
        );
    }

    #[test]
    fn timestamp_playtime_is_derived_from_sessions_minus_background() {
        let d = Database::memory().unwrap();
        let g = game(&d);
        d.manual_session(g, "2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z")
            .unwrap();
        d.manual_session(g, "2026-01-01T02:00:00Z", "2026-01-01T03:00:00Z")
            .unwrap();
        d.create_timestamp(g, "共通ルート終了", "2026-01-01T00:30:00Z")
            .unwrap();
        let second = d
            .create_timestamp(g, "個別ルート終了", "2026-01-01T02:15:00Z")
            .unwrap();
        let points = d.timestamps(g).unwrap();
        assert_eq!(points[0].playtime_seconds, 1800);
        assert_eq!(points[0].since_previous_seconds, 1800);
        assert_eq!(points[1].playtime_seconds, 4500);
        assert_eq!(points[1].since_previous_seconds, 2700);
        d.delete_timestamp(second).unwrap();
        assert_eq!(d.timestamps(g).unwrap().len(), 1);
    }

    #[test]
    fn timestamp_update_reorders_and_recalculates_playtime() {
        let d = Database::memory().unwrap();
        let g = game(&d);
        d.manual_session(g, "2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z")
            .unwrap();
        let first = d
            .create_timestamp(g, "共通ルート終了", "2026-01-01T00:30:00Z")
            .unwrap();
        let second = d
            .create_timestamp(g, "個別ルート終了", "2026-01-01T00:45:00Z")
            .unwrap();

        d.update_timestamp(second, "  真ルート終了  ", "2026-01-01T00:15:00Z")
            .unwrap();
        let updated = d.timestamps(g).unwrap();
        assert_eq!(updated[0].id, second);
        assert_eq!(updated[0].name, "真ルート終了");
        assert_eq!(updated[0].marked_at, "2026-01-01T00:15:00Z");
        assert_eq!(updated[0].playtime_seconds, 900);
        assert_eq!(updated[0].since_previous_seconds, 900);
        assert_eq!(updated[1].id, first);
        assert_eq!(updated[1].playtime_seconds, 1800);
        assert_eq!(updated[1].since_previous_seconds, 900);

        assert!(
            d.update_timestamp(first, "   ", "2026-01-01T00:30:00Z")
                .is_err()
        );
        assert!(d.update_timestamp(first, "有効な名称", "invalid").is_err());
        assert!(
            d.update_timestamp(i64::MAX, "存在しない記録", "2026-01-01T00:30:00Z")
                .is_err()
        );
    }

    #[test]
    fn screenshot_path_is_resolved_from_the_database_id() {
        let d = Database::memory().unwrap();
        let g = game(&d);
        let id = d
            .add_screenshot(
                g,
                None,
                r#"C:\screenshots\sample.png"#,
                "2026-01-01T00:00:00Z",
                1920,
                1080,
            )
            .unwrap();

        assert_eq!(
            d.screenshot_path(id).unwrap().as_deref(),
            Some(r#"C:\screenshots\sample.png"#)
        );
        assert_eq!(d.screenshot_path(i64::MAX).unwrap(), None);
    }

    #[test]
    fn legacy_focus_is_migrated_to_its_background_complement() {
        let d = Database::memory().unwrap();
        set_background_exclusion(&d, false);
        let g = game(&d);
        let n = now();
        let c = d.0.lock();
        c.execute("INSERT INTO play_sessions(game_id,launched_at,exited_at,created_at,updated_at) VALUES(?,?,?,?,?)", params![g,"2026-01-01T00:00:00Z","2026-01-01T01:00:00Z",n,n]).unwrap();
        let session = c.last_insert_rowid();
        c.execute("INSERT INTO focus_intervals(play_session_id,started_at,ended_at,created_at,updated_at) VALUES(?,?,?,?,?)", params![session,"2026-01-01T00:10:00Z","2026-01-01T00:40:00Z",n,n]).unwrap();
        drop(c);

        assert_eq!(d.migrate_focus_intervals().unwrap(), 1);
        let intervals = d.intervals(session).unwrap();
        assert_eq!(intervals.len(), 2);
        assert_eq!(intervals[0].started_at, "2026-01-01T00:00:00Z");
        assert_eq!(
            intervals[0].ended_at.as_deref(),
            Some("2026-01-01T00:10:00Z")
        );
        assert_eq!(intervals[1].started_at, "2026-01-01T00:40:00Z");
        assert_eq!(d.list_sessions(g).unwrap()[0].playtime_seconds, 3600);
        set_background_exclusion(&d, true);
        assert_eq!(d.list_sessions(g).unwrap()[0].playtime_seconds, 1800);
        assert_eq!(d.migrate_focus_intervals().unwrap(), 0);
    }
    #[test]
    fn migration_validation_matches_sqlite_second_rounding() {
        let d = Database::memory().unwrap();
        let g = game(&d);
        let n = now();
        let c = d.0.lock();
        c.execute("INSERT INTO play_sessions(game_id,launched_at,exited_at,created_at,updated_at) VALUES(?,?,?,?,?)", params![g,"2026-01-01T00:00:00.900Z","2026-01-01T00:00:03.100Z",n,n]).unwrap();
        let session = c.last_insert_rowid();
        c.execute("INSERT INTO focus_intervals(play_session_id,started_at,ended_at,created_at,updated_at) VALUES(?,?,?,?,?)", params![session,"2026-01-01T00:00:00.900Z","2026-01-01T00:00:01.100Z",n,n]).unwrap();
        c.execute("INSERT INTO focus_intervals(play_session_id,started_at,ended_at,created_at,updated_at) VALUES(?,?,?,?,?)", params![session,"2026-01-01T00:00:02.900Z","2026-01-01T00:00:03.100Z",n,n]).unwrap();
        drop(c);

        assert_eq!(d.migrate_focus_intervals().unwrap(), 1);
        assert_eq!(d.list_sessions(g).unwrap()[0].playtime_seconds, 2);
    }
    #[test]
    fn background_tracking_derives_playtime_and_keeps_focus_mirror() {
        let d = Database::memory().unwrap();
        set_background_exclusion(&d, false);
        let g = game(&d);
        let session = d.start_session(g, "2026-01-01T00:00:00Z").unwrap();
        let background = d.start_interval(session, "2026-01-01T00:00:00Z").unwrap();
        d.end_interval(background, "2026-01-01T00:10:00Z").unwrap();
        let focus = d
            .start_legacy_focus_interval(session, "2026-01-01T00:10:00Z")
            .unwrap();
        d.end_legacy_focus_interval(focus, "2026-01-01T00:20:00Z")
            .unwrap();
        let background = d.start_interval(session, "2026-01-01T00:20:00Z").unwrap();
        d.end_interval(background, "2026-01-01T00:30:00Z").unwrap();
        d.end_session(session, "2026-01-01T00:30:00Z").unwrap();

        assert_eq!(d.list_sessions(g).unwrap()[0].playtime_seconds, 1800);
        set_background_exclusion(&d, true);
        let recorded = d.list_sessions(g).unwrap().remove(0);
        assert_eq!(recorded.running_seconds, Some(1800));
        assert_eq!(recorded.background_seconds, 1200);
        assert_eq!(recorded.playtime_seconds, 600);
        let legacy_focus_seconds: i64 = d
            .0
            .lock()
            .query_row(
                "SELECT SUM(strftime('%s',ended_at)-strftime('%s',started_at)) FROM focus_intervals WHERE play_session_id=?",
                [session],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(legacy_focus_seconds, recorded.playtime_seconds);
    }
    #[test]
    fn brand_list_is_independent_from_game_filtering() {
        let d = Database::memory().unwrap();
        game(&d);
        d.create_game(
            &CreateGameInput {
                title: "Other".into(),
                brand: Some("Another Brand".into()),
                release_date: None,
                thumbnail_path: None,
                source: None,
                executable_paths: vec![],
                launch_command: None,
            },
            None,
        )
        .unwrap();
        assert_eq!(d.list_brands().unwrap(), vec!["Another Brand", "B"]);
        assert_eq!(
            d.list_games("", Some("B"), None, "title", false)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(d.list_brands().unwrap().len(), 2);
    }
    #[test]
    fn brand_list_excludes_brands_without_games() {
        let d = Database::memory().unwrap();
        game(&d);
        let temporary_games = ["Temporary One", "Temporary Two"].map(|title| {
            d.create_game(
                &CreateGameInput {
                    title: title.into(),
                    brand: Some("Temporary Brand".into()),
                    release_date: None,
                    thumbnail_path: None,
                    source: None,
                    executable_paths: vec![],
                    launch_command: None,
                },
                None,
            )
            .unwrap()
        });

        assert_eq!(d.list_brands().unwrap(), vec!["B", "Temporary Brand"]);

        d.delete_game(temporary_games[0]).unwrap();
        assert_eq!(d.list_brands().unwrap(), vec!["B", "Temporary Brand"]);

        d.delete_game(temporary_games[1]).unwrap();
        assert_eq!(d.list_brands().unwrap(), vec!["B"]);
    }
    #[test]
    fn statistics_split_sessions_and_background_by_day() {
        for exclude in [true, false] {
            let period = StatisticsPeriod {
                kind: "month".into(),
                year: Some(2026),
                month: Some(1),
                days: None,
                start_date: "2026-01-01".into(),
                end_date: "2026-01-02".into(),
                generated_at: "2026-01-03T00:00:00Z".into(),
            };
            let days = vec![
                StatisticsDayBoundary {
                    date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                    start: 0,
                    end: 86_400,
                },
                StatisticsDayBoundary {
                    date: NaiveDate::from_ymd_opt(2026, 1, 2).unwrap(),
                    start: 86_400,
                    end: 169_200,
                },
            ];
            let sessions = vec![
                RawStatisticsSession {
                    id: 10,
                    game_id: 1,
                    title: "Long game".into(),
                    brand: Some("Brand".into()),
                    thumbnail_path: None,
                    launched_at: "2026-01-01T23:00:00Z".into(),
                    exited_at: Some("2026-01-02T02:00:00Z".into()),
                    needs_review: true,
                    start: 82_800,
                    end: 93_600,
                    background: vec![(88_200, 90_000)],
                },
                RawStatisticsSession {
                    id: 11,
                    game_id: 2,
                    title: "Short game".into(),
                    brand: None,
                    thumbnail_path: None,
                    launched_at: "2026-01-02T03:00:00Z".into(),
                    exited_at: Some("2026-01-02T04:00:00Z".into()),
                    needs_review: false,
                    start: 97_200,
                    end: 100_800,
                    background: vec![],
                },
            ];

            let report = aggregate_statistics(
                period,
                days,
                sessions,
                vec![2026],
                exclude,
                &Coverage::default(),
                true,
            );

            assert_eq!(report.days[0].playtime_seconds, 3_600);
            assert_eq!(
                report.days[1].playtime_seconds,
                if exclude { 9_000 } else { 10_800 }
            );
            assert_eq!(report.days[0].games[0].title, "Long game");
            assert_eq!(report.days[0].games[0].thumbnail_path, None);
            assert_eq!(report.days[1].games[0].title, "Long game");
            assert_eq!(report.days[1].games[1].title, "Short game");
            assert_eq!(
                report.summary.total_playtime_seconds,
                if exclude { 12_600 } else { 14_400 }
            );
            assert_eq!(
                report
                    .games
                    .iter()
                    .map(|game| game.playtime_seconds)
                    .sum::<i64>(),
                report.summary.total_playtime_seconds
            );
            assert_eq!(report.games[0].game_id, 1);
            assert_eq!(
                report.games[0].playtime_seconds,
                if exclude { 9_000 } else { 10_800 }
            );
            assert_eq!(report.games[0].active_day_count, 2);
            assert_eq!(report.summary.active_day_count, 2);
            assert_eq!(report.summary.needs_review_session_count, 1);
            assert_eq!(
                report.summary.longest_session.as_ref().unwrap().session_id,
                10
            );
        }
    }

    #[test]
    fn statistics_command_uses_sessions_minus_background() {
        let database = Database::memory().unwrap();
        let game_id = game(&database);
        let session_id = database
            .manual_session(game_id, "2026-01-15T12:00:00Z", "2026-01-15T13:00:00Z")
            .unwrap();
        database
            .create_interval(session_id, "2026-01-15T12:15:00Z", "2026-01-15T12:30:00Z")
            .unwrap();

        let report = database
            .statistics(&StatisticsPeriodInput {
                kind: "year".into(),
                year: Some(2026),
                month: None,
                days: None,
            })
            .unwrap();

        assert_eq!(report.summary.total_playtime_seconds, 2_700);
        assert_eq!(report.summary.session_count, 1);
        assert_eq!(report.summary.game_count, 1);
        assert_eq!(report.games[0].playtime_seconds, 2_700);
        assert_eq!(
            report
                .days
                .iter()
                .map(|day| day.playtime_seconds)
                .sum::<i64>(),
            2_700
        );
    }
    #[test]
    fn game_statistics_filters_the_game_and_trims_to_played_days() {
        let database = Database::memory().unwrap();
        let game_id = game(&database);
        let background_only_before = database
            .manual_session(game_id, "2026-01-08T12:00:00Z", "2026-01-08T13:00:00Z")
            .unwrap();
        database
            .create_interval(
                background_only_before,
                "2026-01-08T12:00:00Z",
                "2026-01-08T13:00:00Z",
            )
            .unwrap();
        let first_session = database
            .manual_session(game_id, "2026-01-10T12:00:00Z", "2026-01-10T13:00:00Z")
            .unwrap();
        database
            .create_interval(
                first_session,
                "2026-01-10T12:15:00Z",
                "2026-01-10T12:30:00Z",
            )
            .unwrap();
        let background_only_session = database
            .manual_session(game_id, "2026-01-12T12:00:00Z", "2026-01-12T13:00:00Z")
            .unwrap();
        database
            .create_interval(
                background_only_session,
                "2026-01-12T12:00:00Z",
                "2026-01-12T13:00:00Z",
            )
            .unwrap();
        let other_game = database
            .create_game(
                &CreateGameInput {
                    title: "Other".into(),
                    brand: None,
                    release_date: None,
                    thumbnail_path: None,
                    source: None,
                    executable_paths: vec![r#"C:\Other\other.exe"#.into()],
                    launch_command: None,
                },
                None,
            )
            .unwrap();
        database
            .manual_session(other_game, "2026-01-11T12:00:00Z", "2026-01-11T14:00:00Z")
            .unwrap();

        let report = database.game_statistics(game_id).unwrap();

        assert_eq!(report.period.start_date, "2026-01-10");
        assert_eq!(report.period.end_date, "2026-01-10");
        assert_eq!(report.days.len(), 1);
        assert_eq!(report.days[0].playtime_seconds, 2_700);
        assert_eq!(report.summary.total_playtime_seconds, 2_700);
        assert_eq!(report.summary.session_count, 1);
        assert_eq!(report.games.len(), 1);
        assert_eq!(report.games[0].game_id, game_id);
        set_background_exclusion(&database, false);
        let included = database.game_statistics(game_id).unwrap();
        assert_eq!(included.period.start_date, "2026-01-08");
        assert_eq!(included.period.end_date, "2026-01-12");
        assert_eq!(included.days.len(), 5);
        assert_eq!(included.summary.total_playtime_seconds, 10_800);
        assert_eq!(included.summary.session_count, 3);
        assert_eq!(included.summary.active_day_count, 3);
        assert_eq!(included.games.len(), 1);
        assert_eq!(included.games[0].game_id, game_id);
    }
    #[test]
    fn statistics_reject_future_periods() {
        let database = Database::memory().unwrap();
        let future_year = Local::now().year() + 1;
        assert!(
            database
                .statistics(&StatisticsPeriodInput {
                    kind: "year".into(),
                    year: Some(future_year),
                    month: None,
                    days: None,
                })
                .is_err()
        );
    }
    #[test]
    fn statistics_recent_range_covers_the_requested_days_ending_today() {
        let database = Database::memory().unwrap();
        let report = database
            .statistics(&StatisticsPeriodInput {
                kind: "recent".into(),
                year: None,
                month: None,
                days: Some(7),
            })
            .unwrap();

        let today = Utc::now().with_timezone(&Local).date_naive();
        assert_eq!(report.period.kind, "recent");
        assert_eq!(report.period.days, Some(7));
        assert_eq!(report.period.year, None);
        assert_eq!(report.period.month, None);
        assert_eq!(report.period.end_date, today.to_string());
        assert_eq!(
            report.period.start_date,
            (today - Duration::days(6)).to_string()
        );
        assert_eq!(report.days.len(), 7);
    }
    #[test]
    fn statistics_recent_range_rejects_unsupported_day_counts() {
        let database = Database::memory().unwrap();
        for days in [None, Some(0), Some(1), Some(8), Some(365)] {
            assert!(
                database
                    .statistics(&StatisticsPeriodInput {
                        kind: "recent".into(),
                        year: None,
                        month: None,
                        days,
                    })
                    .is_err(),
                "days={days:?} should be rejected"
            );
        }
    }
    #[test]
    fn play_status_defaults_updates_and_filters() {
        let d = Database::memory().unwrap();
        let first = game(&d);
        let second = d
            .create_game(
                &CreateGameInput {
                    title: "Completed".into(),
                    brand: Some("B".into()),
                    release_date: None,
                    thumbnail_path: None,
                    source: None,
                    executable_paths: vec![],
                    launch_command: None,
                },
                None,
            )
            .unwrap();
        assert_eq!(d.get_game(first).unwrap().summary.play_status, "unplayed");
        d.update_play_status(second, "completed").unwrap();
        let completed = d
            .list_games("", None, Some("completed"), "title", false)
            .unwrap();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].id, second);
        assert!(d.update_play_status(first, "invalid").is_err());
    }
}
