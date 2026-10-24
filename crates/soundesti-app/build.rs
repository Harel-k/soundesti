//! Embeds the Soundesti icon and version info into soundesti.exe (Windows resources).

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icons/soundesti.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icons/soundesti.ico");
        res.set("ProductName", "Soundesti");
        res.set("FileDescription", "Soundesti digital audio workstation");
        res.set("LegalCopyright", "GPL-3.0-or-later");
        res.compile().expect("compile Windows resources (needs the Windows SDK rc.exe)");
    }
}
