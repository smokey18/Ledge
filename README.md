# Ledge

See what your coding agents are doing without switching windows.

Ledge is a small always-on-top widget for macOS. It shows which project each
agent is on, how long it has been going, and whether it is busy, waiting on you,
or done. Click a session to open that project in Finder.

Works with Claude Code and Codex.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/smokey18/Ledge/main/scripts/install.sh | sh
```

macOS 11 or newer. No setup — start an agent as usual and it shows up.

<details>
<summary>Other ways to install</summary>

Grab the `.dmg` from the [latest release](../../releases/latest) and drag Ledge
into Applications. The first launch needs **System Settings → Privacy &
Security → Open Anyway**.

Or build it: `npm install && npm run tauri build`.

To check a download is genuine:

```sh
gh attestation verify Ledge_*.dmg --repo smokey18/Ledge
```

</details>

## Notes

- **Open at login** lives in Settings.
- Cloud or remote agent runs don't appear.
- Start Ledge before your agent.
- Everything stays on your Mac.

## Development

Requires Rust and Node.js 20 or newer.

```sh
npm install
npm run tauri dev
npm run tauri build
cd src-tauri && cargo test
```

## License

MIT — see [LICENSE](LICENSE).

The Claude and OpenAI marks used to identify agents are trademarks of their
respective owners and are not covered by that licence.
