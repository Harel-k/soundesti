//! Embeds the files to install. `scripts/package.ps1` sets `SOUNDESTI_PAYLOAD_DIR` to a
//! staged release folder; without it the installer is a "development build" with an
//! empty payload (it can still uninstall and check for updates).

#[path = "src/payload.rs"]
mod payload;

fn main() {
    println!("cargo:rerun-if-env-changed=SOUNDESTI_PAYLOAD_DIR");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("payload.bin");
    let bytes = match std::env::var("SOUNDESTI_PAYLOAD_DIR") {
        Ok(dir) if !dir.is_empty() => {
            let dir = std::path::PathBuf::from(dir);
            rerun_for(&dir);
            payload::pack_dir(&dir).expect("pack payload directory")
        }
        _ => Vec::new(),
    };
    std::fs::write(out, bytes).unwrap();

    println!("cargo:rerun-if-changed=../../assets/icons/soundesti.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icons/soundesti.ico");
        res.set("ProductName", "Soundesti Installer Wizard");
        res.set("FileDescription", "Soundesti Installer Wizard");
        res.compile().expect("compile Windows resources (needs the Windows SDK rc.exe)");
    }
}

fn rerun_for(dir: &std::path::Path) {
    println!("cargo:rerun-if-changed={}", dir.display());
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.path().is_dir() {
                rerun_for(&e.path());
            } else {
                println!("cargo:rerun-if-changed={}", e.path().display());
            }
        }
    }
}
