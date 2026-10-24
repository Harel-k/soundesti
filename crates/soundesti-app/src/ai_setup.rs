//! "AI assistant setup" window: exact, copy-paste instructions for connecting AI tools to
//! the built-in MCP server, plus one-click setup (runs the official CLI, or safely merges
//! an entry into the tool's config file with a backup).
//!
//! Formats were checked against each tool's official docs on 2026-10-24 (see docs/MCP.md).

use std::path::PathBuf;
use std::sync::mpsc;

use egui::{RichText, Ui};
use serde_json::{Value, json};

use crate::app::App;
use crate::theme;

pub const SERVER_NAME: &str = "soundesti";

pub fn url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/mcp")
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Client {
    ClaudeCode,
    Codex,
    Cursor,
    VsCode,
    Windsurf,
    GeminiCli,
    ClaudeDesktop,
    Other,
}

impl Client {
    pub const ALL: [Client; 8] = [
        Client::ClaudeCode,
        Client::Codex,
        Client::Cursor,
        Client::VsCode,
        Client::Windsurf,
        Client::GeminiCli,
        Client::ClaudeDesktop,
        Client::Other,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Client::ClaudeCode => "Claude Code",
            Client::Codex => "Codex",
            Client::Cursor => "Cursor",
            Client::VsCode => "VS Code (Copilot)",
            Client::Windsurf => "Windsurf",
            Client::GeminiCli => "Gemini CLI",
            Client::ClaudeDesktop => "Claude Desktop",
            Client::Other => "Other AI tools",
        }
    }
}

/// How one-click setup is done for a client.
pub enum Setup {
    /// Run a CLI: program name (looked up on PATH, .cmd shims allowed) and arguments.
    Cli {
        program: &'static str,
        args: Vec<String>,
    },
    /// Merge `entry` under `root_key.SERVER_NAME` in a JSON config file.
    JsonFile {
        path: PathBuf,
        root_key: &'static str,
        entry: Value,
    },
    None,
}

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default()
}

fn appdata() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default()
}

/// The stdio bridge command for clients that can only launch programs.
pub fn bridge_command() -> (String, Vec<String>) {
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "soundesti.exe".into());
    (exe, vec!["--mcp-stdio".into()])
}

/// Windsurf moved its config; prefer whichever file already exists.
fn windsurf_config() -> PathBuf {
    let new = appdata().join("devin").join("mcp_config.json");
    let old = home().join(".codeium").join("windsurf").join("mcp_config.json");
    if old.exists() && !new.exists() {
        old
    } else if new.exists() {
        new
    } else {
        old
    }
}

pub fn setup_for(client: Client, port: u16) -> Setup {
    let u = url(port);
    match client {
        Client::ClaudeCode => Setup::Cli {
            program: "claude",
            args: ["mcp", "add", "--transport", "http", "--scope", "user", SERVER_NAME, &u].map(String::from).to_vec(),
        },
        Client::Codex => {
            Setup::Cli { program: "codex", args: ["mcp", "add", SERVER_NAME, "--url", &u].map(String::from).to_vec() }
        }
        Client::GeminiCli => Setup::Cli {
            program: "gemini",
            args: ["mcp", "add", "--scope", "user", "--transport", "http", SERVER_NAME, &u].map(String::from).to_vec(),
        },
        Client::Cursor => Setup::JsonFile {
            path: home().join(".cursor").join("mcp.json"),
            root_key: "mcpServers",
            entry: json!({"url": u}),
        },
        Client::VsCode => Setup::JsonFile {
            path: appdata().join("Code").join("User").join("mcp.json"),
            root_key: "servers",
            entry: json!({"type": "http", "url": u}),
        },
        Client::Windsurf => {
            Setup::JsonFile { path: windsurf_config(), root_key: "mcpServers", entry: json!({"serverUrl": u}) }
        }
        Client::ClaudeDesktop => {
            let (cmd, args) = bridge_command();
            Setup::JsonFile {
                path: appdata().join("Claude").join("claude_desktop_config.json"),
                root_key: "mcpServers",
                entry: json!({"command": cmd, "args": args}),
            }
        }
        Client::Other => Setup::None,
    }
}

