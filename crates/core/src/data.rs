//! The embedded 26.3 worldgen bundle, decoded once and shared by every world.
use crate::biomes::BiomeId;
use claudium_core::density::graph::DensityGraph;
use claudium_world::worldgen::{
    climate::{ClimateIndex, Parameter, ParameterPoint, quantize},
    density::DensityRegistry,
    router::NoiseRouter,
    structure::start::StructureRegistry,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::sync::OnceLock;

static BUNDLE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/worldgen.bin"));

/// Which noise settings a world uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Preset {
    Overworld,
    LargeBiomes,
    Amplified,
    Nether,
    End,
}
impl Preset {
    pub fn settings_name(self) -> &'static str {
        match self {
            Self::Overworld => "minecraft:overworld",
            Self::LargeBiomes => "minecraft:large_biomes",
            Self::Amplified => "minecraft:amplified",
            Self::Nether => "minecraft:nether",
            Self::End => "minecraft:end",
        }
    }
}

/// One noise-settings entry with its router decoded into its own graph: a
/// world clones the graph and compiles it for its seed.
pub struct Settings {
    pub graph: DensityGraph,
    pub router: NoiseRouter,
    pub legacy_random: bool,
    pub sea_level: i32,
    pub min_y: i32,
    pub height: i32,
    /// `spawn_target`: per point, (density function id, interval).
    pub spawn_target: Vec<Vec<(claudium_core::density::graph::FunctionId, Parameter)>>,
}

pub struct Data {
    pub version: String,
    /// Biome identifiers in registry order; `BiomeId` indexes this.
    pub biome_names: Vec<String>,
    pub biome_tags: BTreeMap<String, BTreeSet<BiomeId>>,
    pub overworld: ClimateIndex<BiomeId>,
    pub nether: ClimateIndex<BiomeId>,
    pub settings: BTreeMap<Preset, Settings>,
    pub structures: StructureRegistry,
    pub structure_json: BTreeMap<String, Value>,
    pub structure_set_json: BTreeMap<String, Value>,
}

/// The decoded bundle; built on first use (about a tenth of a second).
pub fn data() -> &'static Data {
    static DATA: OnceLock<Data> = OnceLock::new();
    DATA.get_or_init(|| Data::load().expect("embedded worldgen bundle"))
}

fn map(value: &Value) -> BTreeMap<String, String> {
    value
        .as_object()
        .map(|o| {
            o.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
                .collect()
        })
        .unwrap_or_default()
}

