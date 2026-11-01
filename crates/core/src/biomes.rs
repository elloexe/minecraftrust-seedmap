//! Biome identities, display names and map colours.

/// Index into the bundle's biome list (registry order).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct BiomeId(pub u8);

/// Map colour of a biome, by identifier. The palette follows the colours
/// seed-map users know (ocean blues, desert sand, jungle greens), extended to
/// the biomes 26.x added.
pub fn color(name: &str) -> [u8; 3] {
    let path = name.strip_prefix("minecraft:").unwrap_or(name);
    match path {
        "ocean" => [0, 0, 112],
        "deep_ocean" => [0, 0, 48],
        "warm_ocean" => [0, 0, 172],
        "lukewarm_ocean" => [0, 0, 144],
        "deep_lukewarm_ocean" => [0, 0, 64],
        "cold_ocean" => [32, 32, 112],
        "deep_cold_ocean" => [32, 32, 56],
        "frozen_ocean" => [112, 112, 214],
        "deep_frozen_ocean" => [64, 64, 144],
        "river" => [0, 0, 255],
        "frozen_river" => [160, 160, 255],
        "beach" => [250, 222, 85],
        "snowy_beach" => [250, 240, 192],
        "stony_shore" => [162, 162, 132],
        "plains" => [141, 179, 96],
        "sunflower_plains" => [181, 219, 136],
        "snowy_plains" => [255, 255, 255],
        "ice_spikes" => [180, 220, 220],
        "desert" => [250, 148, 24],
        "swamp" => [7, 249, 178],
        "mangrove_swamp" => [44, 204, 142],
        "forest" => [5, 102, 33],
        "flower_forest" => [45, 142, 73],
        "birch_forest" => [48, 116, 68],
        "old_growth_birch_forest" => [88, 156, 108],
        "dark_forest" => [64, 81, 26],
        "pale_garden" => [105, 112, 95],
        "dappled_forest" => [74, 128, 48],
        "taiga" => [11, 102, 89],
        "snowy_taiga" => [49, 85, 74],
        "old_growth_pine_taiga" => [89, 102, 81],
        "old_growth_spruce_taiga" => [129, 142, 121],
        "savanna" => [189, 178, 95],
        "savanna_plateau" => [167, 157, 100],
        "windswept_savanna" => [229, 218, 135],
        "windswept_hills" => [96, 96, 96],
        "windswept_gravelly_hills" => [136, 136, 136],
        "windswept_forest" => [80, 112, 80],
        "jungle" => [83, 123, 9],
        "sparse_jungle" => [98, 139, 23],
        "bamboo_jungle" => [118, 142, 20],
        "badlands" => [217, 69, 21],
        "eroded_badlands" => [255, 109, 61],
        "wooded_badlands" => [176, 151, 101],
        "meadow" => [96, 164, 69],
        "cherry_grove" => [255, 145, 200],
        "grove" => [71, 114, 108],
        "snowy_slopes" => [196, 196, 196],
        "frozen_peaks" => [160, 160, 160],
        "jagged_peaks" => [220, 220, 200],
        "stony_peaks" => [123, 143, 116],
        "mushroom_fields" => [255, 0, 255],
        "dripstone_caves" => [78, 48, 18],
        "lush_caves" => [40, 60, 0],
        "deep_dark" => [3, 31, 41],
        "sulfur_caves" => [196, 186, 40],
        "nether_wastes" => [191, 59, 59],
        "soul_sand_valley" => [94, 56, 48],
        "crimson_forest" => [221, 8, 8],
        "warped_forest" => [73, 144, 123],
        "basalt_deltas" => [64, 54, 54],
        // The End: islands bright on a near-black void.
        "the_end" => [214, 210, 160],
        "end_highlands" => [226, 220, 150],
        "end_midlands" => [170, 164, 116],
        "small_end_islands" => [22, 20, 36],
        "end_barrens" => [72, 62, 104],
        "the_void" => [0, 0, 0],
        other => {
            // A stable colour for a biome this palette has not met.
            let mut h: u32 = 2_166_136_261;
            for b in other.bytes() {
                h = (h ^ u32::from(b)).wrapping_mul(16_777_619);
            }
            [(h >> 16) as u8 | 64, (h >> 8) as u8 | 64, h as u8 | 64]
        }
    }
}

/// "minecraft:old_growth_pine_taiga" -> "Old Growth Pine Taiga".
pub fn display_name(name: &str) -> String {
    let path = name.rsplit(':').next().unwrap_or(name);
    path.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Biomes added in the 26.x releases, which older seed tools do not know.
pub const NEW_IN_26: [&str; 2] = ["minecraft:dappled_forest", "minecraft:sulfur_caves"];
