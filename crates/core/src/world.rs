//! A seeded world: Minecraft Rust's density functions compiled for one seed,
//! and per-thread samplers that answer biome and height queries.
use crate::biomes::BiomeId;
use crate::data::{Data, Preset, data};
use claudium_core::density::graph::{DensityCompiler, FunctionId};
use claudium_core::density::{DensitySampler, SamplerContext};
use claudium_core::noise::DensityVolume;
use claudium_world::worldgen::climate::{ClimateIndex, ClimateSearch, TargetPoint};
use claudium_world::worldgen::router::RouterField;
use claudium_world::worldgen::spawn_finder::{SpawnTargetPoint, find_spawn_position};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Dimension {
    Overworld,
    Nether,
    End,
}
impl Dimension {
    pub const ALL: [Self; 3] = [Self::Overworld, Self::Nether, Self::End];
    pub fn name(self) -> &'static str {
        match self {
            Self::Overworld => "Overworld",
            Self::Nether => "Nether",
            Self::End => "The End",
        }
    }
}

/// World options a seed map needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WorldSpec {
    pub seed: i64,
    pub dimension: Dimension,
    pub large_biomes: bool,
    pub amplified: bool,
}
impl WorldSpec {
    pub fn new(seed: i64, dimension: Dimension) -> Self {
        Self {
            seed,
            dimension,
            large_biomes: false,
            amplified: false,
        }
    }
    pub fn preset(self) -> Preset {
        match self.dimension {
            Dimension::Nether => Preset::Nether,
            Dimension::End => Preset::End,
            Dimension::Overworld if self.large_biomes => Preset::LargeBiomes,
            Dimension::Overworld if self.amplified => Preset::Amplified,
            Dimension::Overworld => Preset::Overworld,
        }
    }
}

/// One seed and dimension, compiled. Cheap to share between threads.
pub struct World {
    pub spec: WorldSpec,
    pub data: &'static Data,
    climate: [Arc<dyn DensitySampler>; 6],
    surface: OnceLock<Option<Arc<dyn DensitySampler>>>,
    compiler: Mutex<DensityCompiler>,
    final_density: OnceLock<Option<Arc<dyn DensitySampler>>>,
    spawn: OnceLock<[i32; 2]>,
}
impl std::fmt::Debug for World {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("World").field("spec", &self.spec).finish()
    }
}

impl World {
    pub fn new(spec: WorldSpec) -> Arc<Self> {
        let data = data();
        let settings = data.settings(spec.preset());
        let mut compiler =
            DensityCompiler::new_seeded(settings.graph.clone(), spec.seed, settings.legacy_random);
        // Vanilla's binding order: the six climate roots first.
        let climate = std::array::from_fn(|i| {
            compiler
                .get_sampler(settings.router.function(RouterField::ALL[i]))
                .expect("climate root compiles")
        });
        Arc::new(Self {
            spec,
            data,
            climate,
            surface: OnceLock::new(),
            compiler: Mutex::new(compiler),
            final_density: OnceLock::new(),
            spawn: OnceLock::new(),
        })
    }

    pub fn seed(&self) -> i64 {
        self.spec.seed
    }
    pub fn sea_level(&self) -> i32 {
        self.data.settings(self.spec.preset()).sea_level
    }
    fn index(&self) -> Option<&'static ClimateIndex<BiomeId>> {
        match self.spec.dimension {
            Dimension::Overworld => Some(&self.data.overworld),
            Dimension::Nether => Some(&self.data.nether),
            Dimension::End => None,
        }
    }
    fn compile(&self, id: FunctionId) -> Option<Arc<dyn DensitySampler>> {
        self.compiler.lock().ok()?.get_sampler(id).ok()
    }
    /// The overworld's preliminary surface level sampler, compiled on first use.
    fn surface(&self) -> Option<&Arc<dyn DensitySampler>> {
        self.surface
            .get_or_init(|| {
                let preset = self.spec.preset();
                if self.spec.dimension != Dimension::Overworld {
                    return None;
                }
                let name = match preset {
                    Preset::Amplified => "minecraft:overworld_amplified/preliminary_surface_level",
                    Preset::LargeBiomes => {
                        "minecraft:overworld_large_biomes/preliminary_surface_level"
                    }
                    _ => "minecraft:overworld/preliminary_surface_level",
                };
                let settings = self.data.settings(preset);
                let id = settings.graph.function(name).or_else(|| {
                    settings
                        .graph
                        .function("minecraft:overworld/preliminary_surface_level")
                })?;
                self.compile(id)
            })
            .as_ref()
    }
    fn final_density(&self) -> Option<&Arc<dyn DensitySampler>> {
        self.final_density
            .get_or_init(|| {
                let settings = self.data.settings(self.spec.preset());
                self.compile(settings.router.function(RouterField::FinalDensity))
            })
            .as_ref()
    }

    /// A per-thread query state (density caches and the climate search's
    /// previous-leaf memory, as one vanilla worker has).
    pub fn sampler(&self) -> Sampler<'_> {
        Sampler {
            world: self,
            context: SamplerContext::cached(),
            search: self.index().map(ClimateIndex::searcher),
            exact_heights: true,
        }
    }

    /// NoiseSpawnFinder.findSpawnPosition: where the world spawn search
    /// starts (the player spawns at the first good block spiralling out of
    /// this column). Overworld only.
    pub fn spawn(&self) -> [i32; 2] {
        *self.spawn.get_or_init(|| {
            let settings = self.data.settings(self.spec.preset());
            if settings.spawn_target.is_empty() {
                return [0, 0];
            }
            let targets: Vec<SpawnTargetPoint> = settings
                .spawn_target
                .iter()
                .map(|point| SpawnTargetPoint {
                    parameters: point
                        .iter()
                        .filter_map(|(id, p)| Some((self.compile(*id)?, *p)))
                        .collect(),
                })
                .collect();
            find_spawn_position(&targets, &mut SamplerContext::cached()).unwrap_or([0, 0])
        })
    }
}

