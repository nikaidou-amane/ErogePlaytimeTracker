use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    SessionStarted { game_id: i64 },
    SessionEnded { game_id: i64, session_id: i64 },
    FocusStarted { game_id: i64, session_id: i64 },
    FocusEnded { game_id: i64, session_id: i64 },
    BackgroundStarted { game_id: i64, session_id: i64 },
    BackgroundEnded { game_id: i64, session_id: i64 },
}

#[derive(Default)]
pub struct TrackerState {
    // A detected process is still tracked before it has a visible window,
    // but does not own a persisted session yet.
    running: HashMap<i64, Option<i64>>,
    focused: Option<i64>,
    background: HashSet<i64>,
}

impl TrackerState {
    pub fn reconcile_running<F>(
        &mut self,
        alive: &HashSet<i64>,
        windowed: &HashSet<i64>,
        mut create: F,
    ) -> Vec<Action>
    where
        F: FnMut(i64) -> Option<i64>,
    {
        let mut actions = vec![];
        let ended: Vec<_> = self
            .running
            .keys()
            .filter(|game| !alive.contains(game))
            .copied()
            .collect();
        for game in ended {
            let Some(session) = self.running.remove(&game).flatten() else {
                continue;
            };
            if self.focused == Some(game) {
                self.focused = None;
                actions.push(Action::FocusEnded {
                    game_id: game,
                    session_id: session,
                });
            }
            if self.background.remove(&game) {
                actions.push(Action::BackgroundEnded {
                    game_id: game,
                    session_id: session,
                });
            }
            actions.push(Action::SessionEnded {
                game_id: game,
                session_id: session,
            });
        }
        for &game in alive {
            let session = self.running.entry(game).or_default();
            if session.is_none()
                && windowed.contains(&game)
                && let Some(id) = create(game)
            {
                *session = Some(id);
                actions.push(Action::SessionStarted { game_id: game });
            }
        }
        actions
    }

    pub fn observe_windows(
        &mut self,
        foreground: Option<i64>,
        windowed: &HashSet<i64>,
    ) -> Vec<Action> {
        let foreground =
            foreground.filter(|game| self.session(*game).is_some() && windowed.contains(game));
        let mut actions = vec![];
        if foreground != self.focused {
            if let Some(game) = self.focused.take() {
                actions.push(Action::FocusEnded {
                    game_id: game,
                    session_id: self.session(game).expect("focused game has a session"),
                });
            }
            if let Some(game) = foreground {
                self.focused = Some(game);
                actions.push(Action::FocusStarted {
                    game_id: game,
                    session_id: self.session(game).expect("focused game has a session"),
                });
            }
        }

        let desired_background: HashSet<_> = self
            .running
            .keys()
            .filter(|game| {
                self.session(**game).is_some()
                    && windowed.contains(game)
                    && foreground != Some(**game)
            })
            .copied()
            .collect();
        let ended: Vec<_> = self
            .background
            .difference(&desired_background)
            .copied()
            .collect();
        for game in ended {
            self.background.remove(&game);
            actions.push(Action::BackgroundEnded {
                game_id: game,
                session_id: self.session(game).expect("background game has a session"),
            });
        }
        let started: Vec<_> = desired_background
            .difference(&self.background)
            .copied()
            .collect();
        for game in started {
            self.background.insert(game);
            actions.push(Action::BackgroundStarted {
                game_id: game,
                session_id: self.session(game).expect("background game has a session"),
            });
        }
        actions
    }

