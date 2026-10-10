mod platform;
pub mod state;
use crate::{
    database::{Database, normalize_path},
    models::{TrackingGameStatus, TrackingStatus},
};
use chrono::Utc;
use parking_lot::Mutex;
use state::{Action, TrackerState};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter};

const RECONCILIATION_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Clone)]
pub struct TrackingService {
    inner: Arc<Inner>,
}
struct Inner {
    db: Database,
    state: Mutex<TrackerState>,
    open_focus_interval: Mutex<Option<i64>>,
    open_background_intervals: Mutex<HashMap<i64, i64>>,
    observed_foreground: Mutex<Option<(u32, String)>>,
    // PID -> (game ID, normalized executable path). Keeping the path prevents
    // a recycled PID from inheriting a stale game association.
    known_pid_games: Mutex<HashMap<u32, (i64, String)>>,
    watched_pids: Mutex<HashSet<u32>>,
    games_that_had_windows: Mutex<HashSet<i64>>,
    windowless_since: Mutex<HashMap<i64, String>>,
    stop: AtomicBool,
    app: AppHandle,
}
impl TrackingService {
    pub fn start(db: Database, app: AppHandle) -> Self {
        let this = Self {
            inner: Arc::new(Inner {
                db,
                state: Mutex::new(TrackerState::default()),
                open_focus_interval: Mutex::new(None),
                open_background_intervals: Mutex::new(HashMap::new()),
                observed_foreground: Mutex::new(None),
                known_pid_games: Mutex::new(HashMap::new()),
                watched_pids: Mutex::new(HashSet::new()),
                games_that_had_windows: Mutex::new(HashSet::new()),
                windowless_since: Mutex::new(HashMap::new()),
                stop: AtomicBool::new(false),
                app,
            }),
        };
        let worker = this.clone();
        let events = platform::tracking_events();
        thread::spawn(move || {
            log::info!("tracker started with foreground and process-exit notifications");
            while !worker.inner.stop.load(Ordering::Relaxed) {
                if let Err(e) = worker.tick() {
                    log::error!("tracking reconciliation failed: {e:#}")
                }
                if let Ok(event) = events.recv_timeout(RECONCILIATION_INTERVAL) {
                    worker.handle_event(event);
                    while let Ok(event) = events.recv_timeout(Duration::from_millis(20)) {
                        worker.handle_event(event);
                    }
                }
            }
            log::info!("tracker stopped")
        });
        this
    }
    fn handle_event(&self, event: platform::TrackingEvent) {
        if let platform::TrackingEvent::ProcessExited(pid) = event {
            self.inner.watched_pids.lock().remove(&pid);
            log::info!("tracked process exited pid={pid}; reconciling immediately");
        }
    }
    fn tick(&self) -> anyhow::Result<()> {
        let registered = self.inner.db.registered_executables()?;
        let by_path: HashMap<String, i64> = registered
            .iter()
            .map(|(g, _, p)| (normalize_path(p), *g))
            .collect();
        let mut game_roots: HashMap<i64, Vec<String>> = HashMap::new();
        for (game, _, path) in &registered {
            if let Some(root) = executable_root(path) {
                game_roots.entry(*game).or_default().push(root);
            }
        }
        let processes = platform::processes()?;
        let mut pid_game = self.inner.known_pid_games.lock();
        pid_game.retain(|pid, (_, known_path)| {
            processes
                .get(pid)
                .and_then(|process| process.path.as_deref())
                .is_some_and(|path| normalize_path(path) == *known_path)
        });
        for (pid, info) in &processes {
            if let Some(g) = info
                .path
                .as_deref()
                .and_then(|path| by_path.get(&normalize_path(path)))
                && let Some(path) = info.path.as_deref()
            {
                pid_game.insert(*pid, (*g, normalize_path(path)));
            }
        }
        // Associate descendants with a registered launcher/game process only
        // when their executable is inside the registered game's directory.
        // DRM services and other global helpers can outlive the game and must
        // not keep its session open.
        let mut changed = true;
        while changed {
            changed = false;
            for (pid, info) in &processes {
                if !pid_game.contains_key(pid)
                    && let Some(game) = pid_game.get(&info.parent_pid).map(|known| known.0)
                    && let Some(path) = info.path.as_deref()
                    && is_in_game_root(path, game_roots.get(&game))
                {
                    pid_game.insert(*pid, (game, normalize_path(path)));
                    log::info!(
                        "associated child process pid={pid} executable={} game={game}",
                        info.path
                            .as_deref()
                            .and_then(|p| std::path::Path::new(p).file_name())
                            .and_then(|p| p.to_str())
                            .unwrap_or("<unavailable>")
                    );
                    changed = true;
                }
            }
        }
        {
            let mut watched = self.inner.watched_pids.lock();
            watched.retain(|pid| pid_game.contains_key(pid));
            for &pid in pid_game.keys() {
                if !watched.contains(&pid) && platform::watch_process_exit(pid) {
                    watched.insert(pid);
                    log::debug!("watching process exit pid={pid}");
                }
            }
        }
        let alive: HashSet<i64> = pid_game.values().map(|known| known.0).collect();
        let visible_window_pids = platform::visible_window_pids()?;
        let windowed: HashSet<i64> = visible_window_pids
            .iter()
            .filter_map(|pid| pid_game.get(pid).map(|known| known.0))
            .collect();
        let at = Utc::now().to_rfc3339();
        let running_before: HashSet<i64> =
            self.inner.state.lock().running().keys().copied().collect();
        let mut had_windows = self.inner.games_that_had_windows.lock();
        let mut windowless_since = self.inner.windowless_since.lock();
        for &game in &alive {
            if windowed.contains(&game) {
                had_windows.insert(game);
                windowless_since.remove(&game);
            } else if had_windows.contains(&game) {
                windowless_since.entry(game).or_insert_with(|| at.clone());
            }
        }
        let end_overrides: HashMap<i64, String> = running_before
            .difference(&alive)
            .filter_map(|game| windowless_since.get(game).map(|at| (*game, at.clone())))
            .collect();
        drop(windowless_since);
        drop(had_windows);
        let mut state = self.inner.state.lock();
        let actions = state.reconcile_running(&alive, &windowed, |g| {
            match self.inner.db.start_session(g, &at) {
                Ok(id) => {
                    log::info!("session started game={g} session={id}");
                    Some(id)
                }
                Err(e) => {
                    log::error!("session start failed: {e:#}");
                    None
                }
            }
        });
        drop(state);
        self.apply(actions, &at, &end_overrides)?;
        let foreground = platform::foreground_pid().map(|pid| {
            let path = platform::process_path(pid);
            let game = pid_game.get(&pid).map(|known| known.0).or_else(|| {
                path.as_deref()
                    .and_then(|p| by_path.get(&normalize_path(p)).copied())
            });
            (pid, path, game)
        });
        if !alive.is_empty() {
            let observed = foreground
                .as_ref()
                .map(|(pid, path, _)| (*pid, path.clone().unwrap_or_default()));
            let mut previous = self.inner.observed_foreground.lock();
            if *previous != observed {
                match &foreground {
                    Some((pid, path, game)) => log::info!(
                        "foreground transition pid={pid} executable={} matched_game={game:?}",
                        path.as_deref()
                            .and_then(|p| std::path::Path::new(p).file_name())
                            .and_then(|p| p.to_str())
                            .unwrap_or("<unavailable>")
                    ),
                    None => log::info!("foreground transition: no foreground window"),
                }
                *previous = observed;
            }
        }
        let fg = foreground.and_then(|(_, _, game)| game);
        drop(pid_game);
        let actions = self.inner.state.lock().observe_windows(fg, &windowed);
        self.apply(actions, &at, &HashMap::new())?;
        self.inner
            .games_that_had_windows
            .lock()
            .retain(|game| alive.contains(game));
        self.inner
            .windowless_since
            .lock()
            .retain(|game, _| alive.contains(game));
        self.inner.db.set_setting("last_seen", &at)?;
        self.emit();
        Ok(())
    }
    fn apply(
        &self,
        actions: Vec<Action>,
        at: &str,
        end_overrides: &HashMap<i64, String>,
    ) -> anyhow::Result<()> {
        for a in actions {
            match a {
                Action::SessionStarted { game_id } => {
                    log::info!("first visible window detected game={game_id}")
                }
                Action::SessionEnded {
                    game_id,
                    session_id,
                } => {
                    self.inner.db.end_session(
                        session_id,
                        end_overrides
                            .get(&game_id)
                            .map(String::as_str)
                            .unwrap_or(at),
                    )?;
                    self.inner
                        .open_background_intervals
                        .lock()
                        .remove(&session_id);
                    log::info!("session ended game={game_id}")
                }
                Action::FocusStarted {
                    game_id,
                    session_id,
                } => {
                    *self.inner.open_focus_interval.lock() =
                        Some(self.inner.db.start_legacy_focus_interval(session_id, at)?);
                    log::info!("foreground entered game={game_id}")
                }
                Action::FocusEnded {
                    game_id,
                    session_id: _,
                } => {
                    if let Some(id) = self.inner.open_focus_interval.lock().take() {
                        self.inner.db.end_legacy_focus_interval(id, at)?
                    }
                    log::info!("foreground left game={game_id}")
                }
                Action::BackgroundStarted {
                    game_id,
                    session_id,
                } => {
                    let id = self.inner.db.start_interval(session_id, at)?;
                    self.inner
                        .open_background_intervals
                        .lock()
                        .insert(session_id, id);
                    log::info!("background entered game={game_id}")
                }
                Action::BackgroundEnded {
                    game_id,
                    session_id,
                } => {
                    if let Some(id) = self
                        .inner
                        .open_background_intervals
                        .lock()
                        .remove(&session_id)
                    {
                        self.inner.db.end_interval(id, at)?;
                    }
                    log::info!("background left game={game_id}")
                }
            }
        }
        Ok(())
    }
    fn emit(&self) {
        let status = self.status();
        if let Some(tray) = self.inner.app.tray_by_id("tracker") {
            let label = if status.games.is_empty() {
                "Eroge Playtime Tracker - idle".to_string()
            } else {
                format!(
                    "Eroge Playtime Tracker - tracking {} game(s)",
                    status.games.len()
                )
            };
            let _ = tray.set_tooltip(Some(label));
        }
        let _ = self.inner.app.emit("tracking-status", status);
    }
    pub fn status(&self) -> TrackingStatus {
        let s = self.inner.state.lock();
        let games_that_had_windows = self.inner.games_that_had_windows.lock();
        let names: HashMap<i64, String> = self
            .inner
            .db
            .registered_executables()
            .unwrap_or_default()
            .into_iter()
            .map(|(g, n, _)| (g, n))
            .collect();
        TrackingStatus {
            games: s
                .running()
                .iter()
                .map(|(g, x)| TrackingGameStatus {
                    game_id: *g,
                    title: names.get(g).cloned().unwrap_or_default(),
                    session_id: *x,
                    phase: if x.is_none() {
                        "starting"
                    } else if s.focused() == Some(*g) {
                        "foreground"
                    } else if s.background().contains(g) {
                        "background"
                    } else if games_that_had_windows.contains(g) {
                        "window_transition"
                    } else {
                        "starting"
                    }
                    .to_string(),
                })
                .collect(),
        }
    }
    pub fn focused_game(&self) -> Option<(i64, i64)> {
        let foreground_pid = platform::foreground_pid()?;
        let observed_game = self
            .inner
            .known_pid_games
            .lock()
            .get(&foreground_pid)
            .map(|known| known.0)?;
        let state = self.inner.state.lock();
        let game = state.focused()?;
        if observed_game != game {
            return None;
        }
        state.session(game).map(|session| (game, session))
    }
    pub fn shutdown(&self) {
        self.inner.stop.store(true, Ordering::Relaxed);
        let at = Utc::now().to_rfc3339();
        let mut s = self.inner.state.lock();
        let actions = s.reconcile_running(&HashSet::new(), &HashSet::new(), |_| None);
        drop(s);
        if let Err(e) = self.apply(actions, &at, &HashMap::new()) {
            log::error!("clean shutdown failed: {e:#}")
        }
    }
}