pub struct Sampler<'w> {
    world: &'w World,
    context: SamplerContext,
    search: Option<ClimateSearch<'static, BiomeId>>,
    /// Find overworld heights by scanning the final density (slower,
    /// closer) rather than trusting the preliminary surface estimate.
    pub exact_heights: bool,
}
impl<'w> Sampler<'w> {
    pub fn world(&self) -> &'w World {
        self.world
    }

    /// The noise biome at a quart position (block >> 2), as the biome
    /// source answers it: what chunk biome storage is filled from.
    pub fn biome(&mut self, quart: [i32; 3]) -> BiomeId {
        match self.world.spec.dimension {
            Dimension::End => self.end_biome(quart),
            _ => {
                let position = quart.map(|v| v.wrapping_shl(2));
                let mut values = [0f32; 6];
                for (s, v) in self.world.climate.iter().zip(&mut values) {
                    *v = s.sample_value(&mut self.context, position).unwrap_or(0.0);
                }
                self.find(values)
            }
        }
    }
    fn find(&mut self, values: [f32; 6]) -> BiomeId {
        let target = TargetPoint::from_values(values);
        self.search
            .as_mut()
            .and_then(|s| s.find(target).ok().copied())
            .unwrap_or_default()
    }
    /// Biome at a block position (its quart; no fuzzy zoom).
    pub fn biome_at(&mut self, x: i32, y: i32, z: i32) -> BiomeId {
        self.biome([x >> 2, y >> 2, z >> 2])
    }

    fn end_biome(&mut self, quart: [i32; 3]) -> BiomeId {
        let data = self.world.data;
        let id = |name: &str| data.biome(name).unwrap_or_default();
        let block = quart.map(|v| v.wrapping_shl(2));
        let (x, z) = (block[0] >> 4, block[2] >> 4);
        if i64::from(x) * i64::from(x) + i64::from(z) * i64::from(z) <= 4096 {
            return id("the_end");
        }
        let position = [
            x.wrapping_mul(2).wrapping_add(1).wrapping_mul(8),
            block[1],
            z.wrapping_mul(2).wrapping_add(1).wrapping_mul(8),
        ];
        let erosion = self.world.climate[3]
            .sample_value(&mut self.context, position)
            .unwrap_or(0.0);
        if erosion > 0.25 {
            id("end_highlands")
        } else if erosion >= -0.0625 {
            id("end_midlands")
        } else if erosion < -0.21875 {
            id("small_end_islands")
        } else {
            id("end_barrens")
        }
    }

    /// A grid of noise biomes at quart height `qy`: `w` × `h` samples from
    /// quart (`qx`, `qz`), `step` quarts apart, row by row. Climate fields
    /// are sampled in bulk, which is several times faster than one by one.
    #[allow(clippy::needless_range_loop)]
    pub fn biome_grid(
        &mut self,
        qx: i32,
        qz: i32,
        w: usize,
        h: usize,
        step: i32,
        qy: i32,
        out: &mut Vec<BiomeId>,
    ) {
        out.clear();
        if self.world.spec.dimension == Dimension::End {
            for j in 0..h as i32 {
                for i in 0..w as i32 {
                    let b = self.end_biome([qx + i * step, qy, qz + j * step]);
                    out.push(b);
                }
            }
            return;
        }
        let Ok(volume) = DensityVolume::new(
            [w as i32, 1, h as i32],
            [qx.wrapping_shl(2), qy.wrapping_shl(2), qz.wrapping_shl(2)],
            [step * 4, 4, step * 4],
        ) else {
            return;
        };
        let mut fields: [Vec<f32>; 6] = std::array::from_fn(|_| vec![0f32; volume.size()]);
        for (s, f) in self.world.climate.iter().zip(&mut fields) {
            if s.sample_volume(&mut self.context, f, volume).is_err() {
                // Fall back to scalar sampling for a sampler without bulk support.
                for j in 0..h {
                    for i in 0..w {
                        let p = [
                            (qx + i as i32 * step) << 2,
                            qy << 2,
                            (qz + j as i32 * step) << 2,
                        ];
                        f[j * w + i] = s.sample_value(&mut self.context, p).unwrap_or(0.0);
                    }
                }
            }
        }
        for i in 0..volume.size() {
            let values: [f32; 6] = std::array::from_fn(|k| fields[k][i]);
            out.push(self.find(values));
        }
    }

    /// Like [`Self::biome_grid`], but each column's biome is taken at its
    /// estimated surface (the preliminary surface level, on the quart
    /// lattice): what a player standing there sees. Five climate fields do
    /// not depend on height and are sampled in bulk; only depth is sampled
    /// per column. Other dimensions fall back to height `qy`.
    pub fn biome_grid_surface(
        &mut self,
        qx: i32,
        qz: i32,
        w: usize,
        h: usize,
        step: i32,
        qy: i32,
        out: &mut Vec<BiomeId>,
    ) {
        let Some(surface) = self.world.surface().cloned() else {
            return self.biome_grid(qx, qz, w, h, step, qy, out);
        };
        out.clear();
        let Ok(volume) = DensityVolume::new(
            [w as i32, 1, h as i32],
            [qx.wrapping_shl(2), 0, qz.wrapping_shl(2)],
            [step * 4, 4, step * 4],
        ) else {
            return;
        };
        let mut fields: [Vec<f32>; 7] = std::array::from_fn(|_| vec![0f32; volume.size()]);
        let mut samplers: Vec<Arc<dyn DensitySampler>> = self.world.climate.to_vec();
        samplers.push(surface);
        for (k, (s, f)) in samplers.iter().zip(&mut fields).enumerate() {
            if k == 4 {
                continue; // depth: per column below
            }
            if s.sample_volume(&mut self.context, f, volume).is_err() {
                for j in 0..h {
                    for i in 0..w {
                        let p = [(qx + i as i32 * step) << 2, 0, (qz + j as i32 * step) << 2];
                        f[j * w + i] = s.sample_value(&mut self.context, p).unwrap_or(0.0);
                    }
                }
            }
        }
        let depth = &self.world.climate[4];
        for j in 0..h {
            for i in 0..w {
                let n = j * w + i;
                let x = (qx + i as i32 * step) << 2;
                let z = (qz + j as i32 * step) << 2;
                let y = ((fields[6][n].floor() as i32) >> 2) << 2;
                fields[4][n] = depth
                    .sample_value(&mut self.context, [x, y, z])
                    .unwrap_or(0.0);
                let values = std::array::from_fn(|k| fields[k][n]);
                let b = self.find(values);
                out.push(b);
            }
        }
    }

    /// The overworld's preliminary surface level at a block column: the
    /// terrain height estimate the generator itself uses (aquifers, surface
    /// rules). Within a few blocks of the real surface on land.
    pub fn surface_estimate(&mut self, x: i32, z: i32) -> i32 {
        match self.world.surface() {
            Some(s) => s
                .sample_value(&mut self.context, [x, 0, z])
                .map(|v| v.floor() as i32)
                .unwrap_or(self.world.sea_level()),
            None => self.world.sea_level(),
        }
    }

    /// Whether the noise terrain is solid at a block (final density > 0).
    pub fn solid(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        let density = self.world.final_density()?.clone();
        Some(density.sample_value(&mut self.context, [x, y, z]).ok()? > 0.0)
    }

    /// The highest block whose final density is solid (> 0) in a column,
    /// scanning down from `top` in `step`-block steps then refining: the
    /// noise terrain height before surface rules and carvers.
    pub fn terrain_height(&mut self, x: i32, z: i32, top: i32, bottom: i32) -> Option<i32> {
        let density = self.world.final_density()?.clone();
        let mut y = top;
        let step = 4;
        while y >= bottom {
            let d = density.sample_value(&mut self.context, [x, y, z]).ok()?;
            if d > 0.0 {
                let mut refine = y + step - 1;
                while refine > y {
                    if density
                        .sample_value(&mut self.context, [x, refine, z])
                        .ok()?
                        > 0.0
                    {
                        return Some(refine);
                    }
                    refine -= 1;
                }
                return Some(y);
            }
            y -= step;
        }
        None
    }
}
