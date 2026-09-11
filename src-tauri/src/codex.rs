//! Verified against Codex CLI 0.154.0-alpha.6.2.

use crate::state::{Incoming, Signal};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::AppHandle;

const POLL_INTERVAL: Duration = Duration::from_millis(500);
const INITIAL_RECENCY: Duration = Duration::from_secs(10 * 60);

#[derive(Default)]
struct Rollout {
    offset: u64,
    session_id: String,
    cwd: String,
    root: bool,
    active: bool,
}

pub const EVENTS: &[&str] = &[
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PermissionRequest",
    "Stop",
    "SessionEnd",
];

pub fn map(payload: &Value) -> Option<Incoming> {
    let event = payload
        .get("hook_event_name")
        .or_else(|| payload.get("event_name"))?
        .as_str()?;
    let session_id = payload
        .get("session_id")
        .or_else(|| payload.get("thread_id"))?
        .as_str()?
        .to_string();
    let cwd = payload.get("cwd").and_then(Value::as_str).unwrap_or("");

    let state = match event {
        "UserPromptSubmit" | "PreToolUse" | "PostToolUse" => Signal::Working,
        "PermissionRequest" => Signal::Waiting,
        "Stop" => Signal::Completed,
        // Not `completed`: this fires when the session closes, not the turn.
        "SessionEnd" => Signal::Ended,
        _ => return None,
    };

    Some(Incoming {
        session_id,
        agent: "codex".into(),
        project_name: crate::state::project_name(cwd),
        cwd: cwd.to_string(),
        state,
    })
}

pub fn map_notify(payload: &Value) -> Option<Incoming> {
    if payload.get("type")?.as_str()? != "agent-turn-complete" {
        return None;
    }
    let session_id = payload.get("thread-id")?.as_str()?.to_string();
    let cwd = payload.get("cwd").and_then(Value::as_str).unwrap_or("");

    Some(Incoming {
        session_id,
        agent: "codex".into(),
        project_name: crate::state::project_name(cwd),
        cwd: cwd.to_string(),
        state: Signal::Completed,
    })
}

pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let root =
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".codex/sessions");
        let mut rollouts = HashMap::new();
        let mut first_scan = true;

        loop {
            for path in files_under(&root) {
                let Ok(metadata) = path.metadata() else {
                    continue;
                };
                let new = !rollouts.contains_key(&path);
                let rollout = rollouts.entry(path.clone()).or_default();

                if (new && !first_scan)
                    || metadata
                        .modified()
                        .ok()
                        .and_then(|time| time.elapsed().ok())
                        .is_some_and(|age| age <= INITIAL_RECENCY)
                {
                    let events = read_events(&path, rollout);
                    if new {
                        if let Some(event) = events.into_iter().last() {
                            if !first_scan || event.state == Signal::Working {
                                crate::dispatch_incoming(&app, event);
                            }
                        }
                    } else {
                        for event in events {
                            crate::dispatch_incoming(&app, event);
                        }
                    }
                } else if new {
                    read_meta(&path, rollout);
                    rollout.offset = metadata.len();
                }
            }
            first_scan = false;
            std::thread::sleep(POLL_INTERVAL);
        }
    });
}

fn read_meta(path: &Path, rollout: &mut Rollout) {
    let Ok(file) = File::open(path) else { return };
    let mut line = String::new();
    if BufReader::new(file).read_line(&mut line).is_ok() {
        if let Ok(value) = serde_json::from_str::<Value>(&line) {
            apply_meta(&value, rollout);
        }
    }
}

fn apply_meta(value: &Value, rollout: &mut Rollout) {
    let Some(payload) = value
        .get("payload")
        .filter(|_| value.get("type").and_then(Value::as_str) == Some("session_meta"))
    else {
        return;
    };
    rollout.session_id = payload
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    rollout.cwd = payload
        .get("cwd")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    rollout.root = payload.get("source").is_none_or(Value::is_string);
}

fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut dirs = vec![root.to_path_buf()];

    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|ext| ext == "jsonl") {
                files.push(path);
            }
        }
    }
    files
}

