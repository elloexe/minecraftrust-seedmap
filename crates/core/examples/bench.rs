use seedmap_core::*;
use std::time::Instant;
fn main() {
    let t = Instant::now();
    let d = data();
    println!("data {:?} biomes={}", t.elapsed(), d.biome_names.len());
    let seed: i64 = std::env::args()
        .nth(1)
        .map(|s| s.parse().unwrap())
        .unwrap_or(1234);
    let t = Instant::now();
    let w = World::new(WorldSpec::new(seed, Dimension::Overworld));
    println!("world {:?}", t.elapsed());
    let mut s = w.sampler();
    let t = Instant::now();
    let mut n = 0;
    for z in 0..100 {
        for x in 0..100 {
            let _ = s.biome([x * 4, 16, z * 4]);
            n += 1;
        }
    }
    println!("scalar {:?}/sample", t.elapsed() / n);
    let mut out = Vec::new();
    let t = Instant::now();
    s.biome_grid(0, 0, 256, 256, 1, 16, &mut out);
    println!("grid {:?}/sample", t.elapsed() / (256 * 256));
    println!("origin: {}", d.biome_name(s.biome([0, 16, 0])));
    let t = Instant::now();
    println!("spawn {:?} in {:?}", w.spawn(), t.elapsed());
    let t = Instant::now();
    for x in [0, 100, 500] {
        println!(
            "surface est {} terrain {:?}",
            s.surface_estimate(x, 0),
            s.terrain_height(x, 0, 320, -64)
        );
    }
    println!("heights {:?}", t.elapsed());
}
