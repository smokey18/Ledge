use crate::state::{Incoming, Signal};
use serde_json::Value;

pub const EVENTS: &[&str] = &["beforeSubmitPrompt", "postToolUse", "stop"];

pub fn map(payload: &Value) -> Option<Incoming> {
    let event = payload.get("hook_event_name")?.as_str()?;
    let session_id = payload.get("conversation_id")?.as_str()?.to_string();
    let cwd = payload
        .get("workspace_roots")
        .and_then(Value::as_array)
        .and_then(|roots| roots.first())
        .and_then(Value::as_str)
        .unwrap_or("");

    let state = match event {
        "beforeSubmitPrompt" | "postToolUse" => Signal::Working,
        "stop" => match payload.get("status").and_then(Value::as_str) {
            Some("error") => Signal::Ended,
            _ => Signal::Completed,
        },
        _ => return None,
    };

    let title = (event == "beforeSubmitPrompt")
        .then(|| payload.get("prompt")?.as_str())
        .flatten()
        .and_then(crate::state::first_line_title);

    Some(Incoming {
        session_id,
        agent: "cursor".into(),
        project_name: crate::state::project_name(cwd),
        cwd: cwd.to_string(),
        state,
        transcript: None,
        title,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(name: &str) -> Value {
        json!({
            "hook_event_name": name,
            "conversation_id": "c1",
            "generation_id": "g1",
            "workspace_roots": ["/Users/x/Herd/mendmyhouse"],
            "cursor_version": "2.4.0"
        })
    }

    #[test]
    fn maps_the_events_ledge_installs() {
        let working = map(&ev("postToolUse")).unwrap();
        assert_eq!(working.state, Signal::Working);
        assert_eq!(working.agent, "cursor");
        assert_eq!(working.project_name, "mendmyhouse");
        assert_eq!(
            map(&ev("beforeSubmitPrompt")).unwrap().state,
            Signal::Working
        );
        assert!(map(&ev("preToolUse")).is_none());
        assert!(map(&ev("sessionStart")).is_none());
    }

    #[test]
    fn stop_is_done_unless_the_agent_errored() {
        let mut stop = ev("stop");
        stop["status"] = json!("completed");
        assert_eq!(map(&stop).unwrap().state, Signal::Completed);
        stop["status"] = json!("aborted");
        assert_eq!(map(&stop).unwrap().state, Signal::Completed);
        stop["status"] = json!("error");
        assert_eq!(map(&stop).unwrap().state, Signal::Ended);
    }

    #[test]
    fn the_title_is_the_first_line_of_the_prompt() {
        let mut e = ev("beforeSubmitPrompt");
        e["prompt"] = json!("\n  fix the login bug  \nand add a test");
        assert_eq!(map(&e).unwrap().title.as_deref(), Some("fix the login bug"));

        e["prompt"] = json!("x".repeat(200));
        assert_eq!(
            map(&e).unwrap().title.unwrap().chars().count(),
            crate::state::TITLE_MAX
        );

        let mut tool = ev("postToolUse");
        tool["prompt"] = json!("not a prompt event");
        assert!(map(&tool).unwrap().title.is_none());
    }

    #[test]
    fn malformed_payloads_are_ignored_rather_than_fatal() {
        assert!(map(&json!({})).is_none());
        assert!(map(&json!({"hook_event_name": "stop"})).is_none());
        let bare = json!({"hook_event_name": "stop", "conversation_id": "a"});
        assert_eq!(map(&bare).unwrap().project_name, "unknown");
    }
}