/// Human instructions for a client: (steps, copyable snippets with titles).
pub fn instructions(client: Client, port: u16) -> (Vec<String>, Vec<(String, String)>) {
    let u = url(port);
    let json_snippet =
        |root: &str, entry: Value| serde_json::to_string_pretty(&json!({root: {SERVER_NAME: entry}})).unwrap();
    match client {
        Client::ClaudeCode => (
            vec![
                "Run this once in a terminal (available in every project):".into(),
                "Then start Claude Code and check it with /mcp.".into(),
                "Per-project instead: put the JSON in .mcp.json at the project root.".into(),
            ],
            vec![
                ("Command".into(), format!("claude mcp add --transport http --scope user {SERVER_NAME} {u}")),
                (".mcp.json (per project)".into(), json_snippet("mcpServers", json!({"type": "http", "url": u}))),
            ],
        ),
        Client::Codex => (
            vec![
                "Run this once in a terminal:".into(),
                "Or add the TOML to ~/.codex/config.toml (%USERPROFILE%\\.codex\\config.toml).".into(),
                "Restart Codex; `codex mcp list` should show soundesti.".into(),
            ],
            vec![
                ("Command".into(), format!("codex mcp add {SERVER_NAME} --url {u}")),
                ("config.toml".into(), format!("[mcp_servers.{SERVER_NAME}]\nurl = \"{u}\"")),
            ],
        ),
        Client::Cursor => (
            vec![
                "Add this to %USERPROFILE%\\.cursor\\mcp.json (all projects) or .cursor\\mcp.json (one project).".into(),
                "Then open Cursor Settings > MCP and make sure soundesti is enabled.".into(),
            ],
            vec![("mcp.json".into(), json_snippet("mcpServers", json!({"url": u})))],
        ),
        Client::VsCode => (
            vec![
                "Add this to your user mcp.json (Command Palette: \"MCP: Open User Configuration\"), or to .vscode\\mcp.json in a workspace.".into(),
                "Note the top-level key is \"servers\" in VS Code.".into(),
                "Use it from Copilot Chat in Agent mode.".into(),
            ],
            vec![("mcp.json".into(), json_snippet("servers", json!({"type": "http", "url": u})))],
        ),
        Client::Windsurf => (
            vec![
                format!("Add this to {} (Cascade > MCP servers > View raw config).", windsurf_config().display()),
                "Windsurf uses \"serverUrl\" for HTTP servers. Then press Refresh in the MCP panel.".into(),
            ],
            vec![("mcp_config.json".into(), json_snippet("mcpServers", json!({"serverUrl": u})))],
        ),
        Client::GeminiCli => (
            vec![
                "Run this once in a terminal:".into(),
                "Or add the JSON to %USERPROFILE%\\.gemini\\settings.json. Gemini uses \"httpUrl\" (\"url\" would mean SSE).".into(),
                "Check it inside Gemini CLI with /mcp.".into(),
            ],
            vec![
                ("Command".into(), format!("gemini mcp add --scope user --transport http {SERVER_NAME} {u}")),
                ("settings.json".into(), json_snippet("mcpServers", json!({"httpUrl": u}))),
            ],
        ),
        Client::ClaudeDesktop => {
            let (cmd, args) = bridge_command();
            (
                vec![
                    "Claude Desktop can only launch programs, so it connects through Soundesti's built-in bridge (no Node.js needed).".into(),
                    format!("Add this to {} and restart Claude Desktop.", appdata().join("Claude").join("claude_desktop_config.json").display()),
                    "The bridge starts Soundesti with AI control on if it isn't running. Custom connectors in Claude's settings can't reach your PC (they run in the cloud).".into(),
                ],
                vec![("claude_desktop_config.json".into(), json_snippet("mcpServers", json!({"command": cmd, "args": args})))],
            )
        }
        Client::Other => {
            let (cmd, args) = bridge_command();
            (
                vec![
                    "Tools that support Streamable HTTP MCP: use this URL (no authentication).".into(),
                    "Tools that only support stdio servers: run Soundesti's bridge command.".into(),
                    "Keep Soundesti open with OPTIONS > AI control turned on.".into(),
                ],
                vec![("Server URL".into(), u), ("stdio command".into(), format!("\"{cmd}\" {}", args.join(" ")))],
            )
        }
    }
}

/// Adds/replaces `root_key.soundesti` in a JSON config, keeping everything else.
/// Refuses to touch a file that isn't valid JSON (so a hand-edited config is never lost).
pub fn merge_server_entry(existing: Option<&str>, root_key: &str, entry: &Value) -> Result<String, String> {
    let mut doc: Value = match existing.map(str::trim) {
        None | Some("") => json!({}),
        Some(text) => serde_json::from_str(text)
            .map_err(|e| format!("the existing file is not valid JSON ({e}); fix it or add the snippet by hand"))?,
    };
    let obj = doc.as_object_mut().ok_or("the existing file's top level is not a JSON object")?;
    let servers = obj.entry(root_key).or_insert_with(|| json!({}));
    let servers =
        servers.as_object_mut().ok_or_else(|| format!("\"{root_key}\" in the existing file is not an object"))?;
    servers.insert(SERVER_NAME.into(), entry.clone());
    Ok(serde_json::to_string_pretty(&doc).unwrap() + "\n")
}

