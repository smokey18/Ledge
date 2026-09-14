//! Verified against Codex CLI 0.154.0-alpha.6.2.

use crate::state::{Incoming, Signal};
use notify::{RecursiveMode, Watcher};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Seek};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;
use tauri::AppHandle;

const RETRY_INTERVAL: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_secs(2);
const INACTIVE_POLL_INTERVAL: Duration = Duration::from_secs(10);
const INITIAL_RECENCY: Duration = Duration::from_secs(10 * 60);

#[derive(Default)]
struct Rollout {
    offset: u64,
    inode: u64,
    session_id: String,
    cwd: String,
    root: bool,
    active: bool,
    title: Option<String>,
}
const TITLE_MAX: usize = 80;

fn prompt_title(value: &Value) -> Option<String> {
    let payload = value.get("payload")?;
    if payload.get("role")?.as_str()? != "user" {
        return None;
    }

    let text = payload
        .get("content")?
        .as_array()?
        .iter()
        .find_map(|part| part.get("text")?.as_str())?;

    let line = text.lines().find(|line| !line.trim().is_empty())?.trim();
    if line.starts_with('<') || line.starts_with('#') {
        return None;
    }

    Some(line.chars().take(TITLE_MAX).collect())
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
        transcript: None,
        title: None,
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
        transcript: None,
        title: None,
    })
}

pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let root = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".codex")
            })
            .join("sessions");
        let mut rollouts = HashMap::new();
        let mut first_scan = true;
        let mut inactive_poll = std::time::Instant::now();

        loop {
            if !root.is_dir() {
                std::thread::sleep(RETRY_INTERVAL);
                continue;
            }
            let Ok(root) = root.canonicalize() else {
                std::thread::sleep(RETRY_INTERVAL);
                continue;
            };
            let (tx, rx) = mpsc::channel();
            let watcher = notify::recommended_watcher(tx).and_then(|mut watcher| {
                watcher.watch(&root, RecursiveMode::Recursive)?;
                Ok(watcher)
            });
            for path in files_under(&root) {
                for event in update_rollout(&path, &mut rollouts, first_scan) {
                    crate::dispatch_incoming(&app, event);
                }
            }
            first_scan = false;
            let _watcher = match watcher {
                Ok(watcher) => watcher,
                Err(error) => {
                    eprintln!("ledge: Codex watcher unavailable, retrying: {error}");
                    std::thread::sleep(RETRY_INTERVAL);
                    continue;
                }
            };

            loop {
                let first = match rx.recv_timeout(POLL_INTERVAL) {
                    Ok(event) => Some(event),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                let mut paths = HashSet::new();
                let mut rescan = false;
                let mut failed = false;
                for event in first.into_iter().chain(rx.try_iter()) {
                    match event {
                        Ok(event) => {
                            rescan |= event.need_rescan();
                            paths.extend(event.paths);
                        }
                        Err(error) => {
                            eprintln!("ledge: Codex watcher failed: {error}");
                            failed = true;
                        }
                    }
                }
                if failed || !root.is_dir() {
                    break;
                }
                if rescan {
                    paths = files_under(&root).into_iter().collect();
                    rollouts.retain(|path, _| paths.contains(path));
                }
                if inactive_poll.elapsed() >= INACTIVE_POLL_INTERVAL {
                    inactive_poll = std::time::Instant::now();
                    paths.extend(
                        rollouts
                            .iter()
                            .filter(|(_, rollout)| !rollout.active)
                            .map(|(path, _)| path.clone()),
                    );
                }
                let mut files = HashSet::new();
                for path in paths {
                    if path.is_dir() {
                        files.extend(files_under(&path));
                    } else if path.extension().is_some_and(|ext| ext == "jsonl") {
                        files.insert(path);
                    } else if !path.exists() {
                        rollouts.retain(|file, _| !file.starts_with(&path));
                    }
                }
                files.extend(
                    rollouts
                        .iter()
                        .filter(|(_, rollout)| rollout.active || !rollout.session_id.is_empty())
                        .map(|(path, _)| path.clone()),
                );
                for path in files {
                    for event in update_rollout(&path, &mut rollouts, false) {
                        crate::dispatch_incoming(&app, event);
                    }
                }
            }
        }
    });
}

