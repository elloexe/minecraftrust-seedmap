//! The Windows executable's icon and version information.
fn main() {
    println!("cargo:rerun-if-changed=../../assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icon.ico");
        res.set("ProductName", "Minecraft Rust Seedmap");
        res.set(
            "FileDescription",
            "Seed map and structure finder for Minecraft Rust 26.3",
        );
        res.set("LegalCopyright", "MIT licence");
        if let Err(e) = res.compile() {
            // A missing resource compiler only costs the icon.
            println!("cargo:warning=no Windows resources: {e}");
        }
    }
}