impl Data {
    fn load() -> Result<Self, String> {
        let mut text = String::new();
        flate2::read::GzDecoder::new(BUNDLE)
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        let bundle: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let biome_names: Vec<String> = bundle["biomes"]
            .as_array()
            .ok_or("biomes")?
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
        let id_of = |name: &str| -> Option<BiomeId> {
            biome_names
                .iter()
                .position(|n| n == name)
                .map(|i| BiomeId(i as u8))
        };

        // Biome tags, with nested tags resolved.
        let raw_tags: BTreeMap<String, Value> = map(&bundle["biome_tags"])
            .into_iter()
            .map(|(k, v)| (k, serde_json::from_str(&v).unwrap_or(Value::Null)))
            .collect();
        fn resolve(
            name: &str,
            raw: &BTreeMap<String, Value>,
            id_of: &dyn Fn(&str) -> Option<BiomeId>,
            depth: usize,
            out: &mut BTreeSet<BiomeId>,
        ) {
            let Some(tag) = raw.get(name) else { return };
            if depth > 16 {
                return;
            }
            for entry in tag["values"].as_array().into_iter().flatten() {
                let id = entry
                    .as_str()
                    .or_else(|| entry["id"].as_str())
                    .unwrap_or_default();
                if let Some(nested) = id.strip_prefix('#') {
                    resolve(nested, raw, id_of, depth + 1, out);
                } else if let Some(b) = id_of(id) {
                    out.insert(b);
                }
            }
        }
        let mut biome_tags = BTreeMap::new();
        for name in raw_tags.keys() {
            let mut set = BTreeSet::new();
            resolve(name, &raw_tags, &id_of, 0, &mut set);
            biome_tags.insert(name.clone(), set);
        }

        let parameters = map(&bundle["biome_parameters"]);
        let table = |name: &str| -> Result<ClimateIndex<BiomeId>, String> {
            let report: Value = serde_json::from_str(parameters.get(name).ok_or(name.to_owned())?)
                .map_err(|e| e.to_string())?;
            quantized_table(&report, &id_of)
        };
        let overworld = table("minecraft:overworld")?;
        let nether = table("minecraft:nether")?;

        let noises = map(&bundle["noises"]);
        let densities = map(&bundle["densities"]);
        let settings_json = map(&bundle["noise_settings"]);
        let mut settings = BTreeMap::new();
        for preset in [
            Preset::Overworld,
            Preset::LargeBiomes,
            Preset::Amplified,
            Preset::Nether,
            Preset::End,
        ] {
            let json: Value = serde_json::from_str(
                settings_json
                    .get(preset.settings_name())
                    .ok_or("noise settings")?,
            )
            .map_err(|e| e.to_string())?;
            let mut registry =
                DensityRegistry::from_json(&noises, &densities).map_err(|e| format!("{e:?}"))?;
            let router = NoiseRouter::from_json(&json["noise_router"].to_string(), &mut registry)
                .map_err(|e| format!("{e:?}"))?;
            let graph = registry.into_graph();
            let spawn_target = json["spawn_target"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|point| {
                    point
                        .as_object()
                        .into_iter()
                        .flatten()
                        .filter_map(|(name, range)| {
                            let id = graph.function(name)?;
                            let lo = range[0].as_f64()? as f32;
                            let hi = range[1].as_f64()? as f32;
                            Some((id, Parameter::new(quantize(lo), quantize(hi))))
                        })
                        .collect()
                })
                .collect();
            settings.insert(
                preset,
                Settings {
                    graph,
                    router,
                    legacy_random: json["legacy_random_source"].as_bool().unwrap_or(false),
                    sea_level: json["sea_level"].as_i64().unwrap_or(63) as i32,
                    min_y: json["noise"]["min_y"].as_i64().unwrap_or(0) as i32,
                    height: json["noise"]["height"].as_i64().unwrap_or(256) as i32,
                    spawn_target,
                },
            );
        }

        let structures = map(&bundle["structures"]);
        let sets = map(&bundle["structure_sets"]);
        let biome_set = |value: &Value| -> Result<
            BTreeSet<claudium_world::worldgen::biome::BiomeId>,
            claudium_world::worldgen::DecodeError,
        > {
            let mut out = BTreeSet::new();
            let mut add = |id: &str| {
                if let Some(tag) = id.strip_prefix('#') {
                    for b in biome_tags.get(tag).into_iter().flatten() {
                        out.insert(claudium_world::worldgen::biome::BiomeId(u32::from(b.0)));
                    }
                } else if let Some(b) = id_of(id) {
                    out.insert(claudium_world::worldgen::biome::BiomeId(u32::from(b.0)));
                }
            };
            match value {
                Value::String(s) => add(s),
                Value::Array(list) => list.iter().filter_map(Value::as_str).for_each(&mut add),
                _ => {}
            }
            Ok(out)
        };
        let registry = StructureRegistry::new(&structures, &sets, BTreeMap::new(), &biome_set)
            .map_err(|e| format!("structures: {e:?}"))?;
        let structure_json = structures
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::from_str(v).unwrap_or(Value::Null)))
            .collect();

        let structure_set_json = sets
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::from_str(v).unwrap_or(Value::Null)))
            .collect();
        Ok(Self {
            version: bundle["version"].as_str().unwrap_or("26.3").to_owned(),
            biome_names,
            biome_tags,
            overworld,
            nether,
            settings,
            structures: registry,
            structure_json,
            structure_set_json,
        })
    }

    pub fn biome(&self, name: &str) -> Option<BiomeId> {
        let name = if name.contains(':') {
            name.to_owned()
        } else {
            format!("minecraft:{name}")
        };
        self.biome_names
            .iter()
            .position(|n| *n == name)
            .map(|i| BiomeId(i as u8))
    }
    pub fn biome_name(&self, id: BiomeId) -> &str {
        self.biome_names
            .get(usize::from(id.0))
            .map_or("minecraft:the_void", String::as_str)
    }
    pub fn tag(&self, name: &str) -> &BTreeSet<BiomeId> {
        static EMPTY: BTreeSet<BiomeId> = BTreeSet::new();
        self.biome_tags.get(name).unwrap_or(&EMPTY)
    }
    pub fn settings(&self, preset: Preset) -> &Settings {
        &self.settings[&preset]
    }
}

/// The data generator's biome parameter report, quantized as the game does
/// (Claudium's `quantize_parameter_report`: each float parsed as f32).
fn quantized_table(
    report: &Value,
    id_of: &dyn Fn(&str) -> Option<BiomeId>,
) -> Result<ClimateIndex<BiomeId>, String> {
    let float = |v: &Value| -> Result<i64, String> {
        v.to_string()
            .parse::<f32>()
            .map(quantize)
            .map_err(|_| "biome parameter value".to_owned())
    };
    let mut entries = Vec::new();
    for entry in report["biomes"]
        .as_array()
        .ok_or("biome parameter report")?
    {
        let p = &entry["parameters"];
        let mut coordinates = [Parameter::new(0, 0); 6];
        for (slot, key) in coordinates.iter_mut().zip([
            "temperature",
            "humidity",
            "continentalness",
            "erosion",
            "depth",
            "weirdness",
        ]) {
            let value = &p[key];
            *slot = match value.as_array() {
                Some(r) if r.len() == 2 => Parameter::new(float(&r[0])?, float(&r[1])?),
                None if value.is_number() => {
                    let v = float(value)?;
                    Parameter::new(v, v)
                }
                _ => return Err("biome parameter interval".into()),
            };
        }
        let name = entry["biome"].as_str().unwrap_or_default();
        let biome = id_of(name).ok_or(format!("unknown biome {name}"))?;
        entries.push((
            ParameterPoint {
                coordinates,
                offset: float(&p["offset"])?,
            },
            biome,
        ));
    }
    ClimateIndex::new(entries).map_err(|e| format!("{e:?}"))
}