fn update_rollout(
    path: &Path,
    rollouts: &mut HashMap<PathBuf, Rollout>,
    first_scan: bool,
) -> Vec<Incoming> {
    let Ok(metadata) = path.metadata() else {
        rollouts.remove(path);
        return Vec::new();
    };
    let new = !rollouts.contains_key(path);
    let rollout = rollouts.entry(path.to_path_buf()).or_default();
    if new
        && first_scan
        && !metadata
            .modified()
            .ok()
            .and_then(|time| time.elapsed().ok())
            .is_some_and(|age| age <= INITIAL_RECENCY)
    {
        rollout.offset = metadata.len();
        rollout.inode = metadata.ino();
        return Vec::new();
    }
    if metadata.len() == rollout.offset && metadata.ino() == rollout.inode {
        return Vec::new();
    }
    if rollout.session_id.is_empty() && rollout.offset > 0 {
        read_meta(path, rollout);
    }
    let events = read_events(path, rollout);
    if new {
        events
            .into_iter()
            .last()
            .filter(|event| !first_scan || event.state == Signal::Working)
            .into_iter()
            .collect()
    } else {
        events
    }
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
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                dirs.push(path);
            } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "jsonl") {
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
    let Ok(metadata) = file.metadata() else {
        return Vec::new();
    };
    let length = metadata.len();
    if length < rollout.offset || metadata.ino() != rollout.inode {
        *rollout = Rollout::default();
    }
    rollout.inode = metadata.ino();
    if file.seek(std::io::SeekFrom::Start(rollout.offset)).is_err() {
        return Vec::new();
    }

    let mut reader = BufReader::new(file);
    let mut events = Vec::new();
    loop {
        let mut line = String::new();
        let Ok(read) = reader.read_line(&mut line) else {
            break;
        };
        if read == 0 {
            break;
        }
        if !line.ends_with('\n') {
            break;
        }
        rollout.offset += read as u64;

        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("session_meta") => {
                apply_meta(&value, rollout);
            }
            Some("response_item") => {
                if rollout.title.is_none() {
                    rollout.title = prompt_title(&value);
                    if rollout.title.is_some() && rollout.active {
                        push(&mut events, rollout, Signal::Working);
                    }
                }
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
            transcript: None,
            title: rollout.title.clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str) -> Value {
        json!({"type": "response_item", "payload": {"type": "message", "role": "user",
               "content": [{"type": "input_text", "text": text}]}})
    }

    #[test]
    fn the_title_is_the_first_prompt_the_user_actually_typed() {
        assert_eq!(prompt_title(&user("<recommended_plugins> ...")), None);
        assert_eq!(
            prompt_title(&user("# AGENTS.md instructions\n\n<INSTRUCTIONS>")),
            None
        );
        assert_eq!(
            prompt_title(&json!({"payload": {"role": "assistant",
                "content": [{"text": "hello say it back"}]}})),
            None
        );
        assert_eq!(
            prompt_title(&user("\n  hello say it back \nsecond line")).as_deref(),
            Some("hello say it back")
        );
        assert_eq!(
            prompt_title(&user(&"x".repeat(200)))
                .unwrap()
                .chars()
                .count(),
            TITLE_MAX
        );
    }
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
    fn native_watcher_reports_new_nested_rollouts() {
        let root = std::env::temp_dir().join(format!("ledge-watch-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(tx).unwrap();
        watcher.watch(&root, RecursiveMode::Recursive).unwrap();
        let nested = root.join("year/month/day");
        std::fs::create_dir_all(&nested).unwrap();
        let path = nested.join("session.jsonl");
        std::fs::write(&path, "{}\n").unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut observed = false;
        while let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) {
            let Ok(Ok(event)) = rx.recv_timeout(remaining) else {
                break;
            };
            if event
                .paths
                .iter()
                .any(|changed| changed == &path || (changed.is_dir() && path.starts_with(changed)))
            {
                observed = true;
                break;
            }
        }
        drop(watcher);
        std::fs::remove_dir_all(root).unwrap();
        assert!(
            observed,
            "native watcher missed a newly created nested rollout"
        );
    }

    #[test]
    fn partial_lines_wait_for_completion_and_truncation_resets_metadata() {
        let path = std::env::temp_dir().join(format!("ledge-partial-{}.jsonl", std::process::id()));
        let meta =
            json!({"type":"session_meta","payload":{"id":"root","cwd":"/tmp","source":"cli"}});
        let start = json!({"type":"event_msg","payload":{"type":"task_started"}});
        std::fs::write(&path, format!("{meta}\n{start}")).unwrap();
        let mut rollouts = HashMap::new();
        assert!(update_rollout(&path, &mut rollouts, false).is_empty());
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(file).unwrap();
        assert_eq!(
            update_rollout(&path, &mut rollouts, false)[0].state,
            Signal::Working
        );
        assert!(update_rollout(&path, &mut rollouts, false).is_empty());
        std::fs::write(&path, format!("{start}\n")).unwrap();
        assert!(update_rollout(&path, &mut rollouts, false).is_empty());
        std::fs::remove_file(&path).unwrap();
        assert!(update_rollout(&path, &mut rollouts, false).is_empty());
        assert!(rollouts.is_empty());
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

    #[test]
    fn later_turn_in_the_same_rollout_is_reported_again() {
        let path =
            std::env::temp_dir().join(format!("ledge-codex-reopen-{}.jsonl", std::process::id()));
        let mut file = File::create(&path).unwrap();
        writeln!(
            file,
            "{}",
            json!({"type":"session_meta","payload":{"id":"t-1","cwd":"/tmp","source":"cli"}})
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            json!({"type":"event_msg","payload":{"type":"task_started"}})
        )
        .unwrap();
        let mut rollouts = HashMap::new();
        assert_eq!(
            update_rollout(&path, &mut rollouts, false)
                .into_iter()
                .map(|event| event.state)
                .collect::<Vec<_>>(),
            vec![Signal::Working]
        );

        writeln!(
            file,
            "{}",
            json!({"type":"event_msg","payload":{"type":"task_complete"}})
        )
        .unwrap();
        assert_eq!(
            update_rollout(&path, &mut rollouts, false)[0].state,
            Signal::Completed
        );
        writeln!(
            file,
            "{}",
            json!({"type":"event_msg","payload":{"type":"task_started"}})
        )
        .unwrap();
        assert_eq!(
            update_rollout(&path, &mut rollouts, false)[0].state,
            Signal::Working
        );
        let _ = std::fs::remove_file(path);
    }
}
