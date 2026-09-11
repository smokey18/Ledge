//! Removal must restore the file exactly, so entries carry a marker and the
//! original is copied first.

use crate::{claude, codex};
use serde::Serialize;
use serde_json::{json, Value};
use std::io;
use std::path::{Path, PathBuf};
use toml_edit::DocumentMut;

const MARKER: &str = "ledge-hook";

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

fn hooks_file(agent: &str) -> Option<(PathBuf, &'static [&'static str])> {
    match agent {
        "claude" => Some((home().join(".claude/settings.json"), claude::EVENTS)),
        "codex" => Some((home().join(".codex/hooks.json"), codex::EVENTS)),
        _ => None,
    }
}

fn codex_config() -> PathBuf {
    home().join(".codex/config.toml")
}

fn backup(path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();

    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".ledge-backup.{stamp}"));
    std::fs::copy(path, path.with_file_name(name))?;

    Ok(())
}

fn read_json(path: &Path) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

fn write_json(path: &Path, value: &Value) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, format!("{}\n", serde_json::to_string_pretty(value)?))
}

fn entry(binary: &str, agent: &str) -> Value {
    json!({
        "type": "command",
        "command": format!("{binary:?} {agent}"),
        "timeout": 5,
    })
}

fn is_ours(entry: &Value) -> bool {
    entry
        .get("command")
        .and_then(Value::as_str)
        .is_some_and(|command| command.contains(MARKER))
}

fn add_hooks(root: &mut Value, events: &[&str], binary: &str, agent: &str) {
    let hooks = root
        .as_object_mut()
        .expect("root is an object")
        .entry("hooks")
        .or_insert_with(|| json!({}));

    for event in events {
        let Some(groups) = hooks
            .as_object_mut()
            .unwrap()
            .entry(*event)
            .or_insert_with(|| json!([]))
            .as_array_mut()
        else {
            continue;
        };

        let existing = groups
            .iter_mut()
            .filter_map(|group| group.get_mut("hooks")?.as_array_mut())
            .flatten()
            .find(|candidate| is_ours(candidate));

        match existing {
            Some(stale) => *stale = entry(binary, agent),
            None => groups.push(json!({ "hooks": [entry(binary, agent)] })),
        }
    }
}

fn drop_hooks(root: &mut Value) {
    let Some(object) = root.as_object_mut() else {
        return;
    };
    let Some(hooks) = object.get_mut("hooks").and_then(Value::as_object_mut) else {
        return;
    };

    for groups in hooks.values_mut() {
        let Some(groups) = groups.as_array_mut() else {
            continue;
        };

        for group in groups.iter_mut() {
            if let Some(entries) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                entries.retain(|entry| !is_ours(entry));
            }
        }
        groups.retain(|group| {
            group
                .get("hooks")
                .and_then(Value::as_array)
                .is_none_or(|entries| !entries.is_empty())
        });
    }
    hooks.retain(|_, groups| groups.as_array().is_none_or(|groups| !groups.is_empty()));

    if hooks.is_empty() {
        object.remove("hooks");
    }
}

pub fn connect(agent: &str, binary: &str) -> io::Result<()> {
    let Some((path, events)) = hooks_file(agent) else {
        return Ok(());
    };
    backup(&path)?;

    let mut root = read_json(&path);
    drop_hooks(&mut root);
    add_hooks(&mut root, events, binary, agent);
    write_json(&path, &root)
}

pub fn disconnect(agent: &str) -> io::Result<()> {
    let Some((path, _)) = hooks_file(agent).filter(|(path, _)| path.exists()) else {
        return Ok(());
    };
    backup(&path)?;

    let mut root = read_json(&path);
    drop_hooks(&mut root);
    write_json(&path, &root)
}

pub fn is_connected(agent: &str) -> bool {
    hooks_file(agent).is_some_and(|(path, _)| {
        std::fs::read_to_string(path).is_ok_and(|raw| raw.contains(MARKER))
    })
}

pub fn remove_legacy_codex() {
    if is_connected("codex") {
        let _ = disconnect("codex");
    }
    if notify_status().is_ours {
        let _ = notify_disconnect();
    }
}

// Codex's `notify` is a single command slot rather than a list, so taking it
// over means carrying the displaced command along.

#[derive(Serialize)]
pub struct NotifyStatus {
    pub current: Option<Vec<String>>,
    pub is_ours: bool,
}

fn read_toml(path: &Path) -> io::Result<DocumentMut> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .parse::<DocumentMut>()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn notify_array(args: Vec<String>) -> toml_edit::Item {
    let mut array = toml_edit::Array::new();
    for arg in args {
        array.push(arg);
    }
    toml_edit::value(array)
}

pub fn notify_status() -> NotifyStatus {
    let current = read_toml(&codex_config()).ok().and_then(|document| {
        let array = document.get("notify")?.as_array()?;
        Some(
            array
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect::<Vec<_>>(),
        )
    });

    let is_ours = current
        .as_ref()
        .is_some_and(|args| args.first().is_some_and(|arg| arg.contains(MARKER)));

    NotifyStatus { current, is_ours }
}

