//! The socket lives in a 0700 directory: the filesystem is the access control.

use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Manager};

const MAX_PAYLOAD_BYTES: u64 = 8 * 1024 * 1024;
const MAX_AGENT_LINE_BYTES: u64 = 64;
const READ_TIMEOUT: Duration = Duration::from_millis(500);

pub fn socket_path() -> PathBuf {
    dirs_ledge().join("hook.sock")
}

fn dirs_ledge() -> PathBuf {
    crate::home().join(".ledge")
}

pub fn start(app: AppHandle) -> std::io::Result<()> {
    let dir = dirs_ledge();
    std::fs::create_dir_all(&dir)?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;

    let lock = std::fs::File::create(dir.join("instance.lock"))?;
    lock.try_lock().map_err(std::io::Error::other)?;
    app.manage(lock);

    let path = socket_path();
    // A socket left by a crashed run would block the bind.
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let _ = stream.set_read_timeout(Some(READ_TIMEOUT));

            let mut buffer = Vec::new();
            if Read::take(&mut stream, MAX_PAYLOAD_BYTES + MAX_AGENT_LINE_BYTES)
                .read_to_end(&mut buffer)
                .is_err()
            {
                continue;
            }
            if let Some((agent, payload)) = parse(&buffer) {
                crate::dispatch(&app, &agent, payload);
            }
        }
    });

    Ok(())
}

fn parse(buffer: &[u8]) -> Option<(String, serde_json::Value)> {
    let (agent, payload) = buffer.split_at(buffer.iter().position(|byte| *byte == b'\n')?);
    let agent = std::str::from_utf8(agent).ok()?.trim();

    let antigravity = agent
        .strip_prefix("antigravity:")
        .is_some_and(|event| crate::antigravity::EVENTS.contains(&event));
    if !antigravity && !matches!(agent, "claude" | "cursor" | "opencode") {
        return None;
    }
    Some((
        agent.to_string(),
        serde_json::from_slice(&payload[1..]).ok()?,
    ))
}

pub fn cleanup() {
    let _ = std::fs::remove_file(socket_path());
}

pub fn hook_binary_path(app: &AppHandle) -> Option<String> {
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("ledge-hook")));
    let in_resources = app
        .path()
        .resource_dir()
        .ok()
        .map(|dir| dir.join("ledge-hook"));

    beside_exe
        .into_iter()
        .chain(in_resources)
        .find(|path| path.exists())
        .map(|path| path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_framed_payload_and_rejects_an_unknown_agent() {
        let (agent, payload) = parse(b"claude\n{\"hook_event_name\":\"Stop\"}").unwrap();
        assert_eq!(agent, "claude");
        assert_eq!(payload["hook_event_name"], "Stop");

        assert!(parse(b"cursor\n{}").is_some());
        assert!(parse(b"opencode\n{}").is_some());
        assert!(parse(b"antigravity:Stop\n{}").is_some());
        assert!(parse(b"antigravity:Whatever\n{}").is_none());
        assert!(parse(b"antigravity\n{}").is_none());
        assert!(parse(b"someone-else\n{}").is_none());
        assert!(parse(b"codex\n{}").is_none());
        assert!(parse(b"claude\nnot json").is_none());
        assert!(parse(b"no newline").is_none());
    }
}
