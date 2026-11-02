//! Seed finder: millions of seeds a second through staged filters.
//!
//! Every condition is checked cheapest first. Stage one is structure
//! placement alone (one random draw per region, no world needed), which is
//! where nearly every seed is thrown out. Survivors get a compiled world and
//! the real checks: the structure's biome and terrain rules, then biome
//! conditions, then the spawn point.
use crate::biomes::BiomeId;
use crate::structures::{BiomeMask, candidates, catalog, strongholds_near};
use crate::world::{Dimension, World, WorldSpec};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone, Debug, PartialEq)]
pub enum Condition {
    /// At least `count` of the set's structures within `radius` blocks of
    /// the origin (chebyshev, as chunk squares are searched).
    Structure { set: usize, count: u32, radius: i32 },
    /// `count` of the set's structures all within `span` blocks of each
    /// other, somewhere within `radius` of the origin (quad witch huts:
    /// count 4, span 256 or so).
    Cluster {
        set: usize,
        count: u32,
        span: i32,
        radius: i32,
    },
    /// One of the biomes within `radius` blocks of the origin, sampled on a
    /// 16-block grid at sea level.
    Biome { biomes: BiomeMask, radius: i32 },
    /// The world spawn search starts in one of these biomes.
    SpawnBiome { biomes: BiomeMask },
    /// The nearest stronghold within `radius` blocks of the origin.
    Stronghold { radius: i32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeedSource {
    /// Seeds counting up from `start`.
    Sequential { start: i64 },
    /// Random 64-bit seeds.
    Random,
    /// Seeds that typed text can give (32-bit, String.hashCode range).
    Random32,
}

#[derive(Clone, Debug)]
pub struct SearchConfig {
    pub conditions: Vec<Condition>,
    pub source: SeedSource,
    pub threads: usize,
    pub large_biomes: bool,
    /// Stop after this many results.
    pub max_results: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub seed: i64,
    /// A line per condition about what was found.
    pub notes: Vec<String>,
}

#[derive(Default)]
pub struct Progress {
    pub checked: AtomicU64,
    pub stage_one: AtomicU64,
    pub found: AtomicU64,
    pub stop: AtomicBool,
    pub done: AtomicBool,
    /// Seeds whose check failed with an internal error, and the first few
    /// messages: a bad seed is skipped instead of ending the search.
    pub skipped: AtomicU64,
    pub errors: Mutex<Vec<String>>,
    /// Milliseconds the search ran, once it has stopped.
    pub elapsed_ms: AtomicU64,
    pub results: Mutex<Vec<Hit>>,
}

pub struct Search {
    pub progress: Arc<Progress>,
    pub started: Instant,
    handles: Vec<std::thread::JoinHandle<()>>,
}
impl Search {
    pub fn stop(&self) {
        self.progress.stop.store(true, Ordering::Relaxed);
    }
    pub fn is_running(&self) -> bool {
        !self.progress.done.load(Ordering::Relaxed)
    }
    /// Time spent searching (frozen once the search stops).
    pub fn elapsed(&self) -> std::time::Duration {
        if self.is_running() {
            self.started.elapsed()
        } else {
            std::time::Duration::from_millis(self.progress.elapsed_ms.load(Ordering::Relaxed))
        }
    }
    pub fn seeds_per_second(&self) -> f64 {
        self.progress.checked.load(Ordering::Relaxed) as f64
            / self.elapsed().as_secs_f64().max(1e-3)
    }
    pub fn join(self) {
        for h in self.handles {
            let _ = h.join();
        }
    }
}

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Start a search on its own threads.
pub fn start(config: SearchConfig) -> Search {
    let progress = Arc::new(Progress::default());
    let config = Arc::new(config);
    let next = Arc::new(AtomicU64::new(0));
    let threads = config.threads.max(1);
    let entropy = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    let alive = Arc::new(AtomicU64::new(threads as u64));
    let started = Instant::now();
    let handles = (0..threads)
        .map(|t| {
            let (progress, config, next, alive) = (
                progress.clone(),
                config.clone(),
                next.clone(),
                alive.clone(),
            );
            std::thread::Builder::new()
                .name(format!("seed search {t}"))
                .spawn(move || {
                    // Searching yields to the window and the map.
                    let _ = thread_priority::set_current_thread_priority(
                        thread_priority::ThreadPriority::Min,
                    );
                    let mut rng = entropy ^ (t as u64).wrapping_mul(0xA24B_AED4_963E_E407);
                    const BATCH: u64 = 4096;
                    while !progress.stop.load(Ordering::Relaxed) {
                        let base = next.fetch_add(BATCH, Ordering::Relaxed);
                        let mut done = BATCH;
                        for i in 0..BATCH {
                            if progress.stop.load(Ordering::Relaxed) {
                                done = i;
                                break;
                            }
                            let seed = match config.source {
                                SeedSource::Sequential { start } => {
                                    start.wrapping_add((base + i) as i64)
                                }
                                SeedSource::Random => splitmix(&mut rng) as i64,
                                SeedSource::Random32 => i64::from(splitmix(&mut rng) as i32),
                            };
                            let checked =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    check_seed(seed, &config, &progress)
                                }));
                            let hit = match checked {
                                Ok(hit) => hit,
                                Err(panic) => {
                                    let message = panic
                                        .downcast_ref::<String>()
                                        .cloned()
                                        .or_else(|| {
                                            panic.downcast_ref::<&str>().map(|s| s.to_string())
                                        })
                                        .unwrap_or_else(|| "unknown error".into());
                                    progress.skipped.fetch_add(1, Ordering::Relaxed);
                                    if let Ok(mut errors) = progress.errors.lock()
                                        && errors.len() < 10
                                    {
                                        errors.push(format!("seed {seed}: {message}"));
                                    }
                                    None
                                }
                            };
                            if let Some(hit) = hit {
                                let mut results = progress
                                    .results
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                                if results.len() < config.max_results {
                                    results.push(hit);
                                    progress.found.fetch_add(1, Ordering::Relaxed);
                                }
                                if results.len() >= config.max_results {
                                    progress.stop.store(true, Ordering::Relaxed);
                                }
                            }
                        }
                        progress.checked.fetch_add(done, Ordering::Relaxed);
                    }
                    if alive.fetch_sub(1, Ordering::AcqRel) == 1 {
                        progress
                            .elapsed_ms
                            .store(started.elapsed().as_millis() as u64, Ordering::Relaxed);
                        progress.done.store(true, Ordering::Release);
                    }
                })
                .expect("search thread")
        })
        .collect();
    Search {
        progress,
        started,
        handles,
    }
}

