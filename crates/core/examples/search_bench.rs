use seedmap_core::search::*;
use seedmap_core::structures::catalog;
use std::sync::atomic::Ordering;
fn main() {
    let set = |n: &str| catalog().set(n).unwrap().index;
    let cases: Vec<(&str, Vec<Condition>)> = vec![
        (
            "stage 1 only: 4 villages within 300",
            vec![Condition::Structure {
                set: set("villages"),
                count: 4,
                radius: 300,
            }],
        ),
        (
            "2 villages + 2 abandoned camps within 250",
            vec![
                Condition::Structure {
                    set: set("villages"),
                    count: 2,
                    radius: 250,
                },
                Condition::Structure {
                    set: set("abandoned_camp"),
                    count: 2,
                    radius: 250,
                },
            ],
        ),
        (
            "mansion + monument + 2 villages within 400",
            vec![
                Condition::Structure {
                    set: set("woodland_mansions"),
                    count: 1,
                    radius: 400,
                },
                Condition::Structure {
                    set: set("ocean_monuments"),
                    count: 1,
                    radius: 400,
                },
                Condition::Structure {
                    set: set("villages"),
                    count: 2,
                    radius: 400,
                },
            ],
        ),
        (
            "3 witch huts within 200 of each other, 3000 radius",
            vec![Condition::Cluster {
                set: set("swamp_huts"),
                count: 3,
                span: 200,
                radius: 3000,
            }],
        ),
    ];
    // Raw stage one: placement of one region set per seed, one thread.
    let spread = catalog().set("villages").unwrap().spread.unwrap();
    let t = std::time::Instant::now();
    let mut hits = 0u64;
    let n = 50_000_000i64;
    for seed in 0..n {
        let (x, z) = spread.chunk(seed, 0, 0);
        if x < 4 && z < 4 {
            hits += 1;
        }
    }
    let per = t.elapsed().as_secs_f64() / n as f64;
    println!(
        "raw stage 1 (one region test): {:.1} M seeds/s per thread ({hits} hits)",
        1e-6 / per
    );
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    for (name, conditions) in cases {
        let s = start(SearchConfig {
            conditions,
            source: SeedSource::Random,
            threads,
            large_biomes: false,
            max_results: 100000,
        });
        std::thread::sleep(std::time::Duration::from_secs(6));
        s.stop();
        let rate = s.seeds_per_second();
        let p = s.progress.clone();
        s.join();
        println!(
            "{name}: {:.2} M seeds/s on {threads} threads; stage1 {} found {}",
            rate / 1e6,
            p.stage_one.load(Ordering::Relaxed),
            p.found.load(Ordering::Relaxed)
        );
        for h in p.results.lock().unwrap().iter().take(2) {
            println!("   {} {:?}", h.seed, h.notes);
        }
    }
}