pub fn notify_connect(binary: &str) -> io::Result<()> {
    let status = notify_status();
    if status.is_ours {
        return Ok(());
    }

    let path = codex_config();
    backup(&path)?;
    let mut document = read_toml(&path)?;

    let mut args = vec![binary.to_string(), "codex-notify".into()];
    if let Some(displaced) = status.current.filter(|args| !args.is_empty()) {
        args.push("--".into());
        args.extend(displaced);
    }
    document["notify"] = notify_array(args);

    std::fs::write(&path, document.to_string())
}

pub fn notify_disconnect() -> io::Result<()> {
    let status = notify_status();
    if !status.is_ours {
        return Ok(());
    }

    let path = codex_config();
    backup(&path)?;
    let mut document = read_toml(&path)?;

    let displaced: Vec<String> = status
        .current
        .unwrap_or_default()
        .into_iter()
        .skip_while(|arg| arg != "--")
        .skip(1)
        .collect();

    if displaced.is_empty() {
        document.remove("notify");
    } else {
        document["notify"] = notify_array(displaced);
    }

    std::fs::write(&path, document.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BINARY: &str = "/opt/Ledge/ledge-hook";

    #[test]
    fn connecting_then_disconnecting_leaves_foreign_hooks_untouched() {
        let original = json!({
            "model": "opus",
            "hooks": {
                "SessionStart": [{ "hooks": [{ "type": "command", "command": "other-tool" }] }]
            }
        });

        let mut document = original.clone();
        add_hooks(&mut document, claude::EVENTS, BINARY, "claude");
        assert!(serde_json::to_string(&document).unwrap().contains(MARKER));

        drop_hooks(&mut document);
        assert_eq!(document, original);
    }

    #[test]
    fn disconnecting_drops_the_hooks_key_it_created_but_not_one_it_found() {
        let mut document = json!({ "model": "opus" });
        add_hooks(&mut document, &["Stop"], BINARY, "claude");
        drop_hooks(&mut document);

        assert_eq!(document, json!({ "model": "opus" }));
    }

    #[test]
    fn reconnecting_refreshes_and_drops_obsolete_events() {
        let mut document = json!({});
        add_hooks(&mut document, &["SessionStart", "Stop"], BINARY, "claude");
        drop_hooks(&mut document);
        add_hooks(&mut document, &["Stop"], "/new/ledge-hook", "claude");

        assert!(document["hooks"].get("SessionStart").is_none());
        let groups = document["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["hooks"].as_array().unwrap().len(), 1);
        assert!(serde_json::to_string(&document).unwrap().contains("/new/"));
    }

    #[test]
    fn a_real_config_survives_connect_and_disconnect_unchanged() {
        let Some(real_home) = std::env::var_os("HOME").map(PathBuf::from) else {
            return;
        };
        let (claude_src, codex_src) = (
            real_home.join(".claude/settings.json"),
            real_home.join(".codex/config.toml"),
        );
        if !claude_src.exists() || !codex_src.exists() {
            return;
        }

        let scratch = std::env::temp_dir().join(format!("ledge-roundtrip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(scratch.join(".claude")).unwrap();
        std::fs::create_dir_all(scratch.join(".codex")).unwrap();
        std::fs::copy(&claude_src, scratch.join(".claude/settings.json")).unwrap();
        std::fs::copy(&codex_src, scratch.join(".codex/config.toml")).unwrap();

        std::env::set_var("HOME", &scratch);

        let _ = disconnect("claude");
        let _ = disconnect("codex");
        let _ = notify_disconnect();

        let claude_copy = scratch.join(".claude/settings.json");
        let codex_copy = scratch.join(".codex/config.toml");
        let before_claude = std::fs::read_to_string(&claude_copy).unwrap();
        let before_codex = std::fs::read_to_string(&codex_copy).unwrap();

        connect("claude", BINARY).unwrap();
        connect("codex", BINARY).unwrap();
        notify_connect(BINARY).unwrap();

        assert!(std::fs::read_to_string(&claude_copy)
            .unwrap()
            .contains(MARKER));
        assert!(scratch.join(".codex/hooks.json").exists());

        let during_codex = std::fs::read_to_string(&codex_copy).unwrap();
        assert!(during_codex.contains(MARKER));
        if before_codex.contains("notify = [") {
            assert!(during_codex.contains("\"--\""), "displaced command dropped");
        }

        disconnect("claude").unwrap();
        disconnect("codex").unwrap();
        notify_disconnect().unwrap();

        assert_eq!(
            std::fs::read_to_string(&claude_copy).unwrap().trim(),
            before_claude.trim()
        );
        assert_eq!(std::fs::read_to_string(&codex_copy).unwrap(), before_codex);

        std::env::set_var("HOME", &real_home);
        let _ = std::fs::remove_dir_all(&scratch);
    }
}
