//! Spawned once per hook event, so it stays std-only and does no parsing: it
//! forwards stdin and exits. When Ledge is not running it exits silently, since
//! a stopped widget must never block an agent.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_millis(250);
const MAX_PAYLOAD: u64 = 64 * 1024;

fn socket_path() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
        .join(".ledge")
        .join("hook.sock")
}

fn forward(agent: &str, payload: &[u8]) -> Option<()> {
    let mut stream = UnixStream::connect(socket_path()).ok()?;
    stream.set_write_timeout(Some(TIMEOUT)).ok()?;
    stream.write_all(format!("{agent}\n").as_bytes()).ok()?;
    stream.write_all(payload).ok()?;
    stream.flush().ok()
}

/// Codex's `notify` is a single slot, so the command Ledge displaced is passed
/// after `--` and re-run here.
fn chain(payload: &[u8]) {
    let mut args = std::env::args().skip_while(|arg| arg != "--").skip(1);
    let Some(program) = args.next() else { return };

    let Ok(mut child) = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .spawn()
    else {
        return;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(payload);
    }
    let _ = child.wait();
}

fn main() {
    let agent = std::env::args().nth(1).unwrap_or_default();
    let agent = agent.trim();
    if agent.is_empty() || agent == "--" {
        return;
    }

    let mut payload = Vec::new();
    let _ = std::io::stdin().take(MAX_PAYLOAD).read_to_end(&mut payload);

    let _ = forward(agent, &payload);
    chain(&payload);
}
