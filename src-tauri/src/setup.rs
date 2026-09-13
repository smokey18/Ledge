use crate::{claude, codex};
use serde_json::{json, Value};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
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
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Ok(());
    };
    if raw.contains(MARKER) {
        return Ok(());
    }

    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".ledge-backup");
    std::fs::write(path.with_file_name(name), raw)
}

fn read_json(path: &Path) -> io::Result<Value> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(json!({})),
        Err(error) => return Err(error),
    };
    let value: Value = serde_json::from_str(&raw)?;
    if !value.is_object()
        || value.get("hooks").is_some_and(|hooks| {
            hooks
                .as_object()
                .is_none_or(|hooks| hooks.values().any(|groups| !groups.is_array()))
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected settings and hooks objects",
        ));
    }
    Ok(value)
}

pub(crate) fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    // Preserve symlinked configs and keep the replacement on the same filesystem.
    let path = match std::fs::canonicalize(path) {
        Ok(path) => path,
        Err(error) if error.kind() == io::ErrorKind::NotFound => path.to_path_buf(),
        Err(error) => return Err(error),
    };
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("no parent directory"))?;
    std::fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".ledge-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        if let Ok(metadata) = path.metadata() {
            file.set_permissions(metadata.permissions())?;
        }
        file.write_all(contents)?;
        std::fs::rename(&temporary, &path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

fn write_json(path: &Path, value: &Value) -> io::Result<()> {
    write_atomic(
        path,
        format!("{}\n", serde_json::to_string_pretty(value)?).as_bytes(),
    )
}

fn entry(binary: &str, agent: &str) -> Value {
    json!({
        "type": "command",
        "command": format!("'{}' {agent}", binary.replace('\'', "'\"'\"'")),
        "timeout": 5,
    })
}

fn is_ours(entry: &Value) -> bool {
    entry
        .get("command")
        .and_then(Value::as_str)
        .and_then(shlex::split)
        .is_some_and(|args| {
            args.first().is_some_and(|program| {
                Path::new(program)
                    .file_name()
                    .is_some_and(|name| name == MARKER)
            })
        })
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

        groups.push(json!({ "hooks": [entry(binary, agent)] }));
    }
}

fn drop_hooks(root: &mut Value) {
    let Some(object) = root.as_object_mut() else {
        return;
    };
    let Some(hooks) = object.get_mut("hooks").and_then(Value::as_object_mut) else {
        return;
    };

    let had_events = !hooks.is_empty();
    hooks.retain(|_, groups| {
        let Some(groups) = groups.as_array_mut() else {
            return true;
        };
        let was_empty = groups.is_empty();
        groups.retain_mut(|group| {
            let Some(entries) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                return true;
            };
            let was_empty = entries.is_empty();
            entries.retain(|entry| !is_ours(entry));
            was_empty || !entries.is_empty()
        });
        was_empty || !groups.is_empty()
    });

    if had_events && hooks.is_empty() {
        object.remove("hooks");
    }
}

pub fn connect(agent: &str, binary: &str) -> io::Result<()> {
    let Some((path, events)) = hooks_file(agent) else {
        return Ok(());
    };
    let mut root = read_json(&path)?;
    backup(&path)?;
    let original = root.clone();
    drop_hooks(&mut root);
    add_hooks(&mut root, events, binary, agent);
    if root == original {
        return Ok(());
    }
    write_json(&path, &root)
}

pub fn disconnect(agent: &str) -> io::Result<()> {
    let Some((path, _)) = hooks_file(agent).filter(|(path, _)| path.exists()) else {
        return Ok(());
    };
    let mut root = read_json(&path)?;
    let original = root.clone();
    drop_hooks(&mut root);

    if root == original {
        return Ok(());
    }
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

struct NotifyStatus {
    pub current: Option<Vec<String>>,
    pub is_ours: bool,
}

fn read_toml(path: &Path) -> io::Result<DocumentMut> {
    std::fs::read_to_string(path)?
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

fn notify_status() -> NotifyStatus {
    let current = read_toml(&codex_config()).ok().and_then(|document| {
        let array = document.get("notify")?.as_array()?;
        Some(
            array
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect::<Vec<_>>(),
        )
    });

    let is_ours = current.as_ref().is_some_and(|args| {
        args.first().is_some_and(|arg| {
            Path::new(arg)
                .file_name()
                .is_some_and(|name| name == MARKER)
        })
    });

    NotifyStatus { current, is_ours }
}

fn notify_disconnect() -> io::Result<()> {
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

    write_atomic(&path, document.to_string().as_bytes())
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
    fn removal_preserves_empty_groups_and_commands_that_only_mention_ledge() {
        let original = json!({"hooks": {
            "Stop": [{"hooks": []}, {"hooks": [{"command": "echo ledge-hook"}, {"command": "/tmp/ledge-hook-helper"}]}],
            "SessionStart": []
        }});
        let mut document = original.clone();
        add_hooks(&mut document, &["Stop"], BINARY, "claude");
        drop_hooks(&mut document);
        assert_eq!(document, original);
        let mut empty = json!({"hooks": {}});
        drop_hooks(&mut empty);
        assert_eq!(empty, json!({"hooks": {}}));
        assert!(is_ours(&entry("/tmp/a 'quoted' path/ledge-hook", "claude")));
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
    fn the_backup_is_one_file_and_only_ever_holds_the_pristine_config() {
        let path = std::env::temp_dir().join(format!("ledge-backup-{}.json", std::process::id()));
        let copy = PathBuf::from(format!("{}.ledge-backup", path.display()));
        let _ = std::fs::remove_file(&copy);

        std::fs::write(&path, "{\"model\":\"opus\"}").unwrap();
        backup(&path).unwrap();
        assert_eq!(
            std::fs::read_to_string(&copy).unwrap(),
            "{\"model\":\"opus\"}"
        );

        std::fs::write(&path, format!("{{\"hook\":\"{MARKER}\"}}")).unwrap();
        backup(&path).unwrap();
        assert_eq!(
            std::fs::read_to_string(&copy).unwrap(),
            "{\"model\":\"opus\"}"
        );

        for file in [path, copy] {
            let _ = std::fs::remove_file(file);
        }
    }

    #[test]
    fn malformed_configs_are_preserved_and_atomic_writes_follow_symlinks() {
        let scratch = std::env::temp_dir().join(format!("ledge-config-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).unwrap();
        let path = scratch.join("settings.json");
        for raw in ["{broken", "[]", r#"{"hooks":null}"#, r#"{"hooks":[]}"#] {
            std::fs::write(&path, raw).unwrap();
            assert!(read_json(&path).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), raw);
        }
        let link = scratch.join("linked.json");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        write_json(&link, &json!({"model":"opus"})).unwrap();
        assert!(link.is_symlink());
        assert_eq!(read_json(&path).unwrap(), json!({"model":"opus"}));
        std::fs::remove_dir_all(scratch).unwrap();
    }

    #[test]
    fn hook_paths_are_shell_quoted_without_expanding_user_text() {
        let binary = "/tmp/a'$HOME`echo bad`/ledge-hook";
        let command = entry(binary, "claude")["command"]
            .as_str()
            .unwrap()
            .to_owned();
        let output = std::process::Command::new("sh")
            .args(["-c", &format!("printf '%s\\n' {command}")])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!("{binary}\nclaude\n")
        );
    }
}
