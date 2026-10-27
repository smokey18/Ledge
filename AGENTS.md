# Ledge

A small macOS widget that shows when local coding agents are working, waiting for the user, or finished. Tauri 2 with a Rust backend and a vanilla TypeScript + Vite frontend. Apple silicon, macOS 11+, ad-hoc signed.

## Rules

- No code comments of any kind. Express units and constraints through names.
- Run test and build commands freely (see Commands).
- Never run a command that installs third-party software: agent CLIs, apps, `brew install`, `npm install -g`, `cargo install`, new npm packages, or copying Ledge into `/Applications`. Give the user the exact command instead.
- Commit only when asked, directly on `main`, with a conventional subject (`fix:`, `feat:`, `chore:`, `docs:`). Add a body only when the reason isn't obvious.
- User-facing text (README, release notes, prompts, labels) must be plain and short, for non-technical readers.
- Verify agent integrations against official docs or source before writing them, and tell the user what you could not test.

## Layout

- `src-tauri/src/lib.rs`: app setup, event dispatch, sweeper, tray, hook sync (`HOOKED`), login item migration.
- `src-tauri/src/detect.rs`: `AGENTS` list (id, label, executable, marker dir, optional `.app`).
- `src-tauri/src/setup.rs`: adds and removes Ledge's hook entries in agent config files, and the OpenCode plugin file.
- `src-tauri/src/antigravity.rs`, `claude.rs`, `cursor.rs`, `opencode.rs`: map agent events to `state::Incoming`.
- `src-tauri/src/opencode-plugin.js`: the OpenCode plugin Ledge installs, embedded with `include_str!`. Tested by `tests/opencode-plugin.mjs`.
- `src-tauri/src/codex.rs`: watches `~/.codex/sessions` rollouts; no hooks.
- `src-tauri/src/state.rs`: session store, states, staleness sweep, persistence.
- `src-tauri/src/ipc.rs`: Unix socket at `~/.ledge/hook.sock`; accepted agent names are whitelisted in `parse`.
- `src-tauri/src/bin/hook.rs`: `ledge-hook`, forwards stdin to the socket, prints nothing, never blocks the agent.
- `src-tauri/src/commands.rs`, `window.rs`: Tauri commands, rail and popover windows.
- `src/`: frontend. `marks.ts` holds single-path agent marks (24×24 unless set in `VIEW_BOXES`); colours in `tokens.css` and `settings.css`.

## Adding an agent

1. Confirm it runs locally and reports start, waiting-for-approval and finish through hooks or files.
2. Add `src-tauri/src/<agent>.rs` with `EVENTS`, `map()` and tests.
3. Register it in `lib.rs` (`mod`, `dispatch`, `HOOKED`), `setup.rs` (`hooks_file`), `ipc.rs` (`parse`), `detect.rs` (`AGENTS`), and add its mark and colour.
4. The detection marker must be a directory the agent creates, never one Ledge creates by writing its config.
5. Keep every per-agent list alphabetical by id: `AGENTS`, `HOOKED`, `hooks_file`, `dispatch`, `parse`, marks and CSS.

## Gotchas

- Only agents running on the Mac can be tracked. Chats in Claude or ChatGPT, Claude Design and cloud tasks can't.
- Antigravity: hooks live in `~/.gemini/config/hooks.json` (shared by CLI, app and IDE) under a top-level `"ledge"` key. Payloads have no event name, so the command is `ledge-hook antigravity:<Event>`. Project folder is `workspacePaths[0]`. Only `PreInvocation` and `Stop` are used, because `PreToolUse` output needs a `decision`. There is no waiting state.
- Cursor: hooks live in `~/.cursor/hooks.json` as `{"version": 1, "hooks": {event: [entry]}}`, with entries directly in each event's list. Session id is `conversation_id`, project folder is `workspace_roots[0]`. Permission hooks (`preToolUse`, `beforeShellExecution`, …) block on empty output, so Ledge uses only `beforeSubmitPrompt`, `postToolUse` and `stop`. There is no waiting state. Confirmed working with the Cursor CLI (`agent`, 2026.10.22); the app reads the same file.
- OpenCode: no shell hooks. Ledge writes `~/.config/opencode/plugins/ledge.js`, which default-exports `{ id, server, setup }`: OpenCode 1.x calls `server` (events in `event.properties`), OpenCode 2.x calls `setup` (subscribes via `context.event.subscribe`, events in `event.data`, folder in `event.location.directory`). Both translate events in the plugin and send `{state, sessionID, directory, title}` to Ledge's socket as `opencode`, keeping messages far under the 64 KB limit. Subagent (child) sessions are skipped; placeholder titles (`New session…`) are ignored. 2.x questions are `form.*` events, and Esc is `session.execution.interrupted` (done).
- Hook timeouts are in seconds for Antigravity, Claude Code and Cursor.
- Config files with comments fail to parse; Ledge then leaves them untouched.
- Open at login is a login item (`MacosLauncher::AppleScript`). Release builds replace any old LaunchAgent on launch; dev builds skip this.
- Keep `@tauri-apps/api`, `@tauri-apps/cli` and the Rust `tauri` crate on the same major.minor.

## Commands

```sh
npm run hook:dev                        # once on a fresh checkout, before cargo
cd src-tauri && cargo fmt && cargo clippy --all-targets && cargo test
npm run build
python3 tests/install.py
node tests/opencode-plugin.mjs
npm run tauri build -- --bundles app    # the .dmg step fails from a background shell
```

For manual testing, have the user quit Ledge from the menu bar and run `npm run tauri dev` (fast, rebuilds on change). Don't toggle Open at login in dev mode. Only build the release app to install a finished version.

To install a release build, the user quits Ledge from the menu bar and runs:

```sh
rm -rf /Applications/Ledge.app && cp -R src-tauri/target/release/bundle/macos/Ledge.app /Applications/ && open /Applications/Ledge.app
```

## Releasing

A `chore: release X.Y.Z` commit bumps the version in `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` and `src-tauri/tauri.conf.json`. Pushing a `vX.Y.Z` tag runs `.github/workflows/release.yml`, which creates a draft release.
