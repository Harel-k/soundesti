#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod channel_rack;
mod files;
mod input;
mod mcp;
mod mcp_tools;
mod mixer;
mod panels;
mod piano_roll;
mod theme;
mod updates;
mod widgets;

fn main() -> eframe::Result {
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
    eframe::run_native("Soundesti", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, open_file, force_mcp)))))
}
