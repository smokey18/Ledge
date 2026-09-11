use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

/// A `working` session with no event for this long is assumed dead.
pub const STALE_MS: i64 = 10 * 60 * 1000;
/// Idle sessions are noise; results are worth coming back to.
const FORGET_IDLE_MS: i64 = 20 * 60 * 1000;
const FORGET_RESULT_MS: i64 = 2 * 60 * 60 * 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Working,
    Waiting,
    Completed,
    Failed,
    Idle,
}

impl State {
    fn is_active(self) -> bool {
        matches!(self, State::Working | State::Waiting)
    }
}

/// An adapter sees that a session ended; only the store knows if that is a failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    Working,
    Waiting,
    Completed,
    Ended,
}

pub struct Incoming {
    pub session_id: String,
    pub agent: String,
    pub project_name: String,
    pub cwd: String,
    pub state: Signal,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionEvent {
    pub session_id: String,
    pub agent: String,
    pub project_name: String,
    #[serde(default)]
    pub cwd: String,
    pub state: State,
    pub started_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    /// One per display, so a docked monitor restores its own position.
    #[serde(default)]
    pub positions: HashMap<String, (f64, f64)>,
    #[serde(default)]
    pub last_display: Option<String>,
    #[serde(default)]
    pub expanded: bool,
    /// Hooks are fire-and-forget; without this a restart loses every session.
    #[serde(default)]
    pub sessions: Vec<SessionEvent>,
}

pub struct Ledge {
    pub sessions: Mutex<HashMap<String, SessionEvent>>,
    pub settings: Mutex<Settings>,
    move_gen: AtomicU64,
    move_watcher: AtomicBool,
}

pub fn project_name(cwd: &str) -> String {
    std::path::Path::new(cwd)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

fn settings_path(app: &AppHandle) -> PathBuf {
    let dir = app.path().app_config_dir().expect("no app config dir");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("settings.json")
}

impl Ledge {
    pub fn load(app: &AppHandle) -> Self {
        let settings: Settings = std::fs::read_to_string(settings_path(app))
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();

        let sessions = settings
            .sessions
            .iter()
            .cloned()
            .map(|mut session| {
                // Nothing can confirm restored work is still alive.
                if session.state.is_active() {
                    session.state = State::Idle;
                }
                (session.session_id.clone(), session)
            })
            .collect();

        Ledge {
            sessions: Mutex::new(sessions),
            settings: Mutex::new(settings),
            move_gen: AtomicU64::new(0),
            move_watcher: AtomicBool::new(false),
        }
    }

    pub fn save(&self, app: &AppHandle) {
        let mut settings = self.settings.lock().unwrap().clone();
        settings.sessions = self.sessions.lock().unwrap().values().cloned().collect();

        if let Ok(json) = serde_json::to_string_pretty(&settings) {
            let _ = std::fs::write(settings_path(app), json);
        }
    }

    pub fn snapshot(&self) -> Vec<SessionEvent> {
        let mut sessions: Vec<SessionEvent> = self
            .sessions
            .lock()
            .unwrap()
            .values()
            .filter(|session| session.state != State::Idle)
            .cloned()
            .collect();
        sessions.sort_by_key(|session| Reverse(session.updated_at));
        sessions
    }

    pub fn apply(&self, incoming: Incoming, now: i64) -> Option<State> {
        let mut sessions = self.sessions.lock().unwrap();

        // A fresh session replaces old results for the same agent and project,
        // but another live session may genuinely be running beside it.
        if incoming.state == Signal::Working && !incoming.cwd.is_empty() {
            sessions.retain(|id, session| {
                id == &incoming.session_id
                    || session.agent != incoming.agent
                    || session.cwd != incoming.cwd
                    || session.state.is_active()
            });
        }
        let previous = sessions.get(&incoming.session_id).map(|s| s.state);

        let state = match incoming.state {
            Signal::Working => State::Working,
            Signal::Waiting => State::Waiting,
            Signal::Completed => State::Completed,
            // Ending mid-turn means completion was never reported.
            Signal::Ended => match previous {
                Some(state) if state.is_active() => State::Failed,
                Some(state) => state,
                None => State::Idle,
            },
        };

        let session = sessions
            .entry(incoming.session_id.clone())
            .or_insert_with(|| SessionEvent {
                session_id: incoming.session_id,
                agent: incoming.agent,
                project_name: incoming.project_name.clone(),
                cwd: incoming.cwd.clone(),
                state,
                started_at: now,
                updated_at: now,
            });

        if state == State::Working && !session.state.is_active() {
            session.started_at = now;
        }

        let changed = session.state != state
            || session.project_name != incoming.project_name
            || session.cwd != incoming.cwd;
        session.state = state;
        session.project_name = incoming.project_name;
        session.cwd = incoming.cwd;
        session.updated_at = now;

        (changed || previous.is_none()).then_some(state)
    }

    pub fn sweep(&self, now: i64) -> bool {
        let mut sessions = self.sessions.lock().unwrap();
        let before = sessions.len();
        let mut changed = false;

        for session in sessions.values_mut() {
            if session.state.is_active() && now - session.updated_at > STALE_MS {
                session.state = State::Idle;
                changed = true;
            }
        }

        sessions.retain(|_, session| {
            let age = now - session.updated_at;
            match session.state {
                State::Idle => age < FORGET_IDLE_MS,
                State::Completed | State::Failed => age < FORGET_RESULT_MS,
                _ => true,
            }
        });

        changed || sessions.len() != before
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.lock().unwrap().is_empty()
    }

    pub fn note_move(&self) -> u64 {
        self.move_gen.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn move_generation(&self) -> u64 {
        self.move_gen.load(Ordering::SeqCst)
    }

    pub fn claim_move_watch(&self) -> bool {
        !self.move_watcher.swap(true, Ordering::SeqCst)
    }

    pub fn finish_move_watch(&self) {
        self.move_watcher.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Ledge {
        Ledge {
            sessions: Mutex::new(HashMap::new()),
            settings: Mutex::new(Settings::default()),
            move_gen: AtomicU64::new(0),
            move_watcher: AtomicBool::new(false),
        }
    }

    fn incoming(state: Signal) -> Incoming {
        Incoming {
            session_id: "s1".into(),
            agent: "claude".into(),
            project_name: "Ledge".into(),
            cwd: "/Users/x/Ledge".into(),
            state,
        }
    }

    fn state_of(ledge: &Ledge) -> State {
        ledge.sessions.lock().unwrap()["s1"].state
    }

    #[test]
    fn ending_mid_turn_fails_but_preserves_a_completed_result() {
        let ledge = store();
        ledge.apply(incoming(Signal::Working), 0);
        ledge.apply(incoming(Signal::Ended), 1);
        assert_eq!(state_of(&ledge), State::Failed);

        let ledge = store();
        ledge.apply(incoming(Signal::Working), 0);
        ledge.apply(incoming(Signal::Completed), 1);
        ledge.apply(incoming(Signal::Ended), 2);
        assert_eq!(state_of(&ledge), State::Completed);
    }

    #[test]
    fn a_new_turn_restarts_the_clock_but_a_continuing_one_does_not() {
        let ledge = store();
        ledge.apply(incoming(Signal::Working), 100);
        ledge.apply(incoming(Signal::Working), 200);
        assert_eq!(ledge.sessions.lock().unwrap()["s1"].started_at, 100);

        ledge.apply(incoming(Signal::Completed), 300);
        ledge.apply(incoming(Signal::Working), 400);
        assert_eq!(ledge.sessions.lock().unwrap()["s1"].started_at, 400);
    }

    #[test]
    fn a_state_change_is_announced_once_and_a_repeat_is_not() {
        let ledge = store();
        assert_eq!(
            ledge.apply(incoming(Signal::Working), 0),
            Some(State::Working)
        );
        assert_eq!(ledge.apply(incoming(Signal::Working), 1), None);
        assert_eq!(
            ledge.apply(incoming(Signal::Completed), 2),
            Some(State::Completed)
        );
    }

    #[test]
    fn work_that_stops_reporting_goes_idle_not_working_forever() {
        let ledge = store();
        ledge.apply(incoming(Signal::Working), 0);
        assert!(!ledge.sweep(STALE_MS - 1));
        assert!(ledge.sweep(STALE_MS + 1));
        assert_eq!(state_of(&ledge), State::Idle);
    }

    #[test]
    fn old_sessions_are_forgotten_so_the_store_cannot_grow_forever() {
        let ledge = store();
        ledge.apply(incoming(Signal::Completed), 0);
        ledge.apply(
            Incoming {
                session_id: "s2".into(),
                ..incoming(Signal::Ended)
            },
            0,
        );

        ledge.sweep(FORGET_IDLE_MS + 1);
        assert_eq!(ledge.snapshot().len(), 1);

        ledge.sweep(FORGET_RESULT_MS + 1);
        assert!(ledge.is_empty());
    }

    #[test]
    fn a_new_session_replaces_inactive_duplicates_but_not_live_ones() {
        let ledge = store();
        ledge.apply(incoming(Signal::Completed), 0);
        ledge.apply(
            Incoming {
                session_id: "s2".into(),
                ..incoming(Signal::Working)
            },
            1,
        );
        assert_eq!(ledge.snapshot().len(), 1);

        ledge.apply(
            Incoming {
                session_id: "s3".into(),
                ..incoming(Signal::Working)
            },
            2,
        );
        assert_eq!(ledge.snapshot().len(), 2);
    }

    #[test]
    fn the_snapshot_puts_the_latest_update_first() {
        let ledge = store();
        for (id, state, now) in [("a", Signal::Waiting, 1), ("b", Signal::Completed, 2)] {
            ledge.apply(
                Incoming {
                    session_id: id.into(),
                    ..incoming(state)
                },
                now,
            );
        }
        let order: Vec<String> = ledge.snapshot().into_iter().map(|s| s.session_id).collect();
        assert_eq!(order, ["b", "a"]);
    }

    #[test]
    fn project_name_is_the_directory_not_the_path() {
        assert_eq!(project_name("/Users/x/Herd/mendmyhouse"), "mendmyhouse");
        assert_eq!(project_name(""), "unknown");
    }
}
