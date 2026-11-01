The first release: a seed map and structure finder for **Minecraft Rust 26.3** (and vanilla Java 26.3).

### Download
- **Windows:** `minecraftrust-seedmap-windows-x64.exe`. It's a single file and nothing needs installing. If SmartScreen warns you, choose *More info → Run anyway*.
- **Linux:** `minecraftrust-seedmap-linux-x64`. Run `chmod +x` on it first.

### What's in it
- Biome maps for the Overworld, Nether and End, plus Large Biomes, at the surface or any Y level (so cave biomes show up)
- All 21 structure sets of 26.3, including the new **Abandoned Camp**, all 128 strongholds and the exact world spawn
- New 26.x biomes: **Dappled Forest** and **Sulfur Caves**
- A multi-threaded seed finder. Stage 1 checks structure placement only, at about 200 M region tests/s per thread, then candidates get real biome and terrain checks. It has presets for quad witch huts, a village at spawn, mushroom islands and more.
- Slime chunks, a coordinate grid, biome highlight, copy `/tp`, and text seeds

### Accuracy (checked against the official 26.3 game classes)
- Noise biomes: 46,818 / 46,818
- World spawn: 4 / 4, exact to the block
- Strongholds: 512 / 512
- Structure starts: 3,031 / 3,036 found (99.8%)

See the README for details.
