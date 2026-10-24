#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ai_setup;
mod app;
mod browser;
mod channel_rack;
mod files;
mod icons;
mod input;
mod mcp;
mod mcp_tools;
mod mixer;
mod panels;
mod params_ui;
mod piano_roll;
mod playlist;
mod plugins;
mod theme;
mod updates;
mod widgets;

fn main() -> eframe::Result {
    // `--mcp-stdio`: run only the MCP stdio bridge (no window); stdout belongs to the protocol.
    // `--scan-plugin <format> <path>`: inspect one plugin file for the scanner (child process).
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == soundesti_plugin_host::scan::SCAN_FLAG) {
        std::process::exit(soundesti_plugin_host::scan::child_main(&args[i + 1..]));
    }
    if std::env::args().any(|a| a == "--mcp-stdio") {
        let port = std::env::var("SOUNDESTI_MCP_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(mcp::DEFAULT_PORT);
        std::process::exit(mcp::run_stdio_bridge(port));
    }
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Soundesti")
            .with_app_id("soundesti")
            .with_inner_size([1280.0, 760.0])
            .with_maximized(true)
            .with_min_inner_size([960.0, 600.0])
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icons/soundesti-256.png"))
                    .expect("bundled icon is a valid PNG"),
            ),
        ..Default::default()
    };
    // `--mcp` turns on AI control at startup; any existing file path argument is opened
    // (the .soundesti file association passes the project path).
    let args: Vec<std::path::PathBuf> = std::env::args_os().skip(1).map(std::path::PathBuf::from).collect();
    let force_mcp = args.iter().any(|a| a.as_os_str() == "--mcp");
    let open_file = args.into_iter().find(|p| p.exists());
    let result = eframe::run_native(
        "Soundesti",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, open_file, force_mcp)))),
    );
    if soundesti_plugin_host::any_loaded() {
        soundesti_plugin_host::exit_skipping_plugin_shutdown(if result.is_ok() { 0 } else { 1 });
    }
    result
}
