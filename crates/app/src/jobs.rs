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
/// Texels of terrain height around each tile (for smoothing and slopes).
pub const HEIGHT_PAD: usize = 3;

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
    /// Biomes, and terrain heights on a (TILE + 2·HEIGHT_PAD)² grid (empty: flat).
    Biome(TileKey, Vec<BiomeId>, Vec<i16>),
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
    /// Jobs that failed with an internal error: not retried every frame.
    failed: Mutex<HashSet<Job>>,
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
            failed: Mutex::new(HashSet::new()),
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
        let running = lock(&self.shared.running).clone();
        let failed = lock(&self.shared.failed).clone();
        let mut queue = lock(&self.shared.queue);
        queue.clear();
        queue.extend(
            jobs.into_iter()
                .filter(|j| !running.contains(j) && !failed.contains(j)),
        );
        if !queue.is_empty() {
            self.shared.ready.notify_all();
        }
    }

    pub fn busy(&self) -> usize {
        lock(&self.shared.running).len() + lock(&self.shared.queue).len()
    }

    pub fn finished(&self) -> Vec<Done> {
        self.rx.try_iter().collect()
    }
}

fn world(shared: &Shared, key: WorldKey) -> Arc<World> {
    let mut worlds = lock(&shared.worlds);
    if worlds.len() > 12 && !worlds.contains_key(&key) {
        worlds.clear();
    }
    worlds
        .entry(key)
        .or_insert_with(|| World::new(key.spec()))
        .clone()
}

/// A lock that survives a panic elsewhere (the data stays usable).
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn worker(shared: &Shared) {
    // Map work yields to the window, so the UI stays responsive.
    let _ = thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Min);
    loop {
        let job = {
            let mut queue = lock(&shared.queue);
            loop {
                if let Some(job) = queue.pop_front() {
                    break job;
                }
                queue = shared
                    .ready
                    .wait(queue)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        };
        if !lock(&shared.running).insert(job) {
            continue;
        }
        let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(shared, job)));
        lock(&shared.running).remove(&job);
        match done {
            Ok(done) => {
                let _ = lock(&shared.tx).send(done);
            }
            // Logged by the panic hook; the map shows the rest.
            Err(_) => {
                lock(&shared.failed).insert(job);
            }
        }
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
            let mut scratch = Vec::new();
            match key.height {
                Height::Surface => sampler.biome_grid_surface(
                    qx + half,
                    qz + half,
                    TILE,
                    TILE,
                    step,
                    16,
                    &mut out,
                    &mut scratch,
                ),
                Height::Fixed(y) => {
                    sampler.biome_grid(qx + half, qz + half, TILE, TILE, step, y >> 2, &mut out)
                }
            }
            // Terrain heights with a border, so shading is seamless across tiles.
            let mut heights = Vec::new();
            let pad = HEIGHT_PAD as i32 * step;
            sampler.surface_grid(
                qx + half - pad,
                qz + half - pad,
                TILE + 2 * HEIGHT_PAD,
                TILE + 2 * HEIGHT_PAD,
                step,
                &mut heights,
            );
            Done::Biome(key, out, heights)
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
