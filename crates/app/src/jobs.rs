//! Background workers: biome tiles, structure tiles, strongholds and spawn.
//!
//! The map asks for what it wants each frame, nearest the screen centre
//! first; the queue is replaced, so panning away drops work no one needs.
use seedmap_core::structures::{Found, candidates, strongholds};
use seedmap_core::{BiomeId, Dimension, World, WorldSpec};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};

pub const TILE: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WorldKey {
    pub seed: i64,
    pub dim: Dimension,
    pub large: bool,
}
impl WorldKey {
    pub fn spec(self) -> WorldSpec {
        let mut spec = WorldSpec::new(self.seed, self.dim);
        spec.large_biomes = self.large;
        spec
    }
}

/// How biomes are sampled vertically.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Height {
    Surface,
    Fixed(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub world: WorldKey,
    /// Texel size is 2^level blocks (2 = one quart).
    pub level: u8,
    pub tx: i32,
    pub tz: i32,
    pub height: Height,
}
impl TileKey {
    pub fn texel(self) -> i32 {
        1 << self.level
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StructKey {
    pub world: WorldKey,
    pub set: usize,
    pub sx: i32,
    pub sz: i32,
}

#[derive(Clone, Debug)]
pub struct Marker {
    pub chunk: (i32, i32),
    pub found: Option<Found>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Job {
    Biome(TileKey),
    Structures(StructKey),
    Strongholds(WorldKey),
    Spawn(WorldKey),
}

pub enum Done {
    Biome(TileKey, Vec<BiomeId>),
    Structures(StructKey, Vec<Marker>),
    Strongholds(WorldKey, Vec<Found>),
    Spawn(WorldKey, [i32; 2]),
}

/// Chunks per structure tile side for a set's region spacing.
pub fn struct_tile_chunks(spacing: Option<i32>) -> i32 {
    match spacing {
        Some(s) if s > 4 => (s * 6).max(32),
        _ => 32,
    }
}

struct Shared {
    queue: Mutex<VecDeque<Job>>,
    ready: Condvar,
    running: Mutex<HashSet<Job>>,
    worlds: Mutex<HashMap<WorldKey, Arc<World>>>,
    tx: Mutex<Sender<Done>>,
    ctx: egui::Context,
}

pub struct Pool {
    shared: Arc<Shared>,
    rx: Receiver<Done>,
    pub threads: usize,
}

impl Pool {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = channel();
        let shared = Arc::new(Shared {
            queue: Mutex::new(VecDeque::new()),
            ready: Condvar::new(),
            running: Mutex::new(HashSet::new()),
            worlds: Mutex::new(HashMap::new()),
            tx: Mutex::new(tx),
            ctx,
        });
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .saturating_sub(1)
            .max(2);
        for i in 0..threads {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name(format!("map worker {i}"))
                .spawn(move || worker(&shared))
                .expect("map worker");
        }
        Self {
            shared,
            rx,
            threads,
        }
    }

    pub fn world(&self, key: WorldKey) -> Arc<World> {
        world(&self.shared, key)
    }

    /// Replace the queue with these jobs, in order (skipping running ones).
    pub fn want(&self, jobs: Vec<Job>) {
        let running = self.shared.running.lock().unwrap().clone();
        let mut queue = self.shared.queue.lock().unwrap();
        queue.clear();
        queue.extend(jobs.into_iter().filter(|j| !running.contains(j)));
        if !queue.is_empty() {
            self.shared.ready.notify_all();
        }
    }

    pub fn busy(&self) -> usize {
        self.shared.running.lock().unwrap().len() + self.shared.queue.lock().unwrap().len()
    }

    pub fn finished(&self) -> Vec<Done> {
        self.rx.try_iter().collect()
    }
}

fn world(shared: &Shared, key: WorldKey) -> Arc<World> {
    let mut worlds = shared.worlds.lock().unwrap();
    if worlds.len() > 12 && !worlds.contains_key(&key) {
        worlds.clear();
    }
    worlds
        .entry(key)
        .or_insert_with(|| World::new(key.spec()))
        .clone()
}

fn worker(shared: &Shared) {
    loop {
        let job = {
            let mut queue = shared.queue.lock().unwrap();
            loop {
                if let Some(job) = queue.pop_front() {
                    break job;
                }
                queue = shared.ready.wait(queue).unwrap();
            }
        };
        if !shared.running.lock().unwrap().insert(job) {
            continue;
        }
        let done = run(shared, job);
        shared.running.lock().unwrap().remove(&job);
        let _ = shared.tx.lock().unwrap().send(done);
        shared.ctx.request_repaint();
    }
}

fn run(shared: &Shared, job: Job) -> Done {
    match job {
        Job::Biome(key) => {
            let w = world(shared, key.world);
            let mut sampler = w.sampler();
            let step = (key.texel() / 4).max(1);
            let span_q = TILE as i32 * step;
            let (qx, qz) = (key.tx * span_q, key.tz * span_q);
            // Sample texel centres.
            let half = step / 2;
            let mut out = Vec::with_capacity(TILE * TILE);
            match key.height {
                Height::Surface => {
                    sampler.biome_grid_surface(qx + half, qz + half, TILE, TILE, step, 16, &mut out)
                }
                Height::Fixed(y) => {
                    sampler.biome_grid(qx + half, qz + half, TILE, TILE, step, y >> 2, &mut out)
                }
            }
            Done::Biome(key, out)
        }
        Job::Structures(key) => {
            let w = world(shared, key.world);
            let info = &seedmap_core::structures::catalog().sets[key.set];
            let size = struct_tile_chunks(info.spacing);
            let (x0, z0) = (key.sx * size, key.sz * size);
            let cands = candidates(
                key.world.seed,
                key.set,
                x0,
                z0,
                x0 + size - 1,
                z0 + size - 1,
            );
            let mut sampler = w.sampler();
            let markers = cands
                .into_iter()
                .map(|(cx, cz)| Marker {
                    chunk: (cx, cz),
                    found: sampler.validate(key.set, cx, cz),
                })
                .collect();
            Done::Structures(key, markers)
        }
        Job::Strongholds(key) => {
            let w = world(shared, key);
            Done::Strongholds(key, strongholds(&w))
        }
        Job::Spawn(key) => {
            let w = world(shared, key);
            Done::Spawn(key, w.spawn())
        }
    }
}
