# Ledge

See what your local coding agents are doing without switching windows.

Ledge is an always-on-top macOS widget that shows each agent session's project,
elapsed time, and current state. Click a session to open its project in Finder.

## Install

[Download the latest release](../../releases/latest), open the `.dmg`, and drag
Ledge into Applications. Requires macOS 11 or newer.

Ledge is not notarized yet, so the first launch requires right-clicking the app
and choosing **Open**.

## Agent setup

| Agent | Setup |
| --- | --- |
| Claude Code | Open Ledge Settings and click **Connect** once. |
| Codex | None. Sessions are detected automatically. |

**Open at login** is optional.

## How it works

Agent activity is detected locally through session logs or a bundled integration.

Only session IDs, event state, timestamps, and project paths are retained. Chat
content is not stored, and nothing leaves your Mac.

## Limits

- Remote or cloud-only agent runs are not shown.
- Sessions that require an integration must start after it is connected.

## Development

Requires Rust and Node.js 20 or newer.

```sh
npm install
npm run tauri dev
npm run tauri build
cd src-tauri && cargo test
```

Development uses a debug integration build. Production builds use an optimized
build and produce the macOS app and DMG.
