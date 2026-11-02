//! Structure placement and viability.
//!
//! Placement (which chunks a structure set picks) goes through Minecraft
//! Rust's own `StructureRegistry`: the same random spread, frequency
//! reduction, exclusion zones and concentric rings the game runs. Whether a
//! candidate really generates is then checked the way `Structure.generate`
//! decides it: the variant chosen by weight, its biome at the start
//! position, and the terrain rules of temples, mansions and End cities.
//! Jigsaw starts are checked at the start chunk's corner (their start
//! piece's centre is within a few blocks of it).
use crate::biomes::BiomeId;
use crate::data::{Data, data};
use crate::world::{Dimension, Sampler, World};
use claudium_core::position::ChunkPos;
use claudium_core::random::{LegacyRandom, WorldgenRandom};
use claudium_world::worldgen::anchor::WorldGenerationContext;
use claudium_world::worldgen::height_provider::HeightProvider;
use claudium_world::worldgen::structure::Rotation;
use claudium_world::worldgen::structure::start::{Rings, StructureKind, StructureRegistry};
use claudium_world::worldgen::structure::stronghold::ring_searches;
use rayon::prelude::*;
use std::sync::OnceLock;

/// A set of biomes as a bit mask (the 26.3 registry has 67).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BiomeMask(pub u128);
impl BiomeMask {
    pub fn contains(self, b: BiomeId) -> bool {
        b.0 < 128 && self.0 & (1u128 << b.0) != 0
    }
    pub fn insert(&mut self, b: BiomeId) {
        if b.0 < 128 {
            self.0 |= 1u128 << b.0;
        }
    }
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// How a structure's start position is found, by structure type.
#[derive(Clone, Debug)]
enum Start {
    /// Structure.onTopOfChunkCenter: chunk centre at the heightmap's top.
    Center {
        ocean_floor: bool,
    },
    /// Desert pyramid / jungle temple: the lowest footprint corner must be
    /// at sea level or above.
    Temple {
        width: i32,
        depth: i32,
    },
    /// Ocean monument: every biome within 29 blocks at sea level must allow it.
    Monument(BiomeMask),
    /// Woodland mansion / End city: rotation-dependent corners at y >= 60.
    Mansion,
    EndCity,
    /// A fixed start (strongholds, fortresses, mineshafts).
    Fixed {
        dx: i32,
        y: i32,
        dz: i32,
    },
    /// Jigsaw: the start height provider, projected to the surface or not.
    Jigsaw {
        height: HeightProvider,
        project: bool,
    },
    /// Nether fossils: a random column and height, then the first floor
    /// below it above the lava sea.
    NetherFossil(HeightProvider),
    /// Mineshafts: Minecraft Rust's own layout gives the start room.
    Mineshaft {
        mesa: bool,
        blocking:
            std::sync::Arc<std::collections::BTreeSet<claudium_world::worldgen::biome::BiomeId>>,
    },
    /// Ruined portals: the chunk centre near the surface.
    Portal,
}

pub struct StructureInfo {
    pub name: String,
    pub biomes: BiomeMask,
    start: Start,
}

/// One structure set as the map shows it.
pub struct SetInfo {
    pub index: usize,
    pub name: String,
    pub label: &'static str,
    pub short: &'static str,
    pub color: [u8; 3],
    pub dimensions: Vec<Dimension>,
    /// Region size in chunks (`None`: concentric rings).
    pub spacing: Option<i32>,
    pub new_in_26: bool,
    /// Structures in the set: (index into `Catalog::structures`, weight).
    pub members: Vec<(usize, i32)>,
    /// A plain random spread (no frequency reduction or exclusion zone),
    /// which the search's fast path computes inline.
    pub spread: Option<Spread>,
}

/// RandomSpreadStructurePlacement's numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spread {
    pub spacing: i32,
    pub separation: i32,
    pub triangular: bool,
    pub salt: i32,
}

const MULTIPLIER: i64 = 0x5DEE_CE66D;
const MASK: i64 = (1 << 48) - 1;

