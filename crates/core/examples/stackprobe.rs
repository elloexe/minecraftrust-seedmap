use seedmap_core::search::{Condition, Progress, SearchConfig, SeedSource, check_seed};
use seedmap_core::structures::{BiomeMask, catalog};
fn main() {
    let kb: usize = std::env::args().nth(1).unwrap().parse().unwrap();
    let _ = seedmap_core::data();
    let _ = catalog();
    let set = |n: &str| catalog().set(n).unwrap().index;
    let conds = vec![
        Condition::Structure {
            set: set("villages"),
            count: 1,
            radius: 1500,
        },
        Condition::Structure {
            set: set("ocean_monuments"),
            count: 1,
            radius: 1500,
        },
        Condition::Structure {
            set: set("woodland_mansions"),
            count: 1,
            radius: 3000,
        },
        Condition::Structure {
            set: set("end_cities"),
            count: 1,
            radius: 1500,
        },
        Condition::Structure {
            set: set("nether_fossils"),
            count: 1,
            radius: 300,
        },
        Condition::Biome {
            biomes: BiomeMask(u128::MAX),
            radius: 500,
        },
        Condition::SpawnBiome {
            biomes: BiomeMask(u128::MAX),
        },
    ];
    let h = std::thread::Builder::new()
        .stack_size(kb * 1024)
        .spawn(move || {
            for seed in 0..20i64 {
                for c in &conds {
                    let config = SearchConfig {
                        conditions: vec![c.clone()],
                        source: SeedSource::Random,
                        threads: 1,
                        large_biomes: false,
                        max_results: 1,
                    };
                    let _ = check_seed(seed, &config, &Progress::default());
                }
            }
        })
        .unwrap();
    h.join().unwrap();
    println!("ok at {kb} KiB");
}
