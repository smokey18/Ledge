use crate::state::{Incoming, Signal};
use serde_json::Value;

pub const PLUGIN: &str = include_str!("opencode-plugin.js");

const PLACEHOLDER_TITLES: &[&str] = &["New session", "Child session"];

pub fn map(payload: &Value) -> Option<Incoming> {
    let session_id = payload.get("sessionID")?.as_str()?.to_string();
    let cwd = payload
        .get("directory")
        .and_then(Value::as_str)
        .unwrap_or("");

    let state = match payload.get("state")?.as_str()? {
        "working" => Signal::Working,
        "waiting" => Signal::Waiting,
        "done" => Signal::Completed,
        "failed" => Signal::Ended,
        _ => return None,
    };

    let title = payload
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| {
            !title.is_empty()
                && !PLACEHOLDER_TITLES
                    .iter()
                    .any(|placeholder| title.starts_with(placeholder))
        })
        .map(str::to_string);

    Some(Incoming {
        session_id,
        agent: "opencode".into(),
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

    fn message(state: &str) -> Value {
        json!({
            "state": state,
            "sessionID": "ses_1",
            "directory": "/Users/x/Herd/mendmyhouse",
            "title": "Fix type_id property missing error"
        })
    }

    #[test]
    fn maps_the_states_the_plugin_sends() {
        let working = map(&message("working")).unwrap();
        assert_eq!(working.state, Signal::Working);
        assert_eq!(working.agent, "opencode");
        assert_eq!(working.project_name, "mendmyhouse");
        assert_eq!(
            working.title.as_deref(),
            Some("Fix type_id property missing error")
        );
        assert_eq!(map(&message("waiting")).unwrap().state, Signal::Waiting);
        assert_eq!(map(&message("done")).unwrap().state, Signal::Completed);
        assert_eq!(map(&message("failed")).unwrap().state, Signal::Ended);
        assert!(map(&message("compacting")).is_none());
    }

    #[test]
    fn placeholder_titles_are_ignored_until_opencode_names_the_session() {
        for title in [
            "New session - 2026-10-27T10:00:00.000Z",
            "Child session - 2026-10-27T10:00:00.000Z",
            "  ",
        ] {
            let mut payload = message("done");
            payload["title"] = json!(title);
            assert!(map(&payload).unwrap().title.is_none());
        }
        let mut untitled = message("done");
        untitled["title"] = json!(null);
        assert!(map(&untitled).unwrap().title.is_none());
    }

    #[test]
    fn malformed_payloads_are_ignored_rather_than_fatal() {
        assert!(map(&json!({})).is_none());
        assert!(map(&json!({ "state": "done" })).is_none());
        assert!(map(&json!({ "sessionID": "a" })).is_none());
        let bare = json!({ "state": "done", "sessionID": "a" });
        assert_eq!(map(&bare).unwrap().project_name, "unknown");
    }

    #[test]
    fn the_plugin_loads_in_opencode_1_and_2_and_talks_to_ledges_socket() {
        assert!(PLUGIN.contains(r#"".ledge", "hook.sock""#));
        assert!(PLUGIN.contains("`opencode\\n${"));
        assert!(PLUGIN.contains(r#"export default { id: "ledge", server: LedgePlugin, setup };"#));
    }
}