/// java.util.Random, inlined for the search's hot loop.
#[derive(Clone, Copy)]
struct JavaRandom(i64);
impl JavaRandom {
    #[inline(always)]
    fn new(seed: i64) -> Self {
        Self((seed ^ MULTIPLIER) & MASK)
    }
    #[inline(always)]
    fn next(&mut self, bits: u32) -> i32 {
        self.0 = (self.0.wrapping_mul(MULTIPLIER).wrapping_add(0xB)) & MASK;
        (self.0 >> (48 - bits)) as i32
    }
    #[inline(always)]
    fn next_int(&mut self, bound: i32) -> i32 {
        if bound & (bound - 1) == 0 {
            return ((i64::from(bound) * i64::from(self.next(31))) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let value = bits % bound;
            if bits.wrapping_sub(value).wrapping_add(bound - 1) >= 0 {
                return value;
            }
        }
    }
}

impl Spread {
    /// The candidate chunk of region (`gx`, `gz`):
    /// RandomSpreadStructurePlacement.getPotentialStructureChunk.
    #[inline]
    pub fn chunk(self, seed: i64, gx: i32, gz: i32) -> (i32, i32) {
        let s = i64::from(gx)
            .wrapping_mul(341_873_128_712)
            .wrapping_add(i64::from(gz).wrapping_mul(132_897_987_541))
            .wrapping_add(seed)
            .wrapping_add(i64::from(self.salt));
        let mut r = JavaRandom::new(s);
        let limit = self.spacing - self.separation;
        let (dx, dz) = if self.triangular {
            let x = (r.next_int(limit) + r.next_int(limit)) / 2;
            let z = (r.next_int(limit) + r.next_int(limit)) / 2;
            (x, z)
        } else {
            let x = r.next_int(limit);
            (x, r.next_int(limit))
        };
        (gx * self.spacing + dx, gz * self.spacing + dz)
    }
}

pub struct Catalog {
    pub sets: Vec<SetInfo>,
    pub structures: Vec<StructureInfo>,
}

fn label(set: &str) -> (&'static str, &'static str, [u8; 3]) {
    match set.trim_start_matches("minecraft:") {
        "villages" => ("Village", "V", [244, 196, 48]),
        "pillager_outposts" => ("Pillager Outpost", "P", [96, 102, 112]),
        "desert_pyramids" => ("Desert Temple", "D", [250, 228, 150]),
        "jungle_temples" => ("Jungle Temple", "J", [60, 150, 50]),
        "swamp_huts" => ("Witch Hut", "W", [128, 72, 176]),
        "igloos" => ("Igloo", "I", [214, 240, 255]),
        "ocean_monuments" => ("Ocean Monument", "M", [32, 206, 196]),
        "woodland_mansions" => ("Woodland Mansion", "WM", [124, 74, 34]),
        "shipwrecks" => ("Shipwreck", "S", [168, 120, 80]),
        "ocean_ruins" => ("Ocean Ruins", "OR", [120, 168, 184]),
        "buried_treasures" => ("Buried Treasure", "T", [255, 222, 0]),
        "ruined_portals" => ("Ruined Portal", "RP", [196, 72, 232]),
        "mineshafts" => ("Mineshaft", "MS", [140, 108, 76]),
        "strongholds" => ("Stronghold", "SH", [36, 224, 120]),
        "ancient_cities" => ("Ancient City", "AC", [18, 84, 104]),
        "trail_ruins" => ("Trail Ruins", "TR", [236, 132, 168]),
        "trial_chambers" => ("Trial Chambers", "TC", [214, 108, 44]),
        "abandoned_camp" => ("Abandoned Camp", "AB", [255, 64, 64]),
        "nether_complexes" => ("Fortress / Bastion", "F", [170, 36, 36]),
        "nether_fossils" => ("Nether Fossil", "NF", [232, 228, 208]),
        "end_cities" => ("End City", "E", [214, 196, 255]),
        _ => ("Structure", "?", [255, 255, 255]),
    }
}

/// Structure sets the 26.x releases added.
const NEW_IN_26: [&str; 1] = ["minecraft:abandoned_camp"];

pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| build_catalog(data()))
}

