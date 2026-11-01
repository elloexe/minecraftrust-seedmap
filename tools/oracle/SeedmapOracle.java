import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.SharedConstants;
import net.minecraft.core.Holder;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.datafix.DataFixers;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterLists;
import net.minecraft.world.level.biome.TheEndBiomeSource;
import net.minecraft.world.level.chunk.ChunkGeneratorStructureState;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.NoiseSpawnFinder;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.WorldgenRandom;
import net.minecraft.world.level.levelgen.densityfunction.SamplerContext;
import net.minecraft.world.level.levelgen.structure.StructureSet;
import net.minecraft.world.level.levelgen.structure.StructureStart;
import net.minecraft.world.level.levelgen.structure.placement.ConcentricRingsStructurePlacement;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager;
import net.minecraft.world.level.storage.LevelStorageSource;

/**
 * Ground truth from the real 26.3 game classes, to check the seed map
 * against: noise biomes on a grid, the spawn search origin, stronghold ring
 * positions, and every structure start in a square of chunks (built with the
 * game's own Structure.generate, templates and terrain included).
 *
 * usage: java -cp <server classpath>:. SeedmapOracle OUT SEED DIMENSION RADIUS_CHUNKS
 * (DIMENSION: overworld, the_nether, the_end)
 */
public final class SeedmapOracle {
    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        Path out = Path.of(args[0]);
        long seed = Long.parseLong(args[1]);
        String dimension = args[2];
        int radius = Integer.parseInt(args[3]);
        var repo = ServerPacksSource.createVanillaTrustedRepository();
        repo.reload();
        repo.setSelected(List.of("vanilla"));
        var resources = new MultiPackResourceManager(PackType.SERVER_DATA, repo.openAllSelected());
        var layers = RegistryLayer.createRegistryAccess();
        var staticTags = TagLoader.loadTagsForExistingRegistries(resources, layers.getLayer(RegistryLayer.STATIC));
        var context = TagLoader.buildUpdatedLookups(layers.getAccessForLoading(RegistryLayer.WORLD), staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader.load(resources, context, RegistryDataLoader.WORLD_REGISTRIES, Runnable::run).join();
        var loaded = net.minecraft.server.ReloadableServerRegistries.reload(layers.replaceFrom(RegistryLayer.WORLD, world), staticTags, resources, Runnable::run).join();
        staticTags.forEach(net.minecraft.core.Registry.PendingTags::apply);
        RegistryAccess access = loaded.layers().compositeAccess();
        var settings = world.lookupOrThrow(Registries.NOISE_SETTINGS).getOrThrow(switch (dimension) {
            case "the_nether" -> NoiseGeneratorSettings.NETHER;
            case "the_end" -> NoiseGeneratorSettings.END;
            default -> NoiseGeneratorSettings.OVERWORLD;
        });
        var presets = world.lookupOrThrow(Registries.MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST);
        BiomeSource biomeSource = switch (dimension) {
            case "the_nether" -> MultiNoiseBiomeSource.createFromPreset(presets.getOrThrow(MultiNoiseBiomeSourceParameterLists.NETHER));
            case "the_end" -> TheEndBiomeSource.create(world.lookupOrThrow(Registries.BIOME));
            default -> MultiNoiseBiomeSource.createFromPreset(presets.getOrThrow(MultiNoiseBiomeSourceParameterLists.OVERWORLD));
        };
        var levelKey = switch (dimension) {
            case "the_nether" -> Level.NETHER;
            case "the_end" -> Level.END;
            default -> Level.OVERWORLD;
        };
        var randomState = RandomState.create(world.lookupOrThrow(Registries.NOISE), seed, settings.value());
        var generator = new NoiseBasedChunkGenerator(biomeSource, settings);
        var state = ChunkGeneratorStructureState.createForNormal(randomState, seed, new ChunkPos(0, 0), biomeSource, world.lookupOrThrow(Registries.STRUCTURE_SET));
        state.ensureStructuresGenerated();
        Path tmp = Files.createTempDirectory("seedmap-oracle");
        var storage = LevelStorageSource.createDefault(tmp).createAccess("oracle");
        var templates = new StructureTemplateManager(resources, storage, DataFixers.getDataFixer(), BuiltInRegistries.BLOCK);
        LevelHeightAccessor height = dimension.equals("overworld") ? LevelHeightAccessor.create(-64, 384) : LevelHeightAccessor.create(0, 256);
        var climate = randomState.createClimateSampler(SamplerContext.builder().enableCaches().build());
        try (var w = new PrintWriter(Files.newBufferedWriter(out, StandardCharsets.UTF_8))) {
            // Noise biomes on a coarse grid at three heights.
            var resolver = biomeSource.createUncachedResolver(randomState);
            for (int qy : new int[] {-8, 16, 40}) {
                for (int qz = -200; qz <= 200; qz += 8) {
                    for (int qx = -200; qx <= 200; qx += 8) {
                        var biome = resolver.getNoiseBiome(qx, qy, qz);
                        w.println("biome " + qx + " " + qy + " " + qz + " " + biome.unwrapKey().orElseThrow().identifier());
                    }
                }
            }
            if (dimension.equals("overworld")) {
                var spawn = NoiseSpawnFinder.findSpawnPosition(settings.value().spawnTarget(),
                    randomState.samplersWithContext(SamplerContext.builder().enableCaches().build()));
                w.println("spawn " + spawn.getX() + " " + spawn.getZ());
            }
            for (Holder<StructureSet> set : state.possibleStructureSets()) {
                if (set.value().placement() instanceof ConcentricRingsStructurePlacement rings) {
                    var positions = state.getRingPositionsFor(rings);
                    int i = 0;
                    for (ChunkPos p : positions) w.println("ring " + (i++) + " " + p.x() + " " + p.z());
                }
            }
            for (Holder<StructureSet> set : state.possibleStructureSets()) {
                String setName = set.unwrapKey().orElseThrow().identifier().toString();
                var placement = set.value().placement();
                var entries = set.value().structures();
                for (int x = -radius; x <= radius; x++) {
                    for (int z = -radius; z <= radius; z++) {
                        if (!placement.isStructureChunk(state, x, z)) continue;
                        ChunkPos pos = new ChunkPos(x, z);
                        var options = new ArrayList<>(entries);
                        WorldgenRandom random = new WorldgenRandom(new LegacyRandomSource(0L));
                        random.setLargeFeatureSeed(seed, x, z);
                        int total = 0;
                        for (var o : options) total += o.weight();
                        String found = null;
                        while (!options.isEmpty()) {
                            StructureSet.StructureSelectionEntry selected;
                            int index = 0;
                            if (entries.size() == 1) {
                                selected = options.get(0);
                            } else {
                                int choice = random.nextInt(total);
                                for (var o : options) {
                                    choice -= o.weight();
                                    if (choice < 0) break;
                                    index++;
                                }
                                selected = options.get(index);
                            }
                            var structure = selected.structure();
                            StructureStart start = structure.value().generate(structure, levelKey, access, generator, biomeSource, climate,
                                randomState, templates, seed, pos, 0, height, structure.value().biomes()::contains);
                            if (start.isValid()) {
                                var box = start.getBoundingBox();
                                found = structure.unwrapKey().orElseThrow().identifier() + " " + box.getCenter().getX() + " " + box.getCenter().getY() + " " + box.getCenter().getZ();
                                break;
                            }
                            if (entries.size() == 1) break;
                            options.remove(index);
                            total -= selected.weight();
                        }
                        w.println((found != null ? "start " : "fail ") + setName + " " + x + " " + z + (found != null ? " " + found : ""));
                    }
                }
            }
        }
        System.exit(0);
    }
}