/// Writes the merged config, keeping a `.bak` of the previous file.
pub fn apply_json_setup(path: &std::path::Path, root_key: &str, entry: &Value) -> Result<String, String> {
    let existing = match std::fs::read_to_string(path) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    let merged = merge_server_entry(existing.as_deref(), root_key, entry)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let mut note = String::new();
    if let Some(old) = &existing {
        let bak = path.with_extension("json.bak");
        std::fs::write(&bak, old).map_err(|e| format!("cannot write backup {}: {e}", bak.display()))?;
        note = format!(" (previous file saved as {})", bak.display());
    }
    std::fs::write(path, merged).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(format!("Added \"{SERVER_NAME}\" to {}{note}. Restart the app to load it.", path.display()))
}

/// Runs a CLI through cmd.exe so npm `.cmd` shims (claude, codex, gemini) are found.
pub fn run_cli(program: &str, args: &[String]) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let found = std::process::Command::new("where.exe")
        .arg(program)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !found {
        return Err(format!(
            "`{program}` was not found on PATH. Install it first, or copy the command and run it yourself."
        ));
    }
    let out = std::process::Command::new("cmd")
        .arg("/C")
        .arg(program)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("could not run {program}: {e}"))?;
    let text =
        format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)).trim().to_string();
    if out.status.success() {
        Ok(if text.is_empty() { format!("{program}: done.") } else { text })
    } else {
        Err(if text.is_empty() { format!("{program} exited with {}", out.status) } else { text })
    }
}

/// Per-window state.
pub struct AiSetupState {
    pub client: Client,
    pub result: Option<Result<String, String>>,
    pub running: Option<mpsc::Receiver<Result<String, String>>>,
}

impl Default for AiSetupState {
    fn default() -> Self {
        Self { client: Client::ClaudeCode, result: None, running: None }
    }
}

