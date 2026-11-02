//! Run every seed-finder condition over many random seeds and report any
//! panic with the seed and condition that caused it.
use seedmap_core::data;
use seedmap_core::search::{Condition, Progress, SearchConfig, SeedSource, check_seed};
use seedmap_core::structures::{BiomeMask, catalog};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};

fn main() {
    let n: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(2000);
    let mut conds: Vec<Condition> = Vec::new();
    for set in &catalog().sets {
        if set.spacing.is_some() {
            conds.push(Condition::Structure {
                set: set.index,
                count: 1,
                radius: 2000,
            });
            conds.push(Condition::Cluster {
                set: set.index,
                count: 2,
                span: 400,
                radius: 2000,
            });
        }
    }
    let mut all = BiomeMask::default();
    for i in 0..data().biome_names.len() {
        all.insert(seedmap_core::BiomeId(i as u8));
    }
    conds.push(Condition::Biome {
        biomes: BiomeMask(1 << 3),
        radius: 1000,
    });
    conds.push(Condition::SpawnBiome { biomes: all });
    conds.push(Condition::Stronghold { radius: 3000 });
    let panics = AtomicU64::new(0);
    let next = AtomicU64::new(0);
    std::panic::set_hook(Box::new(|_| {}));
    std::thread::scope(|s| {
        for t in 0..std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
        {
            let (conds, panics, next) = (&conds, &panics, &next);
            s.spawn(move || {
                let mut x = 0x9E37_79B9_7F4A_7C15u64.wrapping_mul(t as u64 + 1);
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= n {
                        break;
                    }
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    let seed = x as i64;
                    for c in conds {
                        let config = SearchConfig {
                            conditions: vec![c.clone()],
                            source: SeedSource::Random,
                            threads: 1,
                            large_biomes: std::env::var("LARGE").is_ok(),
                            max_results: 1,
                        };
                        let progress = Progress::default();
                        let r =
                            catch_unwind(AssertUnwindSafe(|| check_seed(seed, &config, &progress)));
                        if let Err(e) = r {
                            let msg = e
                                .downcast_ref::<String>()
                                .cloned()
                                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                                .unwrap_or_default();
                            println!("PANIC seed {seed} {c:?}: {msg}");
                            panics.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            });
        }
    });
    println!(
        "done: {n} seeds × {} conditions, {} panics",
        conds.len(),
        panics.load(Ordering::Relaxed)
    );
}
