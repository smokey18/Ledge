use std::path::{Path, PathBuf};

pub struct Agent {
    pub id: &'static str,
    pub label: &'static str,
    pub executable: &'static str,
    pub data_dir: &'static str,
    pub uses_notify: bool,
}

pub const AGENTS: &[Agent] = &[
    Agent {
        id: "claude",
        label: "Claude Code",
        executable: "claude",
        data_dir: ".claude",
        uses_notify: false,
    },
    Agent {
        id: "codex",
        label: "Codex",
        executable: "codex",
        data_dir: ".codex",
        uses_notify: true,
    },
];

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

fn find(agent: &str) -> Option<&'static Agent> {
    AGENTS.iter().find(|candidate| candidate.id == agent)
}

/// A GUI-launched Ledge inherits a thin PATH, so the data directory is often
/// the only signal available.
pub fn is_available(agent: &str) -> bool {
    find(agent).is_some_and(|agent| {
        on_path(agent.executable).is_some() || home().join(agent.data_dir).is_dir()
    })
}

pub fn uses_notify(agent: &str) -> bool {
    find(agent).is_some_and(|agent| agent.uses_notify)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_real_executable_and_rejects_a_made_up_one() {
        assert!(on_path("sh").is_some());
        assert!(on_path("definitely-not-a-real-binary-xyz").is_none());
    }

    #[test]
    fn a_directory_on_path_is_not_mistaken_for_an_executable() {
        assert!(!is_executable(Path::new("/usr")));
        assert!(!is_executable(Path::new("/nonexistent/xyz")));
    }

    #[test]
    fn unknown_agents_are_never_reported_as_available() {
        assert!(!is_available("emacs-doctor"));
    }
}