fn mask_of(
    set: &std::collections::BTreeSet<claudium_world::worldgen::biome::BiomeId>,
) -> BiomeMask {
    let mut mask = BiomeMask::default();
    for b in set {
        mask.insert(BiomeId(b.0 as u8));
    }
    mask
}

fn build_catalog(data: &Data) -> Catalog {
    let registry = &data.structures;
    let structures = registry
        .structures
        .iter()
        .map(|def| {
            let json = &data.structure_json[&def.name];
            let start = match &def.kind {
                StructureKind::Jigsaw(_) => Start::Jigsaw {
                    height: HeightProvider::from_json(&json["start_height"].to_string())
                        .unwrap_or_else(|_| {
                            HeightProvider::from_json(r#"{"absolute":0}"#).unwrap()
                        }),
                    project: json["project_start_to_heightmap"].is_string(),
                },
                StructureKind::BuriedTreasure | StructureKind::OceanRuin { .. } => {
                    Start::Center { ocean_floor: true }
                }
                StructureKind::Shipwreck { beached } => Start::Center {
                    ocean_floor: !beached,
                },
                StructureKind::Igloo | StructureKind::SwampHut => {
                    Start::Center { ocean_floor: false }
                }
                StructureKind::DesertPyramid => Start::Temple {
                    width: 21,
                    depth: 21,
                },
                StructureKind::JungleTemple => Start::Temple {
                    width: 12,
                    depth: 15,
                },
                StructureKind::Monument(surrounding) => Start::Monument(mask_of(surrounding)),
                StructureKind::Mansion => Start::Mansion,
                StructureKind::EndCity => Start::EndCity,
                StructureKind::Stronghold => Start::Fixed { dx: 0, y: 0, dz: 0 },
                StructureKind::Fortress => Start::Fixed {
                    dx: 0,
                    y: 64,
                    dz: 0,
                },
                StructureKind::Mineshaft { mesa, blocking } => Start::Mineshaft {
                    mesa: *mesa,
                    blocking: blocking.clone(),
                },
                StructureKind::NetherFossil(_) => Start::NetherFossil(
                    HeightProvider::from_json(&json["height"].to_string()).unwrap_or_else(|_| {
                        HeightProvider::from_json(r#"{"absolute":32}"#).unwrap()
                    }),
                ),
                StructureKind::RuinedPortal(_) => Start::Portal,
                _ => Start::Center { ocean_floor: false },
            };
            StructureInfo {
                name: def.name.clone(),
                biomes: mask_of(&def.biomes),
                start,
            }
        })
        .collect::<Vec<_>>();

    let mut dimension_biomes = [BiomeMask::default(); 3];
    for (_, b) in data.overworld.entries() {
        dimension_biomes[0].insert(*b);
    }
    for (_, b) in data.nether.entries() {
        dimension_biomes[1].insert(*b);
    }
    for name in [
        "the_end",
        "end_highlands",
        "end_midlands",
        "small_end_islands",
        "end_barrens",
    ] {
        if let Some(b) = data.biome(name) {
            dimension_biomes[2].insert(b);
        }
    }

    let sets = registry
        .sets
        .iter()
        .enumerate()
        .map(|(index, set)| {
            let (label, short, color) = label(&set.name);
            let dimensions = Dimension::ALL
                .into_iter()
                .zip(dimension_biomes)
                .filter(|(_, mask)| {
                    set.structures
                        .iter()
                        .any(|(s, _)| structures[*s].biomes.0 & mask.0 != 0)
                })
                .map(|(d, _)| d)
                .collect();
            SetInfo {
                index,
                name: set.name.clone(),
                label,
                short,
                color,
                dimensions,
                spacing: set.spacing(),
                new_in_26: NEW_IN_26.contains(&set.name.as_str()),
                members: set.structures.clone(),
                spread: spread_of(&data.structure_set_json[&set.name]),
            }
        })
        .collect();
    Catalog { sets, structures }
}

fn spread_of(json: &serde_json::Value) -> Option<Spread> {
    let p = &json["placement"];
    if p["type"].as_str() != Some("minecraft:random_spread")
        || p.get("exclusion_zone").is_some()
        || p["frequency"].as_f64().is_some_and(|f| f < 1.0)
    {
        return None;
    }
    let spacing = p["spacing"].as_i64()? as i32;
    Some(Spread {
        spacing,
        separation: p["separation"].as_i64().unwrap_or(0) as i32,
        triangular: p["spread_type"].as_str() == Some("triangular"),
        salt: p["salt"].as_i64().unwrap_or(0) as i32,
    })
    .filter(|s| s.spacing > s.separation)
}

impl Catalog {
    pub fn set(&self, name: &str) -> Option<&SetInfo> {
        let name = if name.contains(':') {
            name.to_owned()
        } else {
            format!("minecraft:{name}")
        };
        self.sets.iter().find(|s| s.name == name)
    }
    pub fn sets_in(&self, dimension: Dimension) -> impl Iterator<Item = &SetInfo> {
        self.sets
            .iter()
            .filter(move |s| s.dimensions.contains(&dimension))
    }
}

fn registry() -> &'static StructureRegistry {
    &data().structures
}

static NO_RINGS: OnceLock<Rings> = OnceLock::new();
fn no_rings() -> &'static Rings {
    NO_RINGS.get_or_init(Rings::new)
}

