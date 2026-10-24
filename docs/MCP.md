# AI control: Soundesti's built-in MCP server

Soundesti includes a [Model Context Protocol](https://modelcontextprotocol.io) server so AI
assistants (Claude Code, Claude Desktop, Cursor, any MCP client) can drive the app: every
menu item, shortcut and editor tool, plus direct editing of channels, notes, instruments,
mixer, patterns, transport, files, and screenshots of the UI.

## Turn it on
- In the app: **OPTIONS › AI control (MCP server)** (remembered between runs), or
- launch with `soundesti.exe --mcp`.

The hint bar shows `AI :47821` while it runs. Port override: env `SOUNDESTI_MCP_PORT`.

## Connect a client
Easiest: **HELP › AI assistant setup** (or OPTIONS › Set up AI assistants…) shows the exact
steps for each tool with Copy buttons and a **Set it up for me** button (runs the official
CLI, or merges the entry into the tool's config file and keeps a `.bak` of the old one).

Formats below were checked against each tool's official documentation on 2026-10-24.
Server URL: `http://127.0.0.1:47821/mcp` (Streamable HTTP, no authentication).

| Tool | Setup |
|---|---|
| **Claude Code** | `claude mcp add --transport http --scope user soundesti http://127.0.0.1:47821/mcp` — or per project in `.mcp.json`: `{"mcpServers":{"soundesti":{"type":"http","url":"http://127.0.0.1:47821/mcp"}}}` |
| **Codex** | `codex mcp add soundesti --url http://127.0.0.1:47821/mcp` — or in `%USERPROFILE%\.codex\config.toml`: `[mcp_servers.soundesti]` / `url = "http://127.0.0.1:47821/mcp"` |
| **Cursor** | `%USERPROFILE%\.cursor\mcp.json` (or `.cursor\mcp.json` per project): `{"mcpServers":{"soundesti":{"url":"http://127.0.0.1:47821/mcp"}}}` |
| **VS Code (Copilot agent mode)** | User `mcp.json` (Command Palette: *MCP: Open User Configuration*) or `.vscode\mcp.json`: `{"servers":{"soundesti":{"type":"http","url":"http://127.0.0.1:47821/mcp"}}}` — note the key is `servers` |
| **Windsurf** | `mcp_config.json` (Cascade › MCP servers › View raw config; `%APPDATA%\devin\` or `%USERPROFILE%\.codeium\windsurf\`): `{"mcpServers":{"soundesti":{"serverUrl":"http://127.0.0.1:47821/mcp"}}}` — Windsurf uses `serverUrl` |
| **Gemini CLI** | `gemini mcp add --scope user --transport http soundesti http://127.0.0.1:47821/mcp` — or `%USERPROFILE%\.gemini\settings.json`: `{"mcpServers":{"soundesti":{"httpUrl":"http://127.0.0.1:47821/mcp"}}}` — `httpUrl`, because `url` means SSE in Gemini |
| **Claude Desktop** | Its config only launches programs, and custom connectors run in Anthropic's cloud (can't reach your PC). Use Soundesti's built-in bridge in `%APPDATA%\Claude\claude_desktop_config.json`: `{"mcpServers":{"soundesti":{"command":"C:\\Users\\<you>\\AppData\\Local\\Programs\\Soundesti\\soundesti.exe","args":["--mcp-stdio"]}}}` |
| **Anything else** | Streamable-HTTP clients: the URL above. stdio-only clients: run `soundesti.exe --mcp-stdio`. |

### The stdio bridge (`soundesti.exe --mcp-stdio`)
Reads newline-delimited JSON-RPC on stdin, forwards it to the running app, and writes
replies to stdout — no Node.js or third-party bridge needed. If Soundesti isn't running (or
AI control is off) it starts Soundesti with `--mcp` and waits for it. It never inherits its
pipes to the app, so the client sees the bridge exit cleanly.

Quick manual test: `scripts\mcp-call.ps1 get_state`.

## Tools
| Tool | What it does |
|---|---|
| `get_state` | Transport, tempo, B:S:T position, patterns, channels, mixer, windows, focus, file |
| `list_commands` | All 194 commands (id, label, category, FL shortcuts) |
| `run_command` | Run any command by id; `window` targets an editor (e.g. `piano_roll`) |
| `press_shortcut` | Press an FL-style shortcut in a window, e.g. `Ctrl+B` in `piano_roll` |
| `transport` | play / stop / pause / play_stop / seek |
| `set_tempo` | BPM |
| `add_channel`, `remove_channel`, `set_channel` | Channel Rack rows: name, volume, pan, mute, mixer track |
| `set_steps` | Step sequencer, e.g. `"x...x...x...x..."` |
| `add_notes`, `get_notes`, `clear_notes` | Piano roll notes (ticks, or `unit:"steps"`) |
| `get_instrument_params`, `set_instrument_param` | Instrument knobs (e.g. `filter.cutoff`) |
| `set_mixer_track` | Volume, pan, mute, name, solo |
| `pattern` | select / new / rename |
| `set_window` | Open/close any window |
| `save_project`, `open_project`, `new_project` | Files (`.soundesti` only; refuses to discard unsaved work unless told) |
| `export_wav` | Render the pattern to WAV (runs in the background) |
| `screenshot` | PNG of the window (≤1600 px wide) |

Every edit goes through the normal undo system (`run_command edit.undo_toggle` undoes it)
and the same code paths as mouse/keyboard input. Unimplemented commands return an error
saying so instead of pretending.

## Security
- Listens on **127.0.0.1 only** — not reachable from other computers.
- Rejects requests whose `Host` isn't local or that carry a non-local `Origin`, so web pages
  you visit can't control Soundesti (DNS-rebinding/CSRF protection).
- **Off by default.** File tools only accept `.soundesti` / `.wav` paths.
- There is no authentication token: any program running as your user can use it while it's on.

## Verified (2026-10-24)
Unit tests (protocol, errors, host/origin checks, base64, real HTTP round trip) plus a live
session against the running app: initialize → tools/list (23 tools) → build a beat (tempo,
steps, new channel, bassline, filter cutoff, open mixer, Ctrl+A/Ctrl+B in the piano roll,
play) → get_state confirmed playback and 10 notes → screenshot → undo restored 5 notes →
a request with `Origin: https://evil.example` got 403. Stdio bridge: with Soundesti closed, `--mcp-stdio` launched it, answered initialize / tools/list / tools/call, and exited cleanly when stdin closed.
