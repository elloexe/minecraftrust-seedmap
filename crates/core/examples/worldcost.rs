use seedmap_core::*;
use std::time::Instant;
fn main() {
    let d = data();
    let st = d.settings(Preset::Overworld);
    let t = Instant::now();
    for _ in 0..100 {
        let g = st.graph.clone();
        std::hint::black_box(g);
    }
    println!("graph clone {:?}", t.elapsed() / 100);
    let t = Instant::now();
    for i in 0..100 {
        let w = World::new(WorldSpec::new(i, Dimension::Overworld));
        std::hint::black_box(w);
    }
    println!("world new {:?}", t.elapsed() / 100);
    let t = Instant::now();
    for i in 0..100 {
        let w = World::new(WorldSpec::new(i, Dimension::Overworld));
        let mut s = w.sampler();
        s.exact_heights = false;
        std::hint::black_box(s.heightmap(100, 100, false));
        std::hint::black_box(s.biome([25, 16, 25]));
    }
    println!("world + 1 biome + estimate {:?}", t.elapsed() / 100);
}