/// Placement only: whether the set picks this chunk (frequency and
/// exclusion zones included). Concentric rings are not answered here.
pub fn is_candidate(seed: i64, set: usize, cx: i32, cz: i32) -> bool {
    let registry = registry();
    registry.is_structure_chunk(&registry.sets[set], seed, cx, cz, no_rings())
}

/// The set's candidate chunks in an inclusive chunk rectangle (placement
/// only). Random-spread sets cost one draw per region, not per chunk.
pub fn candidates(seed: i64, set: usize, x0: i32, z0: i32, x1: i32, z1: i32) -> Vec<(i32, i32)> {
    let registry = registry();
    let info = &registry.sets[set];
    let mut out = Vec::new();
    let Some(spacing) = info.spacing() else {
        return out;
    };
    if let Some(spread) = catalog().sets[set].spread {
        for gz in z0.div_euclid(spacing)..=z1.div_euclid(spacing) {
            for gx in x0.div_euclid(spacing)..=x1.div_euclid(spacing) {
                let (cx, cz) = spread.chunk(seed, gx, gz);
                if cx >= x0 && cx <= x1 && cz >= z0 && cz <= z1 {
                    out.push((cx, cz));
                }
            }
        }
        return out;
    }
    if spacing <= 1 {
        for cz in z0..=z1 {
            for cx in x0..=x1 {
                if registry.is_structure_chunk(info, seed, cx, cz, no_rings()) {
                    out.push((cx, cz));
                }
            }
        }
        return out;
    }
    for gz in z0.div_euclid(spacing)..=z1.div_euclid(spacing) {
        for gx in x0.div_euclid(spacing)..=x1.div_euclid(spacing) {
            let Some(p) =
                StructureRegistry::potential_chunk(info, seed, gx * spacing, gz * spacing)
            else {
                continue;
            };
            if p.x < x0 || p.x > x1 || p.z < z0 || p.z > z1 {
                continue;
            }
            if registry.is_structure_chunk(info, seed, p.x, p.z, no_rings()) {
                out.push((p.x, p.z));
            }
        }
    }
    out
}

/// A structure that generates (as far as the map can tell).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub set: usize,
    pub chunk: (i32, i32),
    /// Index into `Catalog::structures`: the variant that generates.
    pub structure: usize,
    /// Block position: where vanilla checks the biome.
    pub pos: [i32; 3],
}