fn chunk_box(radius: i32) -> (i32, i32) {
    ((-radius).div_euclid(16), radius.div_euclid(16))
}

/// Placement-only candidates of a structure condition (stage one).
fn placed(seed: i64, set: usize, radius: i32) -> Vec<(i32, i32)> {
    let (lo, hi) = chunk_box(radius);
    candidates(seed, set, lo, lo, hi, hi)
}

fn cluster_of(points: &[(i32, i32)], count: u32, span_chunks: i32) -> Option<Vec<(i32, i32)>> {
    let count = count as usize;
    if points.len() < count {
        return None;
    }
    for (i, a) in points.iter().enumerate() {
        let near: Vec<(i32, i32)> = points[i..]
            .iter()
            .filter(|b| (a.0 - b.0).abs() <= span_chunks && (a.1 - b.1).abs() <= span_chunks)
            .copied()
            .collect();
        if near.len() >= count {
            // Every pair must fit the span, not just pairs with `a`.
            let mut group = vec![*a];
            for b in &near[1..] {
                if group
                    .iter()
                    .all(|g| (g.0 - b.0).abs() <= span_chunks && (g.1 - b.1).abs() <= span_chunks)
                {
                    group.push(*b);
                }
            }
            if group.len() >= count {
                group.truncate(count);
                return Some(group);
            }
        }
    }
    None
}