    pub fn running(&self) -> &HashMap<i64, Option<i64>> {
        &self.running
    }
    pub fn session(&self, game: i64) -> Option<i64> {
        self.running.get(&game).copied().flatten()
    }
    pub fn focused(&self) -> Option<i64> {
        self.focused
    }
    pub fn background(&self) -> &HashSet<i64> {
        &self.background
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn set(values: &[i64]) -> HashSet<i64> {
        values.iter().copied().collect()
    }

    #[test]
    fn startup_is_tracked_without_recording_until_a_window_appears() {
        let mut state = TrackerState::default();
        assert!(
            state
                .reconcile_running(&set(&[1]), &set(&[]), |_| panic!("no visible window"))
                .is_empty()
        );
        assert_eq!(state.running().get(&1), Some(&None));
        assert!(state.observe_windows(Some(1), &set(&[])).is_empty());
        assert_eq!(state.focused(), None);
        assert!(state.background().is_empty());
        assert_eq!(
            state.reconcile_running(&set(&[1]), &set(&[1]), |_| Some(10)),
            vec![Action::SessionStarted { game_id: 1 }]
        );
        assert_eq!(state.session(1), Some(10));
        assert!(matches!(
            state.observe_windows(Some(1), &set(&[1]))[0],
            Action::FocusStarted { .. }
        ));
    }

    #[test]
    fn visible_non_foreground_window_is_background() {
        let mut state = TrackerState::default();
        state.reconcile_running(&set(&[1]), &set(&[1]), |_| Some(10));
        assert!(matches!(
            state.observe_windows(None, &set(&[1]))[0],
            Action::BackgroundStarted { .. }
        ));
        assert_eq!(state.observe_windows(Some(1), &set(&[1])).len(), 2);
    }

    #[test]
    fn destroyed_window_does_not_start_background() {
        let mut state = TrackerState::default();
        state.reconcile_running(&set(&[1]), &set(&[1]), |_| Some(10));
        state.observe_windows(Some(1), &set(&[1]));
        let actions = state.observe_windows(None, &set(&[]));
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], Action::FocusEnded { .. }));
    }

    #[test]
    fn launcher_does_not_duplicate_session() {
        let mut state = TrackerState::default();
        state.reconcile_running(&set(&[1]), &set(&[1]), |_| Some(10));
        assert!(
            state
                .reconcile_running(&set(&[1]), &set(&[1]), |_| panic!("existing session"))
                .is_empty()
        );
        assert_eq!(state.session(1), Some(10));
    }

    #[test]
    fn multiple_games_track_background_independently() {
        let mut state = TrackerState::default();
        state.reconcile_running(&set(&[1, 2, 3]), &set(&[1, 2]), |game| Some(game + 10));
        assert_eq!(state.running().get(&3), Some(&None));
        let actions = state.observe_windows(Some(1), &set(&[1, 2]));
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, Action::BackgroundStarted { game_id: 2, .. }))
        );
        let actions = state.observe_windows(Some(2), &set(&[1, 2]));
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, Action::BackgroundStarted { game_id: 1, .. }))
        );
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, Action::BackgroundEnded { game_id: 2, .. }))
        );
        assert_eq!(state.background(), &set(&[1]));
    }

    #[test]
    fn windowless_bootstrap_exit_and_relaunch_create_only_the_visible_session() {
        let mut state = TrackerState::default();
        state.reconcile_running(&set(&[1]), &set(&[]), |_| panic!("bootstrap"));
        assert!(
            state
                .reconcile_running(&set(&[]), &set(&[]), |_| panic!("no processes"))
                .is_empty()
        );
        assert!(state.running().is_empty());
        state.reconcile_running(&set(&[1]), &set(&[]), |_| panic!("still starting"));
        assert_eq!(
            state.reconcile_running(&set(&[1]), &set(&[1]), |_| Some(20)),
            vec![Action::SessionStarted { game_id: 1 }]
        );
        state.observe_windows(Some(1), &set(&[1]));
        assert_eq!(
            state.reconcile_running(&set(&[]), &set(&[]), |_| panic!("game exited")),
            vec![
                Action::FocusEnded {
                    game_id: 1,
                    session_id: 20
                },
                Action::SessionEnded {
                    game_id: 1,
                    session_id: 20
                },
            ]
        );
    }

    #[test]
    fn temporary_window_loss_keeps_the_session_and_normal_relaunch_starts_another() {
        let mut state = TrackerState::default();
        state.reconcile_running(&set(&[1]), &set(&[1]), |_| Some(10));
        state.observe_windows(None, &set(&[1]));
        assert!(
            state
                .reconcile_running(&set(&[1]), &set(&[]), |_| panic!("window transition"))
                .is_empty()
        );
        assert_eq!(
            state.observe_windows(None, &set(&[])),
            vec![Action::BackgroundEnded {
                game_id: 1,
                session_id: 10
            }]
        );
        assert!(
            state
                .reconcile_running(&set(&[1]), &set(&[1]), |_| panic!("same session"))
                .is_empty()
        );
        assert_eq!(state.session(1), Some(10));
        assert_eq!(
            state.reconcile_running(&set(&[]), &set(&[]), |_| panic!("game exited")),
            vec![Action::SessionEnded {
                game_id: 1,
                session_id: 10
            }]
        );
        assert_eq!(
            state.reconcile_running(&set(&[1]), &set(&[1]), |_| Some(11)),
            vec![Action::SessionStarted { game_id: 1 }]
        );
        assert_eq!(state.session(1), Some(11));
    }

    #[test]
    fn failed_session_creation_retries_without_recording_intervals() {
        let mut state = TrackerState::default();
        assert!(
            state
                .reconcile_running(&set(&[1]), &set(&[1]), |_| None)
                .is_empty()
        );
        assert!(state.observe_windows(Some(1), &set(&[1])).is_empty());
        assert!(state.observe_windows(None, &set(&[1])).is_empty());
        assert_eq!(state.running().get(&1), Some(&None));
        assert_eq!(
            state.reconcile_running(&set(&[1]), &set(&[1]), |_| Some(10)),
            vec![Action::SessionStarted { game_id: 1 }]
        );
        assert_eq!(
            state.observe_windows(Some(1), &set(&[1])),
            vec![Action::FocusStarted {
                game_id: 1,
                session_id: 10
            }]
        );
    }

    #[test]
    fn visible_window_without_a_related_live_process_does_not_start_a_session() {
        let mut state = TrackerState::default();
        assert!(
            state
                .reconcile_running(&set(&[]), &set(&[1]), |_| panic!("no related process"))
                .is_empty()
        );
        assert!(state.observe_windows(Some(1), &set(&[1])).is_empty());
        assert!(state.running().is_empty());
    }

    #[test]
    fn shutdown_closes_recorded_sessions_and_discards_starting_games() {
        let mut state = TrackerState::default();
        state.reconcile_running(&set(&[1, 2, 3]), &set(&[1, 2]), |game| Some(game + 10));
        state.observe_windows(Some(1), &set(&[1, 2]));
        let actions = state.reconcile_running(&set(&[]), &set(&[]), |_| panic!("shutdown"));
        assert_eq!(actions.len(), 4);
        for action in [
            Action::FocusEnded {
                game_id: 1,
                session_id: 11,
            },
            Action::SessionEnded {
                game_id: 1,
                session_id: 11,
            },
            Action::BackgroundEnded {
                game_id: 2,
                session_id: 12,
            },
            Action::SessionEnded {
                game_id: 2,
                session_id: 12,
            },
        ] {
            assert!(actions.contains(&action));
        }
        assert!(state.running().is_empty());
        assert!(state.background().is_empty());
        assert_eq!(state.focused(), None);
    }
}
