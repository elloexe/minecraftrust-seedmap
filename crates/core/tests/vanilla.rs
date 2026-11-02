//! Ground truth recorded from the official 26.3 game classes with
//! tools/oracle/SeedmapOracle.java. If one of these fails, the engine no
//! longer generates what Minecraft Rust and vanilla 26.3 generate.
use seedmap_core::search::parse_seed;
use seedmap_core::structures::{catalog, is_candidate, strongholds};
use seedmap_core::{Dimension, World, WorldSpec, data};

fn world(seed: i64, dim: Dimension) -> std::sync::Arc<World> {
    World::new(WorldSpec::new(seed, dim))
}

#[test]
fn noise_biomes_match_vanilla() {
    let w = world(8_675_309, Dimension::Overworld);
    let mut s = w.sampler();
    for (q, name) in [
        ([16, -8, -48], "minecraft:forest"),
        ([-168, -8, 112], "minecraft:deep_dark"),
        ([56, 16, -144], "minecraft:lukewarm_ocean"),
        ([-128, 16, 16], "minecraft:savanna"),
        ([96, 16, 168], "minecraft:ocean"),
        ([-88, 40, -80], "minecraft:plains"),
    ] {
        assert_eq!(data().biome_name(s.biome(q)), name, "quart {q:?}");
    }
    let nether = world(42, Dimension::Nether);
    let mut s = nether.sampler();
    assert_eq!(
        data().biome_name(s.biome([-40, -8, 32])),
        "minecraft:nether_wastes"
    );
}

#[test]
fn spawn_matches_vanilla() {
    for (seed, spawn) in [
        (1234, [-516, 77]),
        (42, [-31, 6]),
        (-4_172_144_997_902_289_642, [-443, -418]),
        (8_675_309, [18, 86]),
    ] {
        assert_eq!(
            world(seed, Dimension::Overworld).spawn(),
            spawn,
            "seed {seed}"
        );
    }
}

#[test]
fn stronghold_rings_match_vanilla() {
    let rings: Vec<(i32, i32)> = strongholds(&world(8_675_309, Dimension::Overworld))
        .iter()
        .map(|f| f.chunk)
        .collect();
    assert_eq!(rings.len(), 128);
    assert_eq!(&rings[..3], &[(2, -144), (86, 53), (-142, 70)]);
}

#[test]
fn structure_starts_match_vanilla() {
    let cat = catalog();
    let check = |seed: i64, dim: Dimension, set: &str, chunk: (i32, i32), variant: &str| {
        let info = cat.set(set).unwrap();
        assert!(
            is_candidate(seed, info.index, chunk.0, chunk.1),
            "{set} {chunk:?} placement"
        );
        let w = world(seed, dim);
        let found = w.sampler().validate(info.index, chunk.0, chunk.1);
        let found = found.unwrap_or_else(|| panic!("{set} {chunk:?} should generate"));
        assert_eq!(cat.structures[found.structure].name, variant);
    };
    check(
        8_675_309,
        Dimension::Overworld,
        "abandoned_camp",
        (-100, -35),
        "minecraft:abandoned_camp_forest",
    );
    check(
        8_675_309,
        Dimension::Overworld,
        "abandoned_camp",
        (-67, -101),
        "minecraft:abandoned_camp_wooded_badlands",
    );
    check(
        8_675_309,
        Dimension::Overworld,
        "abandoned_camp",
        (-65, 43),
        "minecraft:abandoned_camp_sparse_jungle",
    );
    check(
        42,
        Dimension::End,
        "end_cities",
        (-118, 64),
        "minecraft:end_city",
    );
    check(
        42,
        Dimension::Nether,
        "nether_complexes",
        (-120, 92),
        "minecraft:fortress",
    );
}

#[test]
fn text_seeds_hash_like_java() {
    assert_eq!(parse_seed("hello"), 99_162_322);
    assert_eq!(parse_seed("-42"), -42);
    assert_eq!(
        parse_seed("Minecraft Rust"),
        i64::from(
            "Minecraft Rust"
                .encode_utf16()
                .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(i32::from(c)))
        )
    );
}

#[test]
fn nearby_stronghold_search_agrees_with_the_full_rings() {
    use seedmap_core::structures::strongholds_near;
    for seed in [1234i64, 42, 8_675_309, -7] {
        let w = world(seed, Dimension::Overworld);
        let all = strongholds(&w);
        for radius in [1500, 2500, 4000] {
            let inside = |f: &&seedmap_core::structures::Found| {
                f.pos[0].abs() <= radius && f.pos[2].abs() <= radius
            };
            let mut full: Vec<_> = all.iter().filter(inside).map(|f| f.chunk).collect();
            let near = strongholds_near(&w, radius);
            let mut quick: Vec<_> = near.iter().filter(inside).map(|f| f.chunk).collect();
            full.sort();
            quick.sort();
            assert_eq!(full, quick, "seed {seed} radius {radius}");
            if !full.is_empty() {
                assert!(
                    seedmap_core::structures::stronghold_may_be_near(seed, radius),
                    "stage one rejected seed {seed} radius {radius}"
                );
            }
        }
    }
}
