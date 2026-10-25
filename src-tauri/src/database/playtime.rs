use super::*;

#[derive(Clone, Default)]
pub(super) struct Coverage {
    ranges: Vec<(i64, i64)>,
    prefix: Vec<i64>,
}
impl Coverage {
    pub(super) fn new(mut ranges: Vec<(i64, i64)>) -> Self {
        ranges.sort_unstable();
        let mut merged: Vec<(i64, i64)> = Vec::new();
        for (start, end) in ranges {
            if end <= start {
                continue;
            }
            if let Some(previous) = merged.last_mut()
                && start <= previous.1
            {
                previous.1 = previous.1.max(end);
            } else {
                merged.push((start, end));
            }
        }
        let mut sum = 0_i64;
        let prefix = merged
            .iter()
            .map(|(start, end)| {
                let before = sum;
                sum = sum.saturating_add(end.saturating_sub(*start));
                before
            })
            .collect();
        Self {
            ranges: merged,
            prefix,
        }
    }
    fn before(&self, at: i64) -> i64 {
        let index = self.ranges.partition_point(|(start, _)| *start < at);
        if index == 0 {
            return 0;
        }
        let (start, end) = self.ranges[index - 1];
        self.prefix[index - 1].saturating_add(at.min(end).saturating_sub(start).max(0))
    }
    pub(super) fn between(&self, start: i64, end: i64) -> i64 {
        if end <= start {
            0
        } else {
            self.before(end).saturating_sub(self.before(start)).max(0)
        }
    }
}

pub(super) struct SessionTime {
    pub start: i64,
    pub end: Option<i64>,
    background: Coverage,
    overlap_prefix: Vec<i64>,
}
impl SessionTime {
    pub(super) fn new(
        start: i64,
        end: Option<i64>,
        background: Vec<(i64, i64)>,
        pause: &Coverage,
    ) -> Self {
        let background = Coverage::new(background);
        let mut sum = 0_i64;
        let overlap_prefix = background
            .ranges
            .iter()
            .map(|(start, end)| {
                let before = sum;
                sum = sum.saturating_add(pause.between(*start, *end));
                before
            })
            .collect();
        Self {
            start,
            end,
            background,
            overlap_prefix,
        }
    }
    fn overlap_before(&self, at: i64, pause: &Coverage) -> i64 {
        let index = self
            .background
            .ranges
            .partition_point(|(start, _)| *start < at);
        if index == 0 {
            return 0;
        }
        let (start, end) = self.background.ranges[index - 1];
        self.overlap_prefix[index - 1].saturating_add(pause.between(start, at.min(end)))
    }
    pub(super) fn background_seconds(&self, now: i64) -> i64 {
        self.background.between(self.start, self.end.unwrap_or(now))
    }
    pub(super) fn pause_excluded_seconds(
        &self,
        now: i64,
        settings: &AppSettings,
        pause: &Coverage,
    ) -> i64 {
        self.pause_exclusion(self.start, self.end.unwrap_or(now), now, settings, pause)
    }
    fn pause_exclusion(
        &self,
        start: i64,
        end: i64,
        now: i64,
        settings: &AppSettings,
        pause: &Coverage,
    ) -> i64 {
        if !settings.exclude_pause_time || end <= start {
            return 0;
        }
        let paused = pause.between(start.min(now), end.min(now));
        let overlap = if settings.exclude_background_time {
            self.overlap_before(end.min(now), pause) - self.overlap_before(start.min(now), pause)
        } else {
            0
        };
        (paused - overlap).max(0)
    }
    pub(super) fn playtime(
        &self,
        from: i64,
        to: i64,
        now: i64,
        settings: &AppSettings,
        pause: &Coverage,
    ) -> i64 {
        let start = self.start.max(from);
        let end = self.end.unwrap_or(now).min(to);
        if end <= start {
            return 0;
        }
        let background = if settings.exclude_background_time {
            self.background.between(start, end)
        } else {
            0
        };
        (end - start - background - self.pause_exclusion(start, end, now, settings, pause)).max(0)
    }
}

