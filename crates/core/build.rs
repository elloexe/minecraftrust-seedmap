//! Embed the worldgen bundle `cargo xtask extract` writes.
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let bundle = std::env::var("SEEDMAP_BUNDLE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest.join("../../data/worldgen-26.3.bin"));
    println!("cargo:rerun-if-env-changed=SEEDMAP_BUNDLE");
    println!("cargo:rerun-if-changed={}", bundle.display());
    if !bundle.exists() {
        panic!(
            "\n\nThe worldgen data bundle is missing: {}\n\
             Build it from the official 26.3 server jar first (needs curl and Java 25):\n\n    \
             cargo xtask extract\n\n",
            bundle.display()
        );
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("worldgen.bin");
    std::fs::copy(&bundle, out).unwrap();
}
