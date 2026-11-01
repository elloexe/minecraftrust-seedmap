//! `cargo xtask extract`: build the worldgen data bundle the app embeds.
//!
//! The bundle is made from the official Minecraft: Java Edition 26.3 server
//! jar, downloaded from Mojang, exactly like Minecraft Rust prepares its own
//! world generator data. It is never committed: CI and anyone building from
//! source run this step first. It needs `curl` and a Java 25 runtime (the
//! data generator writes the multi-noise biome parameter tables).
//!
//! ```text
//! cargo xtask extract [--jar server.jar] [--java path/to/java] [--out data/worldgen-26.3.bin]
//! ```
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

const VERSION: &str = "26.3";
const MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

type Result<T> = std::result::Result<T, String>;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("extract") => extract(&args[1..]),
        _ => Err(
            "usage: cargo xtask extract [--jar <server.jar>] [--java <java>] [--out <file>]".into(),
        ),
    };
    if let Err(e) = result {
        eprintln!("xtask: {e}");
        std::process::exit(1);
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned()
}

fn curl(url: &str, out: &Path) -> Result<()> {
    let status = Command::new("curl")
        .args(["-fsSL", "--retry", "4", "-o"])
        .arg(out)
        .arg(url)
        .status()
        .map_err(|e| format!("could not run curl: {e}"))?;
    if !status.success() {
        return Err(format!("download failed: {url}"));
    }
    Ok(())
}

fn download_server(cache: &Path) -> Result<PathBuf> {
    let jar = cache.join(format!("server-{VERSION}.jar"));
    if jar.exists() {
        return Ok(jar);
    }
    fs::create_dir_all(cache).map_err(|e| e.to_string())?;
    let manifest_path = cache.join("version_manifest_v2.json");
    curl(MANIFEST, &manifest_path)?;
    let manifest: Value = read_json(&manifest_path)?;
    let url = manifest["versions"]
        .as_array()
        .and_then(|v| v.iter().find(|v| v["id"] == VERSION))
        .and_then(|v| v["url"].as_str())
        .ok_or(format!("{VERSION} is not in Mojang's version manifest"))?
        .to_owned();
    let version_path = cache.join(format!("{VERSION}.json"));
    curl(&url, &version_path)?;
    let version = read_json(&version_path)?;
    let server = version["downloads"]["server"]["url"]
        .as_str()
        .ok_or("no server download")?;
    eprintln!("Downloading the {VERSION} server jar from Mojang");
    curl(server, &jar)?;
    Ok(jar)
}

fn read_json(path: &Path) -> Result<Value> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The server jar bundles the real game jar under META-INF/versions.
fn inner_jar(server: &Path) -> Result<Vec<u8>> {
    let file = fs::File::open(server).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let name = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_owned()))
        .find(|n| n.starts_with("META-INF/versions/") && n.ends_with(".jar"))
        .ok_or("no bundled game jar in the server jar")?;
    let mut bytes = Vec::new();
    zip.by_name(&name)
        .map_err(|e| e.to_string())?
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}

/// `data/minecraft/<dir>/**.json` as identifier -> minified JSON text.
fn resources(
    zip: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>,
    dir: &str,
) -> Result<BTreeMap<String, String>> {
    let prefix = format!("data/minecraft/{dir}/");
    let names: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_owned()))
        .filter(|n| n.starts_with(&prefix) && n.ends_with(".json"))
        .collect();
    let mut out = BTreeMap::new();
    for name in names {
        let mut text = String::new();
        zip.by_name(&name)
            .map_err(|e| e.to_string())?
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        let value: Value = serde_json::from_str(&text).map_err(|e| format!("{name}: {e}"))?;
        let id = format!("minecraft:{}", &name[prefix.len()..name.len() - 5]);
        out.insert(id, value.to_string());
    }
    if out.is_empty() {
        return Err(format!("nothing under {prefix}"));
    }
    Ok(out)
}

fn run_reports(java: &str, server: &Path, work: &Path) -> Result<PathBuf> {
    let reports = work.join("out/reports/biome_parameters/minecraft");
    if reports.join("overworld.json").exists() {
        return Ok(reports);
    }
    fs::create_dir_all(work).map_err(|e| e.to_string())?;
    eprintln!("Running the {VERSION} data generator for the biome parameter tables");
    let status = Command::new(java)
        .current_dir(work)
        .arg("-DbundlerMainClass=net.minecraft.data.Main")
        .arg("-jar")
        .arg(fs::canonicalize(server).map_err(|e| e.to_string())?)
        .args(["--reports", "--output", "out"])
        .status()
        .map_err(|e| format!("could not run {java} (Java 25 is required): {e}"))?;
    if !status.success() || !reports.join("overworld.json").exists() {
        return Err("the data generator did not write the biome parameter reports".into());
    }
    Ok(reports)
}

fn extract(args: &[String]) -> Result<()> {
    let root = root();
    let cache = root.join(".cache");
    let server = match flag(args, "--jar") {
        Some(p) => PathBuf::from(p),
        None => download_server(&cache)?,
    };
    let java = flag(args, "--java")
        .or_else(|| std::env::var("JAVA").ok())
        .unwrap_or_else(|| "java".into());
    let out = flag(args, "--out")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(format!("data/worldgen-{VERSION}.bin")));

    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(inner_jar(&server)?))
        .map_err(|e| e.to_string())?;
    let mut bundle = Map::new();
    bundle.insert("version".into(), json!(VERSION));
    for (key, dir) in [
        ("noises", "worldgen/noise"),
        ("densities", "worldgen/density_function"),
        ("noise_settings", "worldgen/noise_settings"),
        ("structures", "worldgen/structure"),
        ("structure_sets", "worldgen/structure_set"),
        ("biome_tags", "tags/worldgen/biome"),
    ] {
        bundle.insert(key.into(), json!(resources(&mut zip, dir)?));
    }
    // Only the biome names: colours and labels are the app's own.
    let biomes: Vec<String> = resources(&mut zip, "worldgen/biome")?.into_keys().collect();
    bundle.insert("biomes".into(), json!(biomes));

    let reports = run_reports(&java, &server, &cache.join("datagen"))?;
    let mut parameters = BTreeMap::new();
    for name in ["overworld", "nether"] {
        let text =
            fs::read_to_string(reports.join(format!("{name}.json"))).map_err(|e| e.to_string())?;
        let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        parameters.insert(format!("minecraft:{name}"), value.to_string());
    }
    bundle.insert("biome_parameters".into(), json!(parameters));

    let text = Value::Object(bundle).to_string();
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    encoder
        .write_all(text.as_bytes())
        .map_err(|e| e.to_string())?;
    let bytes = encoder.finish().map_err(|e| e.to_string())?;
    if let Some(dir) = out.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(&out, &bytes).map_err(|e| e.to_string())?;
    eprintln!(
        "Wrote {} ({} KiB, {} KiB of JSON)",
        out.display(),
        bytes.len() / 1024,
        text.len() / 1024
    );
    Ok(())
}