impl Sampler<'_> {
    /// The heightmap top at a column (first free block above the terrain):
    /// WORLD_SURFACE_WG counts the sea, OCEAN_FLOOR_WG does not.
    pub fn heightmap(&mut self, x: i32, z: i32, ocean_floor: bool) -> i32 {
        let world = self.world();
        let settings = world.data.settings(world.spec.preset());
        let (min, max) = (settings.min_y, settings.min_y + settings.height - 1);
        let terrain = match world.spec.dimension {
            Dimension::Overworld if !self.exact_heights => Some(self.surface_estimate(x, z)),
            Dimension::Overworld => {
                let estimate = self.surface_estimate(x, z);
                self.terrain_height_near(x, z, estimate, min, max)
            }
            _ => self.terrain_height(x, z, max.min(127), min),
        };
        let top = terrain.map_or(min, |y| y + 1);
        if ocean_floor || world.spec.dimension != Dimension::Overworld {
            top
        } else {
            top.max(world.sea_level())
        }
    }

    /// Whether the set's structure really starts in this chunk, and which
    /// variant. `cx`, `cz` must be a placement candidate.
    pub fn validate(&mut self, set: usize, cx: i32, cz: i32) -> Option<Found> {
        let catalog = catalog();
        let seed = self.world().seed();
        let members = &catalog.sets[set].members;
        if members.len() == 1 {
            return self.try_structure(members[0].0, cx, cz).map(|pos| Found {
                set,
                chunk: (cx, cz),
                structure: members[0].0,
                pos,
            });
        }
        // ChunkGenerator.tryGenerateStructure's weighted pick without
        // replacement, from the chunk's large-feature seed.
        let mut options = members.clone();
        let mut random = WorldgenRandom::new(LegacyRandom::new(0));
        random.set_large_feature_seed(seed, cx, cz);
        let mut total: i32 = options.iter().map(|o| o.1).sum();
        while !options.is_empty() {
            let mut choice =
                claudium_core::random::RandomSource::next_int_bound(&mut random, total)
                    .unwrap_or(0);
            let mut index = 0;
            for option in &options {
                choice -= option.1;
                if choice < 0 {
                    break;
                }
                index += 1;
            }
            let index = index.min(options.len() - 1);
            let selected = options[index];
            if let Some(pos) = self.try_structure(selected.0, cx, cz) {
                return Some(Found {
                    set,
                    chunk: (cx, cz),
                    structure: selected.0,
                    pos,
                });
            }
            options.remove(index);
            total -= selected.1;
        }
        None
    }

    fn try_structure(&mut self, structure: usize, cx: i32, cz: i32) -> Option<[i32; 3]> {
        let info = &catalog().structures[structure];
        let seed = self.world().seed();
        let (bx, bz) = (cx * 16, cz * 16);
        let biome_ok = |s: &mut Self, p: [i32; 3]| {
            info.biomes
                .contains(s.biome([p[0] >> 2, p[1] >> 2, p[2] >> 2]))
        };
        let mut random = WorldgenRandom::new(LegacyRandom::new(0));
        random.set_large_feature_seed(seed, cx, cz);
        let world = self.world();
        let settings = world.data.settings(world.spec.preset());
        let sea = world.sea_level();
        let position = match &info.start {
            Start::Center { ocean_floor } => {
                let (x, z) = (bx + 8, bz + 8);
                [x, self.heightmap(x, z, *ocean_floor) - 1, z]
            }
            Start::Temple { width, depth } => {
                let (x, z) = (bx + 8, bz + 8);
                if !biome_ok(self, [x, sea, z]) {
                    let y = self.heightmap(x, z, false) - 1;
                    if !biome_ok(self, [x, y, z]) {
                        return None;
                    }
                }
                let corners = [
                    (bx, bz),
                    (bx, bz + depth),
                    (bx + width, bz),
                    (bx + width, bz + depth),
                ];
                let lowest = corners
                    .iter()
                    .map(|&(x, z)| self.heightmap(x, z, false) - 1)
                    .min()
                    .unwrap_or(0);
                if lowest < sea {
                    return None;
                }
                [x, self.heightmap(x, z, false) - 1, z]
            }
            Start::Monument(surrounding) => {
                let (x, z) = (bx + 9, bz + 9);
                if !biome_ok(self, [x, sea, z]) {
                    return None;
                }
                let r = 29;
                // The sea-level layer first: it fails most often.
                let qy0 = sea >> 2;
                let layers = std::iter::once(qy0)
                    .chain(((sea - r) >> 2..=(sea + r) >> 2).filter(|y| *y != qy0));
                for qy in layers {
                    for qz in (z - r) >> 2..=(z + r) >> 2 {
                        for qx in (x - r) >> 2..=(x + r) >> 2 {
                            if !surrounding.contains(self.biome([qx, qy, qz])) {
                                return None;
                            }
                        }
                    }
                }
                [bx + 8, self.heightmap(bx + 8, bz + 8, true) - 1, bz + 8]
            }
            Start::Mansion | Start::EndCity => {
                let (x, z) = (bx + 7, bz + 7);
                let rotation = Rotation::random(&mut random);
                let (sx, sz) = match rotation {
                    Rotation::None => (5, 5),
                    Rotation::Clockwise90 => (-5, 5),
                    Rotation::Clockwise180 => (-5, -5),
                    Rotation::Counterclockwise90 => (5, -5),
                };
                // Biome first (cheap), then the terrain rule.
                let guess = if world.spec.dimension == Dimension::End {
                    64
                } else {
                    sea
                };
                if !biome_ok(self, [x, guess, z]) && world.spec.dimension != Dimension::End {
                    let y = self.heightmap(x, z, false) - 1;
                    if !biome_ok(self, [x, y, z]) {
                        return None;
                    }
                }
                let y = [(x, z), (x, z + sz), (x + sx, z), (x + sx, z + sz)]
                    .iter()
                    .map(|&(px, pz)| self.heightmap(px, pz, false) - 1)
                    .min()
                    .unwrap_or(0);
                if y < 60 {
                    return None;
                }
                [x, y, z]
            }
            Start::Fixed { dx, y, dz } => [bx + dx, *y, bz + dz],
            Start::Mineshaft { mesa, blocking } => {
                let min_y = settings.min_y;
                let (position, _) = claudium_world::worldgen::structure::mineshaft::generate(
                    &mut random,
                    [cx, cz],
                    *mesa,
                    blocking.clone(),
                    sea,
                    min_y,
                    &mut |x, z| self.heightmap(x, z, false),
                );
                position
            }
            Start::Jigsaw { height, project } => {
                let generation = WorldGenerationContext::new(settings.min_y, settings.height, sea);
                let y = height.sample(&mut random, generation).unwrap_or(0);
                if *project {
                    [bx, self.heightmap(bx, bz, false) + y, bz]
                } else {
                    [bx, y, bz]
                }
            }
            Start::NetherFossil(height) => {
                use claudium_core::random::RandomSource;
                let x = bx + random.next_int_bound(16).unwrap_or(0);
                let z = bz + random.next_int_bound(16).unwrap_or(0);
                let generation = WorldGenerationContext::new(settings.min_y, settings.height, sea);
                let mut y = height.sample(&mut random, generation).unwrap_or(sea);
                // Nether biomes do not change with height: reject before the scan.
                if !biome_ok(self, [x, 64, z]) {
                    return None;
                }
                // NetherFossilStructure: walk down to air above a solid block.
                while y > sea {
                    let air = !self.solid(x, y, z).unwrap_or(false);
                    y -= 1;
                    if air && self.solid(x, y, z).unwrap_or(false) {
                        break;
                    }
                }
                if y <= sea {
                    return None;
                }
                [x, y, z]
            }
            Start::Portal => {
                let (x, z) = (bx + 8, bz + 8);
                if world.spec.dimension == Dimension::Nether {
                    [x, 64, z]
                } else {
                    [x, self.heightmap(x, z, false) - 1, z]
                }
            }
        };
        biome_ok(self, position).then_some(position)
    }

    /// Terrain height searched around an estimate: up while solid, else down.
    fn terrain_height_near(
        &mut self,
        x: i32,
        z: i32,
        estimate: i32,
        min: i32,
        max: i32,
    ) -> Option<i32> {
        let start = (estimate + 8).clamp(min, max);
        if self.solid(x, start, z)? {
            let mut y = start;
            while y + 8 <= max && self.solid(x, y + 8, z)? {
                y += 8;
            }
            let mut top = y;
            for k in (y + 1..=(y + 7).min(max)).rev() {
                if self.solid(x, k, z)? {
                    top = k;
                    break;
                }
            }
            Some(top)
        } else {
            self.terrain_height(x, z, start, min)
        }
    }
}