/// Stage one for "at least `count` within `radius`": no allocation, and it
/// stops at the `count`th candidate.
fn enough_placed(seed: i64, set: usize, radius: i32, count: u32) -> bool {
    let (lo, hi) = chunk_box(radius);
    match catalog().sets[set].spread {
        Some(spread) => {
            let mut n = 0;
            for gz in lo.div_euclid(spread.spacing)..=hi.div_euclid(spread.spacing) {
                for gx in lo.div_euclid(spread.spacing)..=hi.div_euclid(spread.spacing) {
                    let (cx, cz) = spread.chunk(seed, gx, gz);
                    if cx >= lo && cx <= hi && cz >= lo && cz <= hi {
                        n += 1;
                        if n >= count {
                            return true;
                        }
                    }
                }
            }
            false
        }
        None => placed(seed, set, radius).len() as u32 >= count,
    }
}

/// Check one seed against every condition, cheapest stage first.
pub fn check_seed(seed: i64, config: &SearchConfig, progress: &Progress) -> Option<Hit> {
    // Stage one: placement alone.
    for c in &config.conditions {
        match *c {
            Condition::Structure { set, count, radius }
                if !enough_placed(seed, set, radius, count) =>
            {
                return None;
            }
            Condition::Stronghold { radius }
                if !crate::structures::stronghold_may_be_near(seed, radius) =>
            {
                return None;
            }
            _ => {}
        }
    }
    let mut stage: Vec<Vec<(i32, i32)>> = Vec::with_capacity(config.conditions.len());
    for c in &config.conditions {
        match *c {
            Condition::Structure { set, radius, .. } => stage.push(placed(seed, set, radius)),
            Condition::Cluster {
                set,
                count,
                span,
                radius,
            } => {
                let p = placed(seed, set, radius);
                cluster_of(&p, count, span.div_euclid(16) + 1)?;
                stage.push(p);
            }
            _ => stage.push(Vec::new()),
        }
    }
    progress.stage_one.fetch_add(1, Ordering::Relaxed);

    // Stage two: the world itself.
    let mut spec = WorldSpec::new(seed, Dimension::Overworld);
    let needs_world = |c: &Condition| match c {
        Condition::Structure { set, .. } | Condition::Cluster { set, .. } => {
            catalog().sets[*set].dimensions.first().copied()
        }
        _ => Some(Dimension::Overworld),
    };
    let mut worlds: Vec<(Dimension, Arc<World>)> = Vec::new();
    let mut world_for = |d: Dimension| -> Arc<World> {
        if let Some((_, w)) = worlds.iter().find(|(wd, _)| *wd == d) {
            return w.clone();
        }
        spec.dimension = d;
        spec.large_biomes = config.large_biomes;
        let w = World::new(spec);
        worlds.push((d, w.clone()));
        w
    };
    let mut notes = Vec::new();
    for (c, placed) in config.conditions.iter().zip(&stage) {
        match *c {
            Condition::Structure { set, count, .. } => {
                let world = world_for(needs_world(c).unwrap_or(Dimension::Overworld));
                // Estimated heights first (fast); a pass is confirmed exactly.
                let mut sampler = world.sampler();
                sampler.exact_heights = false;
                let mut exact = world.sampler();
                let mut found = Vec::new();
                let mut sorted = placed.clone();
                sorted.sort_by_key(|(x, z)| {
                    i64::from(*x) * i64::from(*x) + i64::from(*z) * i64::from(*z)
                });
                for (cx, cz) in sorted {
                    if progress.stop.load(Ordering::Relaxed) {
                        return None;
                    }
                    if sampler.validate(set, cx, cz).is_none() {
                        continue;
                    }
                    if let Some(f) = exact.validate(set, cx, cz) {
                        found.push(f.pos);
                        if found.len() as u32 >= count {
                            break;
                        }
                    }
                }
                if (found.len() as u32) < count {
                    return None;
                }
                let label = catalog().sets[set].label;
                notes.push(format!(
                    "{label}: {}",
                    found
                        .iter()
                        .map(|p| format!("{} {}", p[0], p[2]))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            Condition::Cluster {
                set, count, span, ..
            } => {
                let world = world_for(needs_world(c).unwrap_or(Dimension::Overworld));
                let mut sampler = world.sampler();
                sampler.exact_heights = false;
                let mut exact = world.sampler();
                let valid: Vec<(i32, i32)> = placed
                    .iter()
                    .take_while(|_| !progress.stop.load(Ordering::Relaxed))
                    .filter(|(x, z)| {
                        sampler.validate(set, *x, *z).is_some()
                            && exact.validate(set, *x, *z).is_some()
                    })
                    .copied()
                    .collect();
                let group = cluster_of(&valid, count, span.div_euclid(16) + 1)?;
                let label = catalog().sets[set].label;
                notes.push(format!(
                    "{count}× {label} around {} {}",
                    group.iter().map(|g| g.0 * 16 + 8).sum::<i32>() / count as i32,
                    group.iter().map(|g| g.1 * 16 + 8).sum::<i32>() / count as i32
                ));
            }
            _ => {}
        }
    }
    for c in &config.conditions {
        match *c {
            Condition::Biome { biomes, radius } => {
                let world = world_for(Dimension::Overworld);
                let mut sampler = world.sampler();
                let q = (radius / 4).max(1);
                // Rings outward from the origin, 16 blocks apart.
                let mut hit = None;
                'rings: for r in (0..=q).step_by(4) {
                    if progress.stop.load(Ordering::Relaxed) {
                        return None;
                    }
                    for dz in (-r..=r).step_by(4) {
                        for dx in (-r..=r).step_by(4) {
                            if dx.abs() != r && dz.abs() != r {
                                continue;
                            }
                            let b = sampler.biome([dx, 16, dz]);
                            if biomes.contains(b) {
                                hit = Some((dx * 4, dz * 4, b));
                                break 'rings;
                            }
                        }
                    }
                }
                let (x, z, b) = hit?;
                notes.push(format!(
                    "{} at {x} {z}",
                    crate::biomes::display_name(world.data.biome_name(b))
                ));
            }
            Condition::SpawnBiome { biomes } => {
                let world = world_for(Dimension::Overworld);
                let [x, z] = world.spawn();
                let mut sampler = world.sampler();
                let y = sampler.heightmap(x, z, false);
                let b: BiomeId = sampler.biome_at(x, y, z);
                if !biomes.contains(b) {
                    return None;
                }
                notes.push(format!(
                    "Spawn {x} {z} in {}",
                    crate::biomes::display_name(world.data.biome_name(b))
                ));
            }
            Condition::Stronghold { radius } => {
                let world = world_for(Dimension::Overworld);
                let near = strongholds_near(&world, radius)
                    .into_iter()
                    .map(|f| f.pos)
                    .min_by_key(|p| i64::from(p[0]).pow(2) + i64::from(p[2]).pow(2))?;
                if near[0].abs() > radius || near[2].abs() > radius {
                    return None;
                }
                notes.push(format!("Stronghold at {} {}", near[0] + 4, near[2] + 4));
            }
            _ => {}
        }
    }
    Some(Hit { seed, notes })
}

/// Seed text as Minecraft reads it: a number, or else String.hashCode.
pub fn parse_seed(text: &str) -> i64 {
    let t = text.trim();
    if t.is_empty() {
        return 0;
    }
    if let Ok(v) = t.parse::<i64>() {
        return v;
    }
    let mut h: i32 = 0;
    // Java's String.hashCode over UTF-16 code units.
    for unit in t.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    i64::from(h)
}
