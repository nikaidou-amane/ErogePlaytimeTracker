use super::*;
use anyhow::ensure;
use chrono::SecondsFormat;

fn canonical(value: &str) -> Result<String> {
    Ok(DateTime::parse_from_rfc3339(value)
        .context("日時が不正です")?
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Nanos, true))
}
fn active(c: &Connection) -> Result<Option<TrackingPauseInterval>> {
    Ok(c.query_row("SELECT id,started_at,ended_at,updated_at FROM tracking_pause_intervals WHERE ended_at IS NULL", [], row).optional()?)
}
fn row(r: &rusqlite::Row) -> rusqlite::Result<TrackingPauseInterval> {
    Ok(TrackingPauseInterval {
        id: r.get(0)?,
        started_at: r.get(1)?,
        ended_at: r.get(2)?,
        updated_at: r.get(3)?,
    })
}
fn validate(
    c: &Connection,
    id: Option<i64>,
    start: &str,
    end: Option<&str>,
    at: &str,
) -> Result<()> {
    ensure!(
        start <= at && end.is_none_or(|end| end <= at && end >= start),
        "一時停止区間の日時は開始から現在までの範囲で指定してください。"
    );
    let overlaps: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM tracking_pause_intervals WHERE (?1 IS NULL OR id<>?1) AND COALESCE(ended_at,'9999')>?2 AND started_at<COALESCE(?3,'9999'))",params![id,start,end],|r|r.get(0))?;
    ensure!(!overlaps, "一時停止区間が既存の区間と重複しています。");
    Ok(())
}
fn heartbeat(c: &Connection, at: &str) -> Result<()> {
    c.execute("INSERT INTO settings(key,value,updated_at) VALUES('last_seen',?1,?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",[at])?;
    Ok(())
}