/// The stronghold ring positions (all 128), each found with its biome
/// search as the game runs it; then validated like any other start.
pub fn strongholds(world: &World) -> Vec<Found> {
    stronghold_search(world, None, true)
}

/// The strongholds that can lie within `radius` blocks (chebyshev) of 0 0:
/// only those ring positions get their biome search, which skips most of
/// the 128. For the seed finder, on its own thread.
pub fn strongholds_near(world: &World, radius: i32) -> Vec<Found> {
    stronghold_search(world, Some(radius), false)
}

/// Stage one for strongholds: whether any ring position can end within
/// `radius` blocks of 0 0. Pure random draws, no world needed.
pub fn stronghold_may_be_near(seed: i64, radius: i32) -> bool {
    ring_searches(seed, 32, 3, 128)
        .iter()
        .take(3) // the first ring is always the nearest
        .any(|s| s.may_land_near(0, 0, radius.div_euclid(16) + 1))
}

fn stronghold_search(world: &World, radius: Option<i32>, parallel: bool) -> Vec<Found> {
    let catalog = catalog();
    let Some(set) = catalog.set("strongholds") else {
        return Vec::new();
    };
    if world.spec.dimension != Dimension::Overworld {
        return Vec::new();
    }
    let preferred: std::collections::BTreeSet<claudium_world::worldgen::biome::BiomeId> =
        data().structures.ring_biomes(set.index).clone();
    let (distance, spread, count) = (32, 3, 128);
    let searches: Vec<_> = ring_searches(world.seed(), distance, spread, count)
        .into_iter()
        .filter(|s| radius.is_none_or(|r| s.may_land_near(0, 0, r.div_euclid(16) + 1)))
        .collect();
    let find = |search: &claudium_world::worldgen::structure::stronghold::RingSearch| {
        let mut sampler = world.sampler();
        search.find(&preferred, &mut |quarts: &[[i32; 3]]| {
            quarts
                .iter()
                .map(|q| claudium_world::worldgen::biome::BiomeId(u32::from(sampler.biome(*q).0)))
                .collect()
        })
    };
    let positions: Vec<ChunkPos> = if parallel {
        searches.par_iter().map(find).collect()
    } else {
        searches.iter().map(find).collect()
    };
    let mut sampler = world.sampler();
    positions
        .into_iter()
        .filter_map(|p| {
            sampler.validate(set.index, p.x, p.z).or(Some(Found {
                set: set.index,
                chunk: (p.x, p.z),
                structure: set.members[0].0,
                pos: [p.x * 16, 0, p.z * 16],
            }))
        })
        .collect()
}

