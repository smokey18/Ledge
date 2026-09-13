//! Verified against Claude Code 2.1.268.

use crate::state::{Incoming, Signal};
use serde_json::Value;

pub const EVENTS: &[&str] = &[
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PermissionRequest",
    "Notification",
    "Stop",
    "StopFailure",
    "SessionEnd",
];

pub fn map(payload: &Value) -> Option<Incoming> {
    let event = payload.get("hook_event_name")?.as_str()?;
    let session_id = payload.get("session_id")?.as_str()?.to_string();
    let cwd = payload.get("cwd").and_then(Value::as_str).unwrap_or("");

    // Subagent activity belongs to its parent session.
    if payload.get("agent_id").and_then(Value::as_str).is_some() && event == "SessionEnd" {
        return None;
    }

    let state = match event {
        "UserPromptSubmit" | "PreToolUse" | "PostToolUse" => Signal::Working,
        "PermissionRequest" | "Notification" => Signal::Waiting,
        "Stop" => Signal::Completed,
        "StopFailure" | "SessionEnd" => Signal::Ended,
        _ => return None,
    };

    Some(Incoming {
        session_id,
        agent: "claude".into(),
        project_name: crate::state::project_name(cwd),
        cwd: cwd.to_string(),
        state,
        transcript: payload
            .get("transcript_path")
            .and_then(Value::as_str)
            .map(str::to_string),
        title: None,
    })
}

const TITLE_SCAN_LINES: usize = 200;

pub fn title_from(transcript: &str) -> Option<String> {
    use std::io::{BufRead, BufReader};

    let file = std::fs::File::open(transcript).ok()?;

    BufReader::new(file)
        .lines()
        .take(TITLE_SCAN_LINES)
        .map_while(Result::ok)
        .filter(|line| line.contains(r#""ai-title""#))
        .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
        .find_map(|value| {
            Some(value.get("aiTitle")?.as_str()?.trim().to_string()).filter(|t| !t.is_empty())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(name: &str) -> Value {
        json!({
            "hook_event_name": name,
            "session_id": "abc",
            "transcript_path": "/tmp/t.jsonl",
            "cwd": "/Users/x/Herd/mendmyhouse"
        })
    }

    #[test]
    fn maps_the_events_ledge_installs() {
        assert!(map(&ev("SessionStart")).is_none());
        assert_eq!(map(&ev("PreToolUse")).unwrap().state, Signal::Working);
        assert_eq!(
            map(&ev("PermissionRequest")).unwrap().state,
            Signal::Waiting
        );
        assert_eq!(map(&ev("Stop")).unwrap().state, Signal::Completed);
        assert_eq!(map(&ev("StopFailure")).unwrap().state, Signal::Ended);
        assert_eq!(map(&ev("SessionEnd")).unwrap().state, Signal::Ended);
        assert_eq!(map(&ev("PreToolUse")).unwrap().project_name, "mendmyhouse");
    }

    #[test]
    fn subagent_lifecycle_does_not_masquerade_as_its_own_session() {
        let mut e = ev("SessionEnd");
        e["agent_id"] = json!("sub-1");
        assert!(map(&e).is_none());
    }

    #[test]
    fn malformed_payloads_are_ignored_rather_than_fatal() {
        assert!(map(&json!({})).is_none());
        assert!(map(&json!({"hook_event_name": "Stop"})).is_none());
        assert!(map(&json!({"hook_event_name": 42, "session_id": "a"})).is_none());
        assert!(map(&ev("SomethingNew")).is_none());
        let e = json!({"hook_event_name": "Stop", "session_id": "a"});
        assert_eq!(map(&e).unwrap().project_name, "unknown");
    }
}