impl App {
    pub fn ai_setup_ui(&mut self, ui: &mut Ui) {
        let port = self.mcp.as_ref().map(|m| m.port).unwrap_or(crate::mcp::DEFAULT_PORT);
        ui.horizontal(|ui| {
            let mut on = self.mcp_enabled;
            if ui.checkbox(&mut on, RichText::new("AI control (MCP server)").strong()).changed() {
                let ctx = ui.ctx().clone();
                self.set_mcp(&ctx, on);
            }
            if self.mcp.is_some() {
                ui.label(RichText::new(format!("running at {}", url(port))).color(theme::ACCENT).monospace());
            } else {
                ui.label(RichText::new("off — turn it on, then connect your AI tool").color(theme::WARN));
            }
        });
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            for c in Client::ALL {
                if ui.selectable_label(self.ai_setup.client == c, c.name()).clicked() {
                    self.ai_setup.client = c;
                    self.ai_setup.result = None;
                }
            }
        });
        ui.separator();
        let client = self.ai_setup.client;
        let (steps, snippets) = instructions(client, port);
        for (i, s) in steps.iter().enumerate() {
            ui.label(format!("{}. {s}", i + 1));
        }
        ui.add_space(6.0);
        for (title, snippet) in &snippets {
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).strong());
                if ui.small_button("Copy").clicked() {
                    ui.ctx().copy_text(snippet.clone());
                    self.set_status(format!("{title} for {} copied to the clipboard.", client.name()));
                }
            });
            egui::Frame::new().fill(theme::PANEL_DARK).corner_radius(4).inner_margin(8).show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(snippet).monospace().size(12.0)).selectable(true));
            });
            ui.add_space(4.0);
        }

        let setup = setup_for(client, port);
        let action = match &setup {
            Setup::Cli { program, .. } => Some(format!("Set it up for me (runs `{program} mcp add`)")),
            Setup::JsonFile { path, .. } => Some(format!("Set it up for me (adds the entry to {})", path.display())),
            Setup::None => None,
        };
        if let Some(label) = action {
            let busy = self.ai_setup.running.is_some();
            if ui.add_enabled(!busy, egui::Button::new(RichText::new(label).strong())).clicked() {
                let (tx, rx) = mpsc::channel();
                std::thread::spawn(move || {
                    let r = match setup {
                        Setup::Cli { program, args } => run_cli(program, &args),
                        Setup::JsonFile { path, root_key, entry } => apply_json_setup(&path, root_key, &entry),
                        Setup::None => Ok(String::new()),
                    };
                    let _ = tx.send(r);
                });
                self.ai_setup.running = Some(rx);
                self.ai_setup.result = None;
            }
        }
        if let Some(rx) = &self.ai_setup.running {
            ui.label("Working…");
            if let Ok(r) = rx.try_recv() {
                self.ai_setup.result = Some(r);
                self.ai_setup.running = None;
            }
            ui.ctx().request_repaint();
        }
        match &self.ai_setup.result {
            Some(Ok(m)) => {
                ui.horizontal(|ui| {
                    crate::icons::inline(ui, crate::icons::Icon::Check, theme::ACCENT);
                    ui.colored_label(theme::ACCENT, m);
                });
            }
            Some(Err(e)) => {
                ui.colored_label(theme::WARN, e);
            }
            None => {}
        }
        ui.add_space(6.0);
        ui.label(RichText::new("The server only accepts connections from this PC. Every AI edit can be undone with Ctrl+Z. Details: docs/MCP.md.").small().color(theme::TEXT_DIM));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_into_empty_and_existing_configs() {
        let e = json!({"url": "http://127.0.0.1:47821/mcp"});
        let out: Value = serde_json::from_str(&merge_server_entry(None, "mcpServers", &e).unwrap()).unwrap();
        assert_eq!(out["mcpServers"]["soundesti"]["url"], "http://127.0.0.1:47821/mcp");

        // Other servers and settings are preserved; an old soundesti entry is replaced.
        let existing = r#"{"theme":"dark","mcpServers":{"other":{"command":"x"},"soundesti":{"url":"old"}}}"#;
        let out: Value = serde_json::from_str(&merge_server_entry(Some(existing), "mcpServers", &e).unwrap()).unwrap();
        assert_eq!(out["theme"], "dark");
        assert_eq!(out["mcpServers"]["other"]["command"], "x");
        assert_eq!(out["mcpServers"]["soundesti"]["url"], "http://127.0.0.1:47821/mcp");

        // VS Code's different root key.
        let out: Value =
            serde_json::from_str(&merge_server_entry(Some("{}"), "servers", &json!({"type":"http"})).unwrap()).unwrap();
        assert_eq!(out["servers"]["soundesti"]["type"], "http");
    }

    #[test]
    fn invalid_configs_are_never_overwritten() {
        let e = json!({});
        assert!(merge_server_entry(Some("{ broken"), "mcpServers", &e).is_err());
        assert!(merge_server_entry(Some("[1,2]"), "mcpServers", &e).is_err());
        assert!(merge_server_entry(Some(r#"{"mcpServers": 5}"#), "mcpServers", &e).is_err());
    }

    #[test]
    fn apply_writes_backup() {
        let dir = std::env::temp_dir().join(format!("sd-aisetup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("cfg").join("mcp.json");
        apply_json_setup(&path, "mcpServers", &json!({"url": "u"})).unwrap();
        assert!(!path.with_extension("json.bak").exists(), "no backup for a new file");
        apply_json_setup(&path, "mcpServers", &json!({"url": "u2"})).unwrap();
        let bak: Value =
            serde_json::from_str(&std::fs::read_to_string(path.with_extension("json.bak")).unwrap()).unwrap();
        assert_eq!(bak["mcpServers"]["soundesti"]["url"], "u");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn every_client_has_instructions_with_the_url() {
        for c in Client::ALL {
            let (steps, snippets) = instructions(c, 47821);
            assert!(!steps.is_empty() && !snippets.is_empty(), "{c:?}");
            let all: String = snippets.iter().map(|s| s.1.clone()).collect();
            assert!(all.contains("47821") || all.contains("--mcp-stdio"), "{c:?} snippet lacks the server address");
        }
        // Field names that differ per client (checked against official docs).
        let field = |c| instructions(c, 1).1.iter().map(|s| s.1.clone()).collect::<String>();
        assert!(field(Client::Windsurf).contains("serverUrl"));
        assert!(field(Client::GeminiCli).contains("httpUrl"));
        assert!(field(Client::VsCode).contains("\"servers\""));
        assert!(field(Client::Codex).contains("[mcp_servers.soundesti]"));
    }
}