impl Database {
    pub fn active_pause(&self) -> Result<Option<TrackingPauseInterval>> {
        active(&self.0.lock())
    }
    pub fn pause_intervals(
        &self,
        before_id: Option<i64>,
        limit: u32,
    ) -> Result<Vec<TrackingPauseInterval>> {
        let c = self.0.lock();
        let mut q=c.prepare("SELECT id,started_at,ended_at,updated_at FROM tracking_pause_intervals WHERE (?1 IS NULL OR id<?1) ORDER BY id DESC LIMIT ?2")?;
        Ok(q.query_map(params![before_id, limit.clamp(1, 100)], row)?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn set_paused(&self, paused: bool, at: &str) -> Result<()> {
        let at = canonical(at)?;
        let mut c = self.0.lock();
        let tx = c.transaction()?;
        match (paused, active(&tx)?) {
            (true, None) => {
                validate(&tx, None, &at, None, &at)?;
                tx.execute("INSERT INTO tracking_pause_intervals(started_at,created_at,updated_at) VALUES(?1,?1,?1)",[&at])?;
            }
            (false, Some(pause)) => {
                ensure!(at >= pause.started_at, "一時停止区間の日時が不正です。");
                tx.execute(
                    "UPDATE tracking_pause_intervals SET ended_at=?1,updated_at=?1 WHERE id=?2",
                    params![at, pause.id],
                )?;
            }
            _ => return Ok(()),
        }
        heartbeat(&tx, &at)?;
        tx.commit()?;
        Ok(())
    }
    pub fn update_pause(
        &self,
        id: i64,
        start: &str,
        end: Option<&str>,
        expected: &str,
        at: &str,
    ) -> Result<()> {
        let start = canonical(start)?;
        let end = end.map(canonical).transpose()?;
        let at = canonical(at)?;
        let mut c = self.0.lock();
        let tx = c.transaction()?;
        let previous = tx
            .query_row(
                "SELECT id,started_at,ended_at,updated_at FROM tracking_pause_intervals WHERE id=?",
                [id],
                row,
            )
            .optional()?
            .context("一時停止区間が見つかりません。")?;
        ensure!(
            previous.updated_at == expected,
            "一時停止区間が変更されています。再読み込みしてください。"
        );
        ensure!(
            previous.ended_at.is_none() || end.is_some(),
            "終了済みの一時停止区間を記録中には戻せません。"
        );
        validate(&tx, Some(id), &start, end.as_deref(), &at)?;
        tx.execute("UPDATE tracking_pause_intervals SET started_at=?1,ended_at=?2,updated_at=?3 WHERE id=?4",params![start,end,at,id])?;
        heartbeat(&tx, &at)?;
        tx.commit()?;
        Ok(())
    }
    pub fn delete_pause(&self, id: i64, expected: &str, at: &str) -> Result<()> {
        let mut c = self.0.lock();
        let tx = c.transaction()?;
        let deleted = tx.execute(
            "DELETE FROM tracking_pause_intervals WHERE id=? AND updated_at=?",
            params![id, expected],
        )?;
        ensure!(
            deleted == 1,
            "一時停止区間が変更されています。再読み込みしてください。"
        );
        heartbeat(&tx, at)?;
        tx.commit()?;
        Ok(())
    }
}

pub(crate) fn close_open_pauses(c: &Connection, at: &str) -> Result<()> {
    if let Some(pause) = active(c)? {
        let at = canonical(at)?.max(pause.started_at);
        c.execute(
            "UPDATE tracking_pause_intervals SET ended_at=?1,updated_at=?1 WHERE id=?2",
            params![at, pause.id],
        )?;
    }
    Ok(())
}

pub(crate) fn validate_pause_records(c: &Connection) -> Result<()> {
    let mut q =
        c.prepare("SELECT started_at,ended_at FROM tracking_pause_intervals ORDER BY started_at")?;
    let mut previous_end: Option<String> = None;
    for row in q.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
    })? {
        let (start, end) = row?;
        let start = canonical(&start)?;
        let end = end.context("バックアップに記録中の一時停止区間があります。")?;
        let end = canonical(&end)?;
        ensure!(
            end >= start
                && previous_end
                    .as_ref()
                    .is_none_or(|previous| previous <= &start),
            "一時停止履歴の整合性チェックに失敗しました。"
        );
        previous_end = Some(end);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn game(db: &Database, title: &str) -> i64 {
        db.create_game(
            &CreateGameInput {
                title: title.into(),
                brand: None,
                release_date: None,
                thumbnail_path: None,
                source: None,
                executable_paths: vec![],
                launch_command: None,
            },
            None,
        )
        .unwrap()
    }
    #[test]
    fn all_calculation_modes_agree_across_games_sessions_timestamps_sorting_and_statistics() {
        let db = Database::memory().unwrap();
        let a = game(&db, "A");
        let b = game(&db, "B");
        let c = game(&db, "C");
        let session = db
            .manual_session(a, "2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z")
            .unwrap();
        db.create_interval(session, "2026-01-01T00:10:00Z", "2026-01-01T00:30:00Z")
            .unwrap();
        db.manual_session(b, "2026-01-01T00:25:00Z", "2026-01-01T00:50:00Z")
            .unwrap();
        db.manual_session(c, "2026-01-01T10:00:00Z", "2026-01-01T10:40:00Z")
            .unwrap();
        db.create_timestamp(a, "middle", "2026-01-01T00:35:00Z")
            .unwrap();
        db.create_timestamp(a, "end", "2026-01-01T01:00:00Z")
            .unwrap();
        db.set_paused(true, "2026-01-01T00:20:00Z").unwrap();
        db.set_paused(false, "2026-01-01T00:40:00Z").unwrap();
        for (background, paused, expected, middle) in [
            (false, false, 3600, 2100),
            (true, false, 2400, 900),
            (false, true, 2400, 1200),
            (true, true, 1800, 600),
        ] {
            let settings = AppSettings {
                exclude_background_time: background,
                exclude_pause_time: paused,
                ..Default::default()
            };
            db.set_setting("app", &serde_json::to_string(&settings).unwrap())
                .unwrap();
            assert_eq!(
                db.get_game(a).unwrap().summary.total_playtime_seconds,
                expected
            );
            let sessions = db.list_sessions(a).unwrap();
            assert_eq!(sessions[0].playtime_seconds, expected);
            assert_eq!(sessions[0].background_seconds, 1200);
            let points = db.timestamps(a).unwrap();
            assert_eq!(points[0].playtime_seconds, middle);
            assert_eq!(points[1].playtime_seconds, expected);
            assert_eq!(points[1].since_previous_seconds, expected - middle);
            assert_eq!(
                db.game_statistics(a)
                    .unwrap()
                    .summary
                    .total_playtime_seconds,
                expected
            );
            let report = db
                .statistics(&StatisticsPeriodInput {
                    kind: "all".into(),
                    year: None,
                    month: None,
                })
                .unwrap();
            assert_eq!(
                report.summary.total_playtime_seconds,
                expected + if paused { 600 } else { 1500 } + 2400
            );
            let sorted = db
                .list_games("", None, None, "total_playtime", true)
                .unwrap();
            assert_eq!(sorted[0].id, if background && paused { c } else { a });
        }
        db.set_setting(
            "app",
            &serde_json::to_string(&AppSettings::default()).unwrap(),
        )
        .unwrap();
        let pause = db.pause_intervals(None, 50).unwrap().remove(0);
        db.update_pause(
            pause.id,
            "2026-01-01T00:20:00Z",
            Some("2026-01-01T00:35:00Z"),
            &pause.updated_at,
            "2026-01-01T02:00:00Z",
        )
        .unwrap();
        assert_eq!(
            db.game_statistics(a)
                .unwrap()
                .summary
                .total_playtime_seconds,
            2100
        );
        assert_eq!(
            db.statistics(&StatisticsPeriodInput {
                kind: "all".into(),
                year: None,
                month: None
            })
            .unwrap()
            .summary
            .total_playtime_seconds,
            5400
        );
        assert_eq!(db.get_game(a).unwrap().summary.total_playtime_seconds, 2100);
        assert_eq!(db.get_game(b).unwrap().summary.total_playtime_seconds, 900);
        let pause = db.pause_intervals(None, 50).unwrap().remove(0);
        db.delete_pause(pause.id, &pause.updated_at, "2026-01-01T02:01:00Z")
            .unwrap();
        assert_eq!(
            db.statistics(&StatisticsPeriodInput {
                kind: "all".into(),
                year: None,
                month: None
            })
            .unwrap()
            .summary
            .total_playtime_seconds,
            6300
        );
        assert_eq!(db.get_game(a).unwrap().summary.total_playtime_seconds, 2400);
        assert_eq!(db.get_game(b).unwrap().summary.total_playtime_seconds, 1500);
    }
    #[test]
    fn statistics_clip_pause_overlap_at_day_month_and_year_boundaries() {
        let db = Database::memory().unwrap();
        let a = game(&db, "A");
        let b = game(&db, "B");
        let boundary = local_day_start(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()).unwrap();
        let time = |offset| {
            DateTime::from_timestamp(boundary + offset, 0)
                .unwrap()
                .to_rfc3339()
        };
        let session = db.manual_session(a, &time(-3600), &time(7200)).unwrap();
        db.create_interval(session, &time(-1800), &time(1800))
            .unwrap();
        db.manual_session(b, &time(1800), &time(5400)).unwrap();
        db.set_paused(true, &time(-900)).unwrap();
        db.set_paused(false, &time(3600)).unwrap();
        for (background, paused) in [(false, false), (true, false), (false, true), (true, true)] {
            db.set_setting(
                "app",
                &serde_json::to_string(&AppSettings {
                    exclude_background_time: background,
                    exclude_pause_time: paused,
                    ..Default::default()
                })
                .unwrap(),
            )
            .unwrap();
            let before = 3600 - if background { 1800 } else { 0 } - if paused { 900 } else { 0 }
                + if background && paused { 900 } else { 0 };
            let after = 7200 - if background { 1800 } else { 0 } - if paused { 3600 } else { 0 }
                + if background && paused { 1800 } else { 0 };
            let other = 3600 - if paused { 1800 } else { 0 };
            for (kind, year, month, a_total, b_total) in [
                ("all", None, None, before + after, other),
                ("month", Some(2025), Some(12), before, 0),
                ("month", Some(2026), Some(1), after, other),
                ("year", Some(2025), None, before, 0),
                ("year", Some(2026), None, after, other),
            ] {
                let report = db
                    .statistics(&StatisticsPeriodInput {
                        kind: kind.into(),
                        year,
                        month,
                    })
                    .unwrap();
                assert_eq!(report.summary.total_playtime_seconds, a_total + b_total);
                assert_eq!(
                    report
                        .days
                        .iter()
                        .map(|day| day.playtime_seconds)
                        .sum::<i64>(),
                    a_total + b_total
                );
                assert_eq!(
                    report
                        .games
                        .iter()
                        .map(|game| game.playtime_seconds)
                        .sum::<i64>(),
                    a_total + b_total
                );
                assert_eq!(
                    report
                        .games
                        .iter()
                        .find(|game| game.game_id == a)
                        .unwrap()
                        .playtime_seconds,
                    a_total
                );
                assert_eq!(
                    report
                        .games
                        .iter()
                        .find(|game| game.game_id == b)
                        .map(|game| game.playtime_seconds)
                        .unwrap_or(0),
                    b_total
                );
                assert_eq!(
                    report.summary.longest_session.unwrap().playtime_seconds,
                    a_total.max(b_total)
                );
                for day in &report.days {
                    assert_eq!(
                        day.games
                            .iter()
                            .map(|game| game.playtime_seconds)
                            .sum::<i64>(),
                        day.playtime_seconds
                    );
                    if day.date == "2025-12-31" {
                        assert_eq!(day.playtime_seconds, before);
                    }
                    if day.date == "2026-01-01" {
                        assert_eq!(day.playtime_seconds, after + other);
                    }
                }
            }
            assert_eq!(
                db.game_statistics(a)
                    .unwrap()
                    .summary
                    .total_playtime_seconds,
                before + after
            );
            assert_eq!(
                db.list_sessions(a).unwrap()[0].playtime_seconds,
                before + after
            );
        }
    }
    #[test]
    fn statistics_follow_open_pause_and_sessions_started_while_paused() {
        let db = Database::memory().unwrap();
        let a = game(&db, "A");
        let b = game(&db, "B");
        let at = Utc::now().timestamp();
        let time = |offset| {
            DateTime::from_timestamp(at + offset, 0)
                .unwrap()
                .to_rfc3339()
        };
        let first = db.start_session(a, &time(-600)).unwrap();
        db.start_interval(first, &time(-300)).unwrap();
        db.set_paused(true, &time(-200)).unwrap();
        let second = db.start_session(b, &time(-100)).unwrap();
        let period = StatisticsPeriodInput {
            kind: "all".into(),
            year: None,
            month: None,
        };
        assert_eq!(db.list_sessions(a).unwrap()[0].playtime_seconds, 300);
        assert_eq!(db.list_sessions(b).unwrap()[0].playtime_seconds, 0);
        assert_eq!(
            db.game_statistics(a)
                .unwrap()
                .summary
                .total_playtime_seconds,
            300
        );
        assert_eq!(
            db.game_statistics(b)
                .unwrap()
                .summary
                .total_playtime_seconds,
            0
        );
        assert_eq!(
            db.statistics(&period)
                .unwrap()
                .summary
                .total_playtime_seconds,
            300
        );
        db.set_paused(false, &time(-50)).unwrap();
        db.end_session(first, &time(0)).unwrap();
        db.end_session(second, &time(0)).unwrap();
        assert_eq!(db.list_sessions(b).unwrap()[0].playtime_seconds, 50);
        assert_eq!(
            db.statistics(&period)
                .unwrap()
                .summary
                .total_playtime_seconds,
            350
        );
    }
    #[test]
    fn recovery_uses_last_seen_and_settings_and_legacy_focus_remain_compatible() {
        let db = Database::memory().unwrap();
        let a = game(&db, "A");
        let id = db
            .manual_session(a, "2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z")
            .unwrap();
        db.set_setting("app", r#"{"exclude_background_time":true}"#)
            .unwrap();
        assert!(db.settings().unwrap().exclude_pause_time);
        assert!(db.settings().unwrap().tracking_pause_hotkey.is_empty());
        db.set_paused(true, "2026-01-01T00:10:00Z").unwrap();
        db.recover_orphans("2026-01-01T00:20:00Z").unwrap();
        assert!(db.active_pause().unwrap().is_none());
        assert_eq!(db.list_sessions(a).unwrap()[0].playtime_seconds, 3000);
        let focus:i64=db.0.lock().query_row("SELECT SUM(CAST(strftime('%s',ended_at) AS INTEGER)-CAST(strftime('%s',started_at) AS INTEGER)) FROM focus_intervals WHERE play_session_id=?",[id],|r|r.get(0)).unwrap();
        assert_eq!(focus, 3600);
    }
    #[test]
    fn pause_lifecycle_edit_delete_and_conflict_are_global() {
        let db = Database::memory().unwrap();
        db.set_paused(true, "2026-01-01T00:10:00Z").unwrap();
        db.set_paused(true, "2026-01-01T00:11:00Z").unwrap();
        assert_eq!(db.pause_intervals(None, 100).unwrap().len(), 1);
        let first = db.active_pause().unwrap().unwrap();
        db.set_paused(false, "2026-01-01T00:20:00Z").unwrap();
        assert!(
            db.delete_pause(first.id, &first.updated_at, "2026-01-01T00:21:00Z")
                .is_err()
        );
        let first = db.pause_intervals(None, 100).unwrap().remove(0);
        db.update_pause(
            first.id,
            "2026-01-01T00:12:00Z",
            Some("2026-01-01T00:18:00Z"),
            &first.updated_at,
            "2026-01-01T00:22:00Z",
        )
        .unwrap();
        db.set_paused(true, "2026-01-01T00:30:00Z").unwrap();
        let second = db.active_pause().unwrap().unwrap();
        assert!(
            db.update_pause(
                first.id,
                "2026-01-01T00:12:00Z",
                Some("2026-01-01T00:35:00Z"),
                "2026-01-01T00:22:00.000000000Z",
                "2026-01-01T00:40:00Z"
            )
            .is_err()
        );
        db.delete_pause(second.id, &second.updated_at, "2026-01-01T00:40:00Z")
            .unwrap();
        assert!(db.active_pause().unwrap().is_none());
    }
}
