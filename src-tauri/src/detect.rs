use crate::home;
use std::path::{Path, PathBuf};

pub struct Agent {
    pub id: &'static str,
    pub label: &'static str,
    pub executable: &'static str,
    pub marker: &'static str,
    pub app: Option<&'static str>,
}

pub const AGENTS: &[Agent] = &[
    Agent {
        id: "antigravity",
        label: "Antigravity",
        executable: "agy",
        marker: ".gemini/antigravity-cli",
        app: Some("Antigravity.app"),
    },
    Agent {
        id: "claude",
        label: "Claude Code",
        executable: "claude",
        marker: ".claude/projects",
        app: Some("Claude.app"),
    },
    Agent {
        id: "codex",
        label: "Codex",
        executable: "codex",
        marker: ".codex/sessions",
        app: None,
    },
    Agent {
        id: "cursor",
        label: "Cursor",
        executable: "cursor-agent",
        marker: ".cursor/projects",
        app: Some("Cursor.app"),
    },
    Agent {
        id: "opencode",
        label: "OpenCode",
        executable: "opencode",
        marker: ".local/share/opencode",
        app: Some("OpenCode.app"),
    },
];

const EXTRA_BIN_DIRS: &[&str] = &[
    "/opt/homebrew/bin",
    "/usr/local/bin",
    ".local/bin",
    ".bun/bin",
    ".cargo/bin",
    ".volta/bin",
];

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let extra = EXTRA_BIN_DIRS.iter().map(|dir| {
        if dir.starts_with('/') {
            PathBuf::from(dir)
        } else {
            home().join(dir)
        }
    });

    std::env::split_paths(&path)
        .chain(extra)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

fn app_installed(bundle: &str) -> bool {
    [PathBuf::from("/Applications"), home().join("Applications")]
        .iter()
        .any(|dir| dir.join(bundle).is_dir())
}

fn find(agent: &str) -> Option<&'static Agent> {
    AGENTS.iter().find(|candidate| candidate.id == agent)
}

pub fn is_available(agent: &str) -> bool {
    find(agent).is_some_and(|agent| {
        on_path(agent.executable).is_some()
            || home().join(agent.marker).is_dir()
            || agent.app.is_some_and(app_installed)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_keys_off_a_directory_the_agent_makes_not_one_ledge_makes() {
        let scratch = std::env::temp_dir().join(format!("ledge-detect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);

        let codex = find("codex").unwrap();
        assert_ne!(codex.marker, ".codex");
        let antigravity = find("antigravity").unwrap();
        assert!(!".gemini/config/hooks.json".starts_with(antigravity.marker));
        assert_ne!(find("cursor").unwrap().marker, ".cursor");
        assert!(!".config/opencode/plugins/ledge.js".starts_with(find("opencode").unwrap().marker));

        std::fs::create_dir_all(scratch.join(".codex")).unwrap();
        assert!(!scratch.join(codex.marker).is_dir());

        std::fs::create_dir_all(scratch.join(codex.marker)).unwrap();
        assert!(scratch.join(codex.marker).is_dir());

        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn agents_are_listed_alphabetically() {
        let ids: Vec<_> = AGENTS.iter().map(|agent| agent.id).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);
    }

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

    #[test]
    fn an_installed_app_bundle_counts_and_a_missing_one_does_not() {
        assert!(app_installed("Utilities"));
        assert!(!app_installed("Definitely Not Installed.app"));
    }
}