/// Slime chunks: the classic per-chunk roll, unchanged in 26.3.
pub fn is_slime_chunk(seed: i64, cx: i32, cz: i32) -> bool {
    let mut r = claudium_core::random::slime_chunk_random(cx, cz, seed, 987_234_911);
    claudium_core::random::RandomSource::next_int_bound(&mut r, 10).unwrap_or(1) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The inline spread agrees with Minecraft Rust's own placement.
    #[test]
    fn fast_spread_matches_the_registry() {
        let registry = registry();
        let mut state = 0x1234_5678_u64;
        let mut next = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            state
        };
        for set in &catalog().sets {
            let Some(spread) = set.spread else { continue };
            for _ in 0..2000 {
                let seed = next() as i64;
                let gx = (next() % 20001) as i32 - 10000;
                let gz = (next() % 20001) as i32 - 10000;
                let ours = spread.chunk(seed, gx, gz);
                let theirs = StructureRegistry::potential_chunk(
                    &registry.sets[set.index],
                    seed,
                    gx * spread.spacing,
                    gz * spread.spacing,
                )
                .unwrap();
                assert_eq!(
                    ours,
                    (theirs.x, theirs.z),
                    "{} seed {seed} region {gx} {gz}",
                    set.name
                );
                assert!(is_candidate(seed, set.index, ours.0, ours.1));
            }
        }
    }

    #[test]
    fn slime_chunks_follow_the_classic_roll() {
        // Seed 0's slime chunks near the origin, as the game has them.
        let found: Vec<(i32, i32)> = (-4..4)
            .flat_map(|x| (-4..4).map(move |z| (x, z)))
            .filter(|&(x, z)| is_slime_chunk(0, x, z))
            .collect();
        assert!(!found.is_empty());
    }
}