fn read_events(path: &Path, rollout: &mut Rollout) -> Vec<Incoming> {
    let Ok(mut file) = File::open(path) else {
        return Vec::new();
    };
    let Ok(length) = file.metadata().map(|metadata| metadata.len()) else {
        return Vec::new();
    };
    if length < rollout.offset {
        rollout.offset = 0;
    }
    if file.seek(std::io::SeekFrom::Start(rollout.offset)).is_err() {
        return Vec::new();
    }

    let mut reader = BufReader::new(file);
    let mut events = Vec::new();
    loop {
        let start = reader.stream_position().unwrap_or(rollout.offset);
        let mut line = String::new();
        let Ok(read) = reader.read_line(&mut line) else {
            break;
        };
        if read == 0 {
            break;
        }
        if !line.ends_with('\n') {
            rollout.offset = start;
            break;
        }
        rollout.offset = reader.stream_position().unwrap_or(length);

        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("session_meta") => {
                apply_meta(&value, rollout);
            }
            Some("turn_context") => {
                rollout.cwd = value
                    .get("payload")
                    .and_then(|payload| payload.get("cwd"))
                    .and_then(Value::as_str)
                    .unwrap_or(&rollout.cwd)
                    .to_string();
                if rollout.active {
                    push(&mut events, rollout, Signal::Working);
                }
            }
            Some("event_msg") => match value
                .get("payload")
                .and_then(|payload| payload.get("type"))
                .and_then(Value::as_str)
            {
                Some("task_started") => {
                    rollout.active = true;
                    push(&mut events, rollout, Signal::Working);
                }
                Some("task_complete") => {
                    rollout.active = false;
                    push(&mut events, rollout, Signal::Completed);
                }
                Some("turn_aborted") => {
                    rollout.active = false;
                    push(&mut events, rollout, Signal::Ended);
                }
                _ => {}
            },
            _ => {}
        }
    }
    events
}

fn push(events: &mut Vec<Incoming>, rollout: &Rollout, state: Signal) {
    if rollout.root && !rollout.session_id.is_empty() {
        events.push(Incoming {
            session_id: rollout.session_id.clone(),
            agent: "codex".into(),
            project_name: crate::state::project_name(&rollout.cwd),
            cwd: rollout.cwd.clone(),
            state,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    fn ev(name: &str) -> Value {
        json!({
            "hook_event_name": name,
            "session_id": "t-1",
            "cwd": "/Users/x/Herd/smartshifts"
        })
    }

    #[test]
    fn maps_codex_events() {
        assert!(map(&ev("SessionStart")).is_none());
        assert_eq!(map(&ev("PreToolUse")).unwrap().state, Signal::Working);
        assert_eq!(
            map(&ev("PermissionRequest")).unwrap().state,
            Signal::Waiting
        );
        assert_eq!(map(&ev("Stop")).unwrap().state, Signal::Completed);
        assert_eq!(map(&ev("SessionEnd")).unwrap().state, Signal::Ended);
        assert_eq!(map(&ev("PreToolUse")).unwrap().project_name, "smartshifts");
    }

    #[test]
    fn session_end_is_never_read_as_completion() {
        assert_ne!(map(&ev("SessionEnd")).unwrap().state, Signal::Completed);
    }

    #[test]
    fn notify_uses_hyphenated_keys_and_only_fires_on_turn_complete() {
        let payload = json!({
            "type": "agent-turn-complete",
            "thread-id": "t-1",
            "turn-id": "u-1",
            "cwd": "/Users/x/Herd/smartshifts",
            "last-assistant-message": "done"
        });
        let got = map_notify(&payload).unwrap();
        assert_eq!(got.state, Signal::Completed);
        assert_eq!(got.session_id, "t-1");
        assert_eq!(got.project_name, "smartshifts");

        let other = json!({"type": "something-else", "thread-id": "t-1"});
        assert!(map_notify(&other).is_none());
        assert!(map_notify(&json!({})).is_none());
    }

    #[test]
    fn rollout_events_track_root_turns_and_ignore_subagents() {
        let path = std::env::temp_dir().join(format!("ledge-codex-{}.jsonl", std::process::id()));
        let mut file = File::create(&path).unwrap();
        for value in [
            json!({"type":"session_meta","payload":{"id":"t-1","cwd":"/tmp/one","source":"vscode"}}),
            json!({"type":"event_msg","payload":{"type":"task_started"}}),
            json!({"type":"turn_context","payload":{"cwd":"/tmp/two"}}),
            json!({"type":"event_msg","payload":{"type":"task_complete"}}),
        ] {
            writeln!(file, "{value}").unwrap();
        }

        let events = read_events(&path, &mut Rollout::default());
        assert_eq!(events.len(), 3);
        assert_eq!(events[1].project_name, "two");
        assert_eq!(events.last().unwrap().state, Signal::Completed);

        std::fs::write(
            &path,
            format!(
                "{}\n{}\n",
                json!({"type":"session_meta","payload":{"id":"child","source":{"subagent":{}}}}),
                json!({"type":"event_msg","payload":{"type":"task_started"}})
            ),
        )
        .unwrap();
        assert!(read_events(&path, &mut Rollout::default()).is_empty());
        let _ = std::fs::remove_file(path);
    }
}
