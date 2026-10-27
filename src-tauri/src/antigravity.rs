use crate::state::{Incoming, Signal};
use serde_json::Value;

pub const EVENTS: &[&str] = &["PreInvocation", "Stop"];

pub fn map(event: &str, payload: &Value) -> Option<Incoming> {
    let session_id = payload.get("conversationId")?.as_str()?.to_string();
    let cwd = payload
        .get("workspacePaths")
        .and_then(Value::as_array)
        .and_then(|paths| paths.first())
        .and_then(Value::as_str)
        .unwrap_or("");

    let state = match event {
        "PreInvocation" => Signal::Working,
        "Stop" => {
            let failed = payload
                .get("error")
                .and_then(Value::as_str)
                .is_some_and(|error| !error.is_empty())
                || payload.get("terminationReason").and_then(Value::as_str) == Some("error");
            if failed {
                Signal::Ended
            } else if payload.get("fullyIdle").and_then(Value::as_bool) == Some(false) {
                Signal::Working
            } else {
                Signal::Completed
            }
        }
        _ => return None,
    };

    Some(Incoming {
        session_id,
        agent: "antigravity".into(),
        project_name: crate::state::project_name(cwd),
        cwd: cwd.to_string(),
        state,
        transcript: None,
        title: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn payload() -> Value {
        json!({
            "conversationId": "ec33ebf9-0cba-4100-8142-c61503f6c587",
            "workspacePaths": ["/Users/x/Herd/mendmyhouse"],
            "transcriptPath": "~/.gemini/antigravity/brain/ec33/transcript.jsonl",
            "modelName": "gemini-3.6-flash-medium"
        })
    }

    fn stop(reason: &str, error: &str, idle: bool) -> Value {
        let mut p = payload();
        p["terminationReason"] = json!(reason);
        p["error"] = json!(error);
        p["fullyIdle"] = json!(idle);
        p
    }

    #[test]
    fn maps_the_events_ledge_installs() {
        let working = map("PreInvocation", &payload()).unwrap();
        assert_eq!(working.state, Signal::Working);
        assert_eq!(working.agent, "antigravity");
        assert_eq!(working.project_name, "mendmyhouse");
        assert_eq!(working.cwd, "/Users/x/Herd/mendmyhouse");
        assert!(map("PreToolUse", &payload()).is_none());
        assert!(map("SessionStart", &payload()).is_none());
    }

    #[test]
    fn stop_means_done_only_once_the_agent_is_fully_idle_and_error_free() {
        assert_eq!(
            map("Stop", &stop("model_stop", "", true)).unwrap().state,
            Signal::Completed
        );
        assert_eq!(
            map("Stop", &stop("model_stop", "", false)).unwrap().state,
            Signal::Working
        );
        assert_eq!(
            map("Stop", &stop("error", "", true)).unwrap().state,
            Signal::Ended
        );
        assert_eq!(
            map("Stop", &stop("model_stop", "boom", true))
                .unwrap()
                .state,
            Signal::Ended
        );
        assert_eq!(map("Stop", &payload()).unwrap().state, Signal::Completed);
    }

    #[test]
    fn malformed_payloads_are_ignored_rather_than_fatal() {
        assert!(map("Stop", &json!({})).is_none());
        assert!(map("Stop", &json!({"conversationId": 7})).is_none());
        let bare = json!({"conversationId": "a"});
        assert_eq!(map("PreInvocation", &bare).unwrap().project_name, "unknown");
    }
}