fn executable_root(path: &str) -> Option<String> {
    let path = normalize_path(path);
    let (root, _) = path.rsplit_once('\\')?;
    Some(format!("{root}\\"))
}

fn is_in_game_root(path: &str, roots: Option<&Vec<String>>) -> bool {
    let path = normalize_path(path);
    roots.is_some_and(|roots| roots.iter().any(|root| path.starts_with(root)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descendants_must_stay_in_the_game_directory() {
        let roots = vec![executable_root(r"D:\vn\game\launcher.exe").unwrap()];
        assert!(is_in_game_root(r"D:\vn\game\main.bin", Some(&roots)));
        assert!(is_in_game_root(r"D:\vn\game\engine\game.exe", Some(&roots)));
        assert!(!is_in_game_root(
            r"C:\Program Files (x86)\SoftDenchi\SdProxy.exe",
            Some(&roots)
        ));
        assert!(!is_in_game_root(r"D:\vn\game-old\main.exe", Some(&roots)));
    }

    #[test]
    fn recording_begins_at_the_first_window_and_preserves_short_sessions() {
        let db = Database::memory().unwrap();
        let game = db
            .create_game(
                &crate::models::CreateGameInput {
                    title: "Game".into(),
                    brand: None,
                    release_date: None,
                    thumbnail_path: None,
                    erogamescape_id: None,
                    source_url: None,
                    executable_paths: vec![r"D:\vn\game\game.exe".into()],
                },
                None,
            )
            .unwrap();
        let mut state = TrackerState::default();
        let first_window_at = "2026-01-01T00:00:10.000Z";
        for (alive, windowed, at) in [
            (true, false, "2026-01-01T00:00:00Z"),
            (false, false, "2026-01-01T00:00:01Z"),
            (true, false, "2026-01-01T00:00:04Z"),
            (true, true, first_window_at),
            (true, true, "2026-01-01T00:00:10.100Z"),
        ] {
            state.reconcile_running(
                &if alive {
                    HashSet::from([game])
                } else {
                    HashSet::new()
                },
                &if windowed {
                    HashSet::from([game])
                } else {
                    HashSet::new()
                },
                |game| Some(db.start_session(game, at).unwrap()),
            );
            assert_eq!(db.list_sessions(game).unwrap().len(), usize::from(windowed));
        }
        let session = state.session(game).unwrap();
        db.end_session(session, "2026-01-01T00:00:10.200Z").unwrap();
        let sessions = db.list_sessions(game).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].launched_at, first_window_at);
        assert_eq!(sessions[0].running_seconds, Some(0));
        assert_eq!(sessions[0].playtime_seconds, 0);
        let summary = db.get_game(game).unwrap().summary;
        assert_eq!(summary.session_count, 1);
        assert_eq!(summary.total_running_seconds, 0);
    }
}
