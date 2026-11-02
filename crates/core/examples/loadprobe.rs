fn main() {
    let kb: usize = std::env::args().nth(1).unwrap().parse().unwrap();
    std::thread::Builder::new()
        .stack_size(kb * 1024)
        .spawn(|| {
            let _ = seedmap_core::data();
            let _ = seedmap_core::structures::catalog();
            let w = seedmap_core::World::new(seedmap_core::WorldSpec::new(
                1,
                seedmap_core::Dimension::Overworld,
            ));
            let mut s = w.sampler();
            let _ = s.terrain_height(0, 0, 320, -64);
            let _ = seedmap_core::structures::strongholds(&w);
        })
        .unwrap()
        .join()
        .unwrap();
    println!("ok at {kb} KiB");
}