#[derive(Default)]
pub(super) struct Cache {
    version: u64,
    external_version: i64,
    pub ignored_changes: u64,
    index: Option<Arc<TimeIndex>>,
}
pub(super) struct TimeIndex {
    pub pause: Coverage,
    pub sessions: HashMap<i64, SessionTime>,
    games: HashMap<i64, Vec<i64>>,
}
impl TimeIndex {
    fn load(connection: &Connection) -> Result<Self> {
        let mut query = connection.prepare(
            "SELECT started_at,ended_at FROM tracking_pause_intervals ORDER BY started_at",
        )?;
        let pause = Coverage::new(
            query
                .query_map([], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
                })?
                .map(|r| {
                    let (start, end) = r?;
                    Ok((
                        timestamp_seconds(&start)?,
                        end.as_deref()
                            .map(timestamp_seconds)
                            .transpose()?
                            .unwrap_or(i64::MAX),
                    ))
                })
                .collect::<Result<_>>()?,
        );
        let mut backgrounds: HashMap<i64, Vec<(i64, i64)>> = HashMap::new();
        let mut query = connection
            .prepare("SELECT play_session_id,started_at,ended_at FROM background_intervals")?;
        for row in query.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })? {
            let (id, start, end) = row?;
            backgrounds.entry(id).or_default().push((
                timestamp_seconds(&start)?,
                end.as_deref()
                    .map(timestamp_seconds)
                    .transpose()?
                    .unwrap_or(i64::MAX),
            ));
        }
        let mut sessions = HashMap::new();
        let mut games: HashMap<i64, Vec<i64>> = HashMap::new();
        let mut query = connection.prepare(
            "SELECT id,game_id,launched_at,exited_at FROM play_sessions ORDER BY launched_at,id",
        )?;
        for row in query.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })? {
            let (id, game, start, end) = row?;
            sessions.insert(
                id,
                SessionTime::new(
                    timestamp_seconds(&start)?,
                    end.as_deref().map(timestamp_seconds).transpose()?,
                    backgrounds.remove(&id).unwrap_or_default(),
                    &pause,
                ),
            );
            games.entry(game).or_default().push(id);
        }
        for ids in games.values_mut() {
            ids.sort_unstable_by_key(|id| sessions[id].start);
        }
        Ok(Self {
            pause,
            sessions,
            games,
        })
    }
    pub(super) fn total(&self, game: i64, now: i64, settings: &AppSettings) -> i64 {
        self.games
            .get(&game)
            .into_iter()
            .flatten()
            .map(|id| self.sessions[id].playtime(i64::MIN, i64::MAX, now, settings, &self.pause))
            .sum()
    }
    pub(super) fn cumulative(
        &self,
        game: i64,
        points: &[i64],
        now: i64,
        settings: &AppSettings,
    ) -> Vec<i64> {
        let ids = self.games.get(&game).map(Vec::as_slice).unwrap_or_default();
        let mut prefix = vec![0_i64];
        for id in ids {
            prefix.push(
                prefix.last().unwrap()
                    + self.sessions[id].playtime(i64::MIN, i64::MAX, now, settings, &self.pause),
            );
        }
        points
            .iter()
            .map(|at| {
                let index = ids.partition_point(|id| self.sessions[id].start < *at);
                if index == 0 {
                    0
                } else {
                    prefix[index - 1]
                        + self.sessions[&ids[index - 1]].playtime(
                            i64::MIN,
                            *at,
                            now,
                            settings,
                            &self.pause,
                        )
                }
            })
            .collect()
    }
}
impl Database {
    pub(super) fn time_index(&self, connection: &Connection) -> Result<Arc<TimeIndex>> {
        let mut cache = self.1.lock();
        let version = connection
            .total_changes()
            .saturating_sub(cache.ignored_changes);
        let external_version =
            connection.pragma_query_value(None, "data_version", |row| row.get::<_, i64>(0))?;
        if cache.version != version
            || cache.external_version != external_version
            || cache.index.is_none()
        {
            cache.index = Some(Arc::new(TimeIndex::load(connection)?));
            cache.version = version;
            cache.external_version = external_version;
        }
        Ok(cache.index.as_ref().unwrap().clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_pause_exclusion_counts_only_the_additional_excluded_seconds() {
        for background in [
            vec![],
            vec![(-10, 15), (20, 35), (35, 45)],
            vec![(10, i64::MAX)],
        ] {
            for paused in [
                vec![],
                vec![(-20, 5), (12, 25), (40, 80)],
                vec![(20, i64::MAX)],
            ] {
                let pause = Coverage::new(paused.clone());
                for end in [None, Some(60)] {
                    let session = SessionTime::new(0, end, background.clone(), &pause);
                    for (b, p) in [(false, false), (true, false), (false, true), (true, true)] {
                        let settings = AppSettings {
                            exclude_background_time: b,
                            exclude_pause_time: p,
                            ..Default::default()
                        };
                        let now = 50;
                        let stop = end.unwrap_or(now);
                        let expected = (0..stop)
                            .filter(|second| {
                                p && *second < now
                                    && paused.iter().any(|(s, e)| s <= second && second < e)
                                    && !(b
                                        && background
                                            .iter()
                                            .any(|(s, e)| s <= second && second < e))
                            })
                            .count() as i64;
                        assert_eq!(
                            session.pause_excluded_seconds(now, &settings, &pause),
                            expected
                        );
                        let excluded_background = if b {
                            session.background_seconds(now)
                        } else {
                            0
                        };
                        assert_eq!(
                            session.playtime(0, stop, now, &settings, &pause)
                                + excluded_background
                                + expected,
                            stop
                        );
                    }
                }
            }
        }
    }
    fn pause_summary_game(db: &Database) -> i64 {
        db.create_game(
            &CreateGameInput {
                title: "Example".into(),
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
    fn session_pause_exclusion_recalculates_and_preserves_raw_background_time() {
        let db = Database::memory().unwrap();
        let game = pause_summary_game(&db);
        let id = db
            .manual_session(game, "2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z")
            .unwrap();
        db.create_interval(id, "2026-01-01T00:10:00Z", "2026-01-01T00:30:00Z")
            .unwrap();
        db.set_paused(true, "2026-01-01T00:20:00Z").unwrap();
        db.set_paused(false, "2026-01-01T00:40:00Z").unwrap();
        for (b, p, expected) in [
            (false, false, 0),
            (true, false, 0),
            (false, true, 1200),
            (true, true, 600),
        ] {
            db.set_setting(
                "app",
                &serde_json::to_string(&AppSettings {
                    exclude_background_time: b,
                    exclude_pause_time: p,
                    ..Default::default()
                })
                .unwrap(),
            )
            .unwrap();
            let result = db.list_sessions(game).unwrap().remove(0);
            assert_eq!(result.pause_excluded_seconds, expected);
            assert_eq!(result.background_seconds, 1200);
            assert_eq!(result.running_seconds, Some(3600));
            assert_eq!(
                result.playtime_seconds
                    + if b { result.background_seconds } else { 0 }
                    + result.pause_excluded_seconds,
                3600
            );
        }
        let pause = db.pause_intervals(None, 50).unwrap().remove(0);
        db.delete_pause(pause.id, &pause.updated_at, "2026-01-02T00:00:00Z")
            .unwrap();
        let result = db.list_sessions(game).unwrap().remove(0);
        assert_eq!(result.pause_excluded_seconds, 0);
        assert_eq!(result.background_seconds, 1200);
    }
    #[test]
    fn session_pause_exclusion_uses_the_same_second_rounding_as_playtime() {
        let db = Database::memory().unwrap();
        let game = pause_summary_game(&db);
        let id = db
            .manual_session(game, "2026-01-01T00:00:00.900Z", "2026-01-01T00:00:03.100Z")
            .unwrap();
        db.create_interval(id, "2026-01-01T00:00:01.900Z", "2026-01-01T00:00:02.100Z")
            .unwrap();
        db.set_paused(true, "2026-01-01T09:00:01.100+09:00")
            .unwrap();
        db.set_paused(false, "2026-01-01T09:00:02.900+09:00")
            .unwrap();
        let result = db.list_sessions(game).unwrap().remove(0);
        assert_eq!(result.running_seconds, Some(3));
        assert_eq!(result.playtime_seconds, 2);
        assert_eq!(result.pause_excluded_seconds, 0);
        assert_eq!(result.background_seconds, 1);
    }
    #[test]
    fn heartbeat_reuses_time_index_but_history_edits_invalidate_it() {
        let db = Database::memory().unwrap();
        let first = {
            let c = db.0.lock();
            db.time_index(&c).unwrap()
        };
        db.set_setting("last_seen", "2026-01-01T00:00:00Z").unwrap();
        let next = {
            let c = db.0.lock();
            db.time_index(&c).unwrap()
        };
        assert!(Arc::ptr_eq(&first, &next));
        db.set_paused(true, "2026-01-01T00:01:00Z").unwrap();
        let changed = {
            let c = db.0.lock();
            db.time_index(&c).unwrap()
        };
        assert!(!Arc::ptr_eq(&first, &changed));
    }

    #[test]
    #[ignore = "synthetic large-history performance check"]
    fn large_history_query_timings() {
        for count in [1000_i64, 10000] {
            let db = Database::memory().unwrap();
            {
                let mut c = db.0.lock();
                let tx = c.transaction().unwrap();
                for game in 1..=100 {
                    tx.execute("INSERT INTO games(id,title,created_at,updated_at) VALUES(?1,?2,'2020-01-01T00:00:00Z','2020-01-01T00:00:00Z')",params![game,format!("Game {game:03}")]).unwrap();
                }
                for id in 0..count {
                    let base = 1_600_000_000 + id * 3600;
                    let time = |value| DateTime::from_timestamp(value, 0).unwrap().to_rfc3339();
                    tx.execute("INSERT INTO play_sessions(id,game_id,launched_at,exited_at,created_at,updated_at,background_migrated) VALUES(?1,?2,?3,?4,?3,?4,1)",params![id+1,id%100+1,time(base),time(base+3000)]).unwrap();
                    for index in 0..10 {
                        tx.execute("INSERT INTO background_intervals(play_session_id,started_at,ended_at,created_at,updated_at) VALUES(?1,?2,?3,?2,?3)",params![id+1,time(base+index*200),time(base+index*200+100)]).unwrap();
                    }
                    tx.execute("INSERT INTO tracking_pause_intervals(started_at,ended_at,created_at,updated_at) VALUES(?1,?2,?1,?2)",params![time(base+400),time(base+600)]).unwrap();
                }
                tx.commit().unwrap();
            }
            let cold = std::time::Instant::now();
            let games = db
                .list_games("", None, None, "total_playtime", true)
                .unwrap();
            let cold = cold.elapsed();
            assert_eq!(
                games.iter().map(|g| g.total_playtime_seconds).sum::<i64>(),
                count * 1900
            );
            let warm = std::time::Instant::now();
            for _ in 0..10 {
                std::hint::black_box(
                    db.list_games("", None, None, "total_playtime", true)
                        .unwrap(),
                );
            }
            println!(
                "sessions={count}, background={}, pauses={count}: cold={cold:?}, warm_average={:?}",
                count * 10,
                warm.elapsed() / 10
            );
        }
    }
    #[test]
    fn union_is_counted_once_and_can_be_clipped_at_any_boundary() {
        let pause = Coverage::new(vec![(20, 40)]);
        let session = SessionTime::new(0, Some(60), vec![(10, 30)], &pause);
        for (background, paused, expected) in [
            (false, false, 60),
            (true, false, 40),
            (false, true, 40),
            (true, true, 30),
        ] {
            let settings = AppSettings {
                exclude_background_time: background,
                exclude_pause_time: paused,
                ..Default::default()
            };
            assert_eq!(session.playtime(0, 60, 60, &settings, &pause), expected);
        }
        let settings = AppSettings::default();
        assert_eq!(session.playtime(0, 25, 60, &settings, &pause), 10);
        assert_eq!(session.playtime(25, 60, 60, &settings, &pause), 20);
    }
    #[test]
    fn open_pause_applies_to_new_sessions_without_stopping_process_tracking() {
        let pause = Coverage::new(vec![(10, i64::MAX)]);
        for start in [0, 20] {
            let session = SessionTime::new(start, None, vec![], &pause);
            assert_eq!(
                session.playtime(i64::MIN, i64::MAX, 30, &AppSettings::default(), &pause),
                if start == 0 { 10 } else { 0 }
            );
        }
    }
}
