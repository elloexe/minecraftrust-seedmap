//! The map: biome tiles, structure markers, grid, slime chunks, spawn.
use crate::jobs::{
    Done, HEIGHT_PAD, Height, Job, Marker, Pool, StructKey, TILE, TileKey, WorldKey,
    struct_tile_chunks,
};
use crate::theme;
use egui::{
    Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, pos2, vec2,
};
use seedmap_core::biomes::{color, display_name};
use seedmap_core::structures::{Found, catalog, is_slime_chunk};
use seedmap_core::{BiomeId, Dimension, data};
use std::collections::{BTreeSet, HashMap};

/// Sharp texels when zoomed in, smooth when zoomed out.
const TILE_FILTER: TextureOptions = TextureOptions {
    magnification: egui::TextureFilter::Nearest,
    minification: egui::TextureFilter::Linear,
    wrap_mode: egui::TextureWrapMode::ClampToEdge,
    mipmap_mode: None,
};

pub const MIN_ZOOM: f64 = 1.0 / 512.0;
pub const MAX_ZOOM: f64 = 8.0;

struct Tile {
    ids: Vec<BiomeId>,
    heights: Vec<i16>,
    tex: Option<TextureHandle>,
    style: u64,
    used: u64,
}

#[derive(Clone, Debug)]
pub struct Selection {
    pub set: usize,
    pub found: Found,
}

/// What to draw, from the app's settings.
pub struct View<'a> {
    pub world: WorldKey,
    pub height: Height,
    pub layers: &'a BTreeSet<String>,
    pub biomes: bool,
    pub grid: bool,
    pub slime: bool,
    pub unverified: bool,
    pub highlight: Option<BiomeId>,
    pub relief: bool,
}

pub struct MapState {
    pub cx: f64,
    pub cz: f64,
    pub zoom: f64,
    tiles: HashMap<TileKey, Tile>,
    structs: HashMap<StructKey, Vec<Marker>>,
    pub strongholds: HashMap<WorldKey, Vec<Found>>,
    pub spawns: HashMap<WorldKey, [i32; 2]>,
    style: u64,
    last_highlight: Option<BiomeId>,
    last_relief: bool,
    frame: u64,
    pub selected: Option<Selection>,
    /// Hovered block and biome name, for the status bar.
    pub cursor: Option<(i32, i32, String)>,
    /// Valid markers in view per set.
    pub counts: HashMap<usize, usize>,
    pub loading: usize,
    pub toast: Option<(String, f64)>,
}

impl Default for MapState {
    fn default() -> Self {
        Self {
            cx: 0.0,
            cz: 0.0,
            zoom: 0.25,
            tiles: HashMap::new(),
            structs: HashMap::new(),
            strongholds: HashMap::new(),
            spawns: HashMap::new(),
            style: 1,
            last_highlight: None,
            last_relief: true,
            frame: 0,
            selected: None,
            cursor: None,
            counts: HashMap::new(),
            loading: 0,
            toast: None,
        }
    }
}

/// Up to what blocks-per-pixel a set's markers are drawn.
fn max_bpp(spacing: Option<i32>) -> f64 {
    match spacing {
        None => f64::INFINITY,
        Some(s) if s <= 2 => 3.0,
        Some(s) if s <= 40 => 48.0,
        Some(_) => 160.0,
    }
}

/// Marker letters for a structure variant.
pub fn variant_short(structure: &str, set_short: &'static str) -> &'static str {
    match structure.trim_start_matches("minecraft:") {
        "bastion_remnant" => "B",
        "fortress" => "F",
        "mineshaft_mesa" => "MS",
        _ => set_short,
    }
}

/// "minecraft:village_plains" -> "Plains" for a set's variant line.
pub fn variant_name(structure: &str) -> String {
    let path = structure.trim_start_matches("minecraft:");
    for prefix in [
        "village_",
        "abandoned_camp_",
        "ruined_portal_",
        "ocean_ruin_",
        "shipwreck_",
        "mineshaft_",
    ] {
        if let Some(rest) = path.strip_prefix(prefix) {
            return display_name(rest);
        }
    }
    display_name(path)
}

fn luminance(c: [u8; 3]) -> f32 {
    0.299 * f32::from(c[0]) + 0.587 * f32::from(c[1]) + 0.114 * f32::from(c[2])
}

fn darken(c: [u8; 3], f: f32) -> [u8; 3] {
    c.map(|v| (f32::from(v) * f).clamp(0.0, 255.0) as u8)
}

/// Biomes under water, which are shaded by depth instead of slope.
fn water_mask() -> &'static Vec<bool> {
    static MASK: std::sync::OnceLock<Vec<bool>> = std::sync::OnceLock::new();
    MASK.get_or_init(|| {
        data()
            .biome_names
            .iter()
            .map(|n| n.contains("ocean") || n.contains("river"))
            .collect()
    })
}

/// A tile's pixels: biome colours, lit from the north-west by the terrain
/// slope (hillshading), oceans darkened with depth, soft biome borders.
fn render_tile(
    ids: &[BiomeId],
    heights: &[i16],
    texel: i32,
    highlight: Option<BiomeId>,
    relief: bool,
) -> egui::ColorImage {
    let data = data();
    let palette: Vec<[u8; 3]> = data.biome_names.iter().map(|n| color(n)).collect();
    let water = water_mask();
    let padded = TILE + 2 * HEIGHT_PAD;
    let shaded = relief && heights.len() == padded * padded;
    // Heights arrive in 1/16 blocks. The surface estimate comes in coarse
    // vertical steps, so smooth it (5×5 box) before taking slopes.
    let mut smooth = vec![0f32; if shaded { padded * padded } else { 0 }];
    if shaded {
        let raw = |x: usize, z: usize| f32::from(heights[z * padded + x]) / 16.0;
        let mut rows = vec![0f32; padded * padded];
        for z in 0..padded {
            for x in 2..padded - 2 {
                rows[z * padded + x] = (x - 2..=x + 2).map(|i| raw(i, z)).sum::<f32>() / 5.0;
            }
        }
        for z in 2..padded - 2 {
            for x in 2..padded - 2 {
                smooth[z * padded + x] =
                    (z - 2..=z + 2).map(|j| rows[j * padded + x]).sum::<f32>() / 5.0;
            }
        }
    }
    let h = |x: usize, z: usize| smooth[z * padded + x];
    // Exaggerate relief as tiles cover more ground per texel.
    let gain = 0.85 * (texel as f32 / 4.0).powf(0.2).min(1.6);
    let mut pixels = Vec::with_capacity(TILE * TILE);
    for z in 0..TILE {
        for x in 0..TILE {
            let id = ids.get(z * TILE + x).copied().unwrap_or_default();
            let mut c = palette.get(usize::from(id.0)).copied().unwrap_or([0, 0, 0]);
            let wet = water.get(usize::from(id.0)).copied().unwrap_or(false);
            if shaded {
                let (px, pz) = (x + HEIGHT_PAD, z + HEIGHT_PAD);
                let dx = h(px + 1, pz) - h(px - 1, pz);
                let dz = h(px, pz + 1) - h(px, pz - 1);
                let slope = (dx + dz) / (2.0 * texel as f32);
                let mut light = 1.0 + (slope * gain).clamp(-0.42, 0.38);
                let top = f32::from(heights[pz * padded + px]) / 16.0;
                if wet {
                    // Water is flat: keep a hint of the sea floor, darken with depth.
                    light = 1.0 + (light - 1.0) * 0.35;
                    light *= 1.0 - ((63.0 - top).max(0.0) / 90.0).min(0.3);
                } else {
                    // High ground a touch lighter.
                    light *= 1.0 + ((top - 64.0) / 900.0).clamp(-0.04, 0.12);
                }
                c = darken(c, light);
            }
            // Soft outlines where the biome changes.
            let right = (x + 1 < TILE).then(|| ids.get(z * TILE + x + 1)).flatten();
            let down = (z + 1 < TILE)
                .then(|| ids.get((z + 1) * TILE + x))
                .flatten();
            if right.is_some_and(|r| *r != id) || down.is_some_and(|d| *d != id) {
                c = darken(c, 0.86);
            }
            if let Some(hl) = highlight
                && id != hl
            {
                let g = (luminance(c) * 0.22) as u8 + 14;
                c = [g, g, g.saturating_add(5)];
            }
            pixels.push(Color32::from_rgb(c[0], c[1], c[2]));
        }
    }
    egui::ColorImage::new([TILE, TILE], pixels)
}

impl MapState {
    pub fn go_to(&mut self, x: f64, z: f64) {
        self.cx = x;
        self.cz = z;
    }

    pub fn clear_for_new_world(&mut self) {
        self.selected = None;
    }

    pub fn absorb(&mut self, done: Vec<Done>) {
        for d in done {
            match d {
                Done::Biome(key, ids, heights) => {
                    self.tiles.insert(
                        key,
                        Tile {
                            ids,
                            heights,
                            tex: None,
                            style: 0,
                            used: self.frame,
                        },
                    );
                }
                Done::Structures(key, markers) => {
                    self.structs.insert(key, markers);
                }
                Done::Strongholds(key, list) => {
                    self.strongholds.insert(key, list);
                }
                Done::Spawn(key, pos) => {
                    self.spawns.insert(key, pos);
                }
            }
        }
    }

    fn evict(&mut self) {
        if self.tiles.len() > 900 {
            let mut used: Vec<u64> = self.tiles.values().map(|t| t.used).collect();
            used.sort_unstable();
            let cut = used[used.len() - 600];
            self.tiles.retain(|_, t| t.used >= cut);
        }
        if self.structs.len() > 6000 {
            self.structs.clear();
        }
    }

    fn texture(
        &mut self,
        ctx: &egui::Context,
        key: TileKey,
        highlight: Option<BiomeId>,
        relief: bool,
    ) -> Option<egui::TextureId> {
        let style = self.style;
        let frame = self.frame;
        let tile = self.tiles.get_mut(&key)?;
        tile.used = frame;
        if tile.tex.is_none() || tile.style != style {
            let image = render_tile(&tile.ids, &tile.heights, key.texel(), highlight, relief);
            let name = format!("tile {key:?}");
            match &mut tile.tex {
                Some(t) => t.set(image, TILE_FILTER),
                None => tile.tex = Some(ctx.load_texture(name, image, TILE_FILTER)),
            }
            tile.style = style;
        }
        tile.tex.as_ref().map(TextureHandle::id)
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, pool: &Pool, view: &View<'_>) {
        self.frame += 1;
        if view.highlight != self.last_highlight || view.relief != self.last_relief {
            self.last_highlight = view.highlight;
            self.last_relief = view.relief;
            self.style += 1;
        }
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = response.rect;
        painter.rect_filled(rect, 0.0, theme::BG);

        // Pan and zoom.
        if response.dragged() {
            let d = response.drag_delta();
            self.cx -= f64::from(d.x) / self.zoom;
            self.cz -= f64::from(d.y) / self.zoom;
        }
        if let Some(hover) = response.hover_pos() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = f64::from(pinch) * (f64::from(scroll) / 300.0).exp();
            if (factor - 1.0).abs() > 1e-4 {
                let before = self.screen_to_world(rect, hover);
                self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
                let after = self.screen_to_world(rect, hover);
                self.cx += before.0 - after.0;
                self.cz += before.1 - after.1;
            }
        }
        if response.double_clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            let (x, z) = self.screen_to_world(rect, p);
            self.cx = x;
            self.cz = z;
            self.zoom = (self.zoom * 2.0).min(MAX_ZOOM);
        }

        let bpp = 1.0 / self.zoom;
        let level = (bpp.log2().ceil() as i32).clamp(2, 11) as u8;
        let (wx0, wz0) = self.screen_to_world(rect, rect.min);
        let (wx1, wz1) = self.screen_to_world(rect, rect.max);
        let mut jobs: Vec<Job> = Vec::new();

        // Biome tiles: a coarse level first (fast), then the sharp one.
        if view.biomes {
            let coarse = (level + 2).min(13);
            for (pass, lvl) in [(0, coarse), (1, level)] {
                let span = f64::from(TILE as i32 * (1 << lvl));
                let (tx0, tx1) = ((wx0 / span).floor() as i32, (wx1 / span).floor() as i32);
                let (tz0, tz1) = ((wz0 / span).floor() as i32, (wz1 / span).floor() as i32);
                let mut wanted = Vec::new();
                for tz in tz0..=tz1 {
                    for tx in tx0..=tx1 {
                        let key = TileKey {
                            world: view.world,
                            level: lvl,
                            tx,
                            tz,
                            height: view.height,
                        };
                        let min =
                            self.world_to_screen(rect, f64::from(tx) * span, f64::from(tz) * span);
                        let max = self.world_to_screen(
                            rect,
                            f64::from(tx + 1) * span,
                            f64::from(tz + 1) * span,
                        );
                        let tile_rect = Rect::from_min_max(min, max);
                        if let Some(tex) = self.texture(ui.ctx(), key, view.highlight, view.relief)
                        {
                            painter.image(
                                tex,
                                tile_rect,
                                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                                Color32::WHITE,
                            );
                        } else if pass == 1 || lvl == coarse {
                            let d = tile_rect.center().distance(rect.center());
                            wanted.push((d, Job::Biome(key)));
                        }
                    }
                }
                wanted.sort_by(|a, b| a.0.total_cmp(&b.0));
                jobs.extend(wanted.into_iter().map(|w| w.1));
            }
        }

        // Grid.
        if view.grid {
            self.draw_grid(&painter, rect, wx0, wz0, wx1, wz1);
        }

        // Slime chunks when zoomed in.
        if view.slime && view.world.dim == Dimension::Overworld && bpp <= 2.5 {
            let (c0x, c0z) = ((wx0 / 16.0).floor() as i32, (wz0 / 16.0).floor() as i32);
            let (c1x, c1z) = ((wx1 / 16.0).floor() as i32, (wz1 / 16.0).floor() as i32);
            for cz in c0z..=c1z {
                for cx in c0x..=c1x {
                    if is_slime_chunk(view.world.seed, cx, cz) {
                        let a = self.world_to_screen(rect, f64::from(cx * 16), f64::from(cz * 16));
                        let b = self.world_to_screen(
                            rect,
                            f64::from(cx * 16 + 16),
                            f64::from(cz * 16 + 16),
                        );
                        painter.rect_filled(
                            Rect::from_min_max(a, b),
                            0.0,
                            Color32::from_rgba_unmultiplied(80, 255, 80, 70),
                        );
                        painter.rect_stroke(
                            Rect::from_min_max(a, b),
                            0.0,
                            Stroke::new(1.0, Color32::from_rgba_unmultiplied(60, 200, 60, 140)),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
            }
        }

        // Origin cross.
        let origin = self.world_to_screen(rect, 0.0, 0.0);
        if rect.expand(10.0).contains(origin) {
            let s = Stroke::new(1.5, Color32::from_rgba_unmultiplied(255, 255, 255, 140));
            painter.line_segment([origin - vec2(8.0, 0.0), origin + vec2(8.0, 0.0)], s);
            painter.line_segment([origin - vec2(0.0, 8.0), origin + vec2(0.0, 8.0)], s);
        }

        // Structures.
        let catalog = catalog();
        let mut markers: Vec<(Pos2, usize, Found, bool)> = Vec::new();
        self.counts.clear();
        let (c0x, c0z) = (
            (wx0 / 16.0).floor() as i32 - 1,
            (wz0 / 16.0).floor() as i32 - 1,
        );
        let (c1x, c1z) = (
            (wx1 / 16.0).floor() as i32 + 1,
            (wz1 / 16.0).floor() as i32 + 1,
        );
        let mut struct_jobs = Vec::new();
        for set in catalog.sets_in(view.world.dim) {
            if !view.layers.contains(&set.name) {
                continue;
            }
            if set.spacing.is_none() {
                continue; // strongholds below
            }
            if bpp > max_bpp(set.spacing) {
                continue;
            }
            let size = struct_tile_chunks(set.spacing);
            for sz in c0z.div_euclid(size)..=c1z.div_euclid(size) {
                for sx in c0x.div_euclid(size)..=c1x.div_euclid(size) {
                    let key = StructKey {
                        world: view.world,
                        set: set.index,
                        sx,
                        sz,
                    };
                    match self.structs.get(&key) {
                        Some(list) => {
                            for m in list {
                                let (found, valid) = match &m.found {
                                    Some(f) => (f.clone(), true),
                                    None if view.unverified => (
                                        Found {
                                            set: set.index,
                                            chunk: m.chunk,
                                            structure: set.members[0].0,
                                            pos: [m.chunk.0 * 16 + 8, 64, m.chunk.1 * 16 + 8],
                                        },
                                        false,
                                    ),
                                    None => continue,
                                };
                                let p = self.world_to_screen(
                                    rect,
                                    f64::from(found.pos[0]),
                                    f64::from(found.pos[2]),
                                );
                                if rect.expand(12.0).contains(p) {
                                    if valid {
                                        *self.counts.entry(set.index).or_default() += 1;
                                    }
                                    markers.push((p, set.index, found, valid));
                                }
                            }
                        }
                        None => {
                            let center = self.world_to_screen(
                                rect,
                                f64::from((sx * size + size / 2) * 16),
                                f64::from((sz * size + size / 2) * 16),
                            );
                            struct_jobs
                                .push((center.distance(rect.center()), Job::Structures(key)));
                        }
                    }
                }
            }
        }
        struct_jobs.sort_by(|a, b| a.0.total_cmp(&b.0));
        // Structures before sharp biome tiles: they are what people look for.
        let mut ordered: Vec<Job> = Vec::new();
        let coarse_count = jobs
            .iter()
            .filter(|j| matches!(j, Job::Biome(k) if k.level != level))
            .count();
        ordered.extend(jobs.iter().take(coarse_count).copied());
        ordered.extend(struct_jobs.into_iter().map(|j| j.1));
        ordered.extend(jobs.iter().skip(coarse_count).copied());

        // Strongholds and spawn.
        if view.world.dim == Dimension::Overworld {
            if let Some(set) = catalog.set("strongholds")
                && view.layers.contains(&set.name)
            {
                match self.strongholds.get(&view.world) {
                    Some(list) => {
                        for f in list {
                            let p = self.world_to_screen(
                                rect,
                                f64::from(f.pos[0] + 4),
                                f64::from(f.pos[2] + 4),
                            );
                            if rect.expand(12.0).contains(p) {
                                *self.counts.entry(set.index).or_default() += 1;
                                markers.push((p, set.index, f.clone(), true));
                            }
                        }
                    }
                    None => ordered.insert(0, Job::Strongholds(view.world)),
                }
            }
            if !self.spawns.contains_key(&view.world) {
                ordered.insert(0, Job::Spawn(view.world));
            }
        }
        self.loading = ordered.len();
        pool.want(ordered);

        // Paint markers, find the hovered one.
        let hover = response.hover_pos();
        // Small dots far out, lettered badges closer in.
        let radius = 4.5 + (self.zoom.log2() as f32 + 4.5).clamp(0.0, 7.0);
        let mut hovered: Option<(f32, usize)> = None;
        for (i, (p, _, _, _)) in markers.iter().enumerate() {
            if let Some(h) = hover {
                let d = p.distance(h);
                if d <= radius + 3.0 && hovered.is_none_or(|(bd, _)| d < bd) {
                    hovered = Some((d, i));
                }
            }
        }
        for (i, (p, set, found, valid)) in markers.iter().enumerate() {
            let info = &catalog.sets[*set];
            let short = variant_short(&catalog.structures[found.structure].name, info.short);
            let selected = self
                .selected
                .as_ref()
                .is_some_and(|s| s.found.chunk == found.chunk && s.set == *set);
            let big = hovered.is_some_and(|(_, h)| h == i) || selected;
            draw_marker(
                &painter,
                *p,
                radius * if big { 1.22 } else { 1.0 },
                info.color,
                short,
                *valid,
                selected,
                info.spacing.is_none(),
            );
        }

        // Spawn star.
        if let Some(&[sx, sz]) = self.spawns.get(&view.world) {
            let p = self.world_to_screen(rect, f64::from(sx), f64::from(sz));
            if rect.expand(12.0).contains(p) {
                draw_star(&painter, p, radius + 3.0);
            }
        }

        // Hover info.
        self.cursor = None;
        if let Some(h) = hover {
            let (x, z) = self.screen_to_world(rect, h);
            let (bx, bz) = (x.floor() as i32, z.floor() as i32);
            let world = pool.world(view.world);
            let mut sampler = world.sampler();
            let y = match view.height {
                Height::Fixed(y) => y,
                Height::Surface => sampler.surface_estimate(bx, bz),
            };
            let biome = sampler.biome_at(bx, y, bz);
            let name = display_name(world.data.biome_name(biome));
            self.cursor = Some((bx, bz, name));
            if let Some((_, i)) = hovered {
                let (_, set, found, valid) = &markers[i];
                let info = &catalog.sets[*set];
                let variant = variant_name(&catalog.structures[found.structure].name);
                egui::Tooltip::always_open(
                    ui.ctx().clone(),
                    ui.layer_id(),
                    egui::Id::unique("marker tip"),
                    egui::PopupAnchor::Pointer,
                )
                .show(|ui| {
                    ui.label(
                        egui::RichText::new(info.label)
                            .strong()
                            .color(theme::rgb(info.color)),
                    );
                    if info.members.len() > 1 {
                        ui.label(variant);
                    }
                    ui.label(
                        egui::RichText::new(format!("X {}  Z {}", found.pos[0], found.pos[2]))
                            .monospace(),
                    );
                    if !valid {
                        ui.label(
                            egui::RichText::new("Placement only: biome rules failed")
                                .small()
                                .color(theme::WARN),
                        );
                    }
                    ui.label(
                        egui::RichText::new("Click to select · right-click to copy")
                            .small()
                            .color(theme::MUTED),
                    );
                });
                if response.clicked() {
                    self.selected = Some(Selection {
                        set: *set,
                        found: found.clone(),
                    });
                }
            }
        }
        if response.clicked() && hovered.is_none() {
            self.selected = None;
        }
        // Right-click: copy.
        let pointer_world = hover.map(|h| self.screen_to_world(rect, h));
        let hovered_found = hovered.map(|(_, i)| markers[i].2.clone());
        response.context_menu(|ui| {
            let (x, z) = match (&hovered_found, pointer_world) {
                (Some(f), _) => (f.pos[0], f.pos[2]),
                (None, Some((x, z))) => (x.floor() as i32, z.floor() as i32),
                _ => (0, 0),
            };
            if ui.button(format!("Copy  {x}, {z}")).clicked() {
                ui.ctx().copy_text(format!("{x} {z}"));
                ui.close();
            }
            if ui.button(format!("Copy  /tp @s {x} ~ {z}")).clicked() {
                ui.ctx().copy_text(format!("/tp @s {x} ~ {z}"));
                ui.close();
            }
        });

        self.draw_scale(&painter, rect);
        self.draw_controls(ui, rect);
        if self.loading > 0 {
            // A slim moving bar while tiles are generated.
            let t = ui.input(|i| i.time) as f32;
            let w = rect.width() * 0.25;
            let x = rect.left() + ((t * 0.6).fract() * (rect.width() + w)) - w;
            let bar = Rect::from_min_max(
                pos2(x.max(rect.left()), rect.top()),
                pos2((x + w).min(rect.right()), rect.top() + 2.5),
            );
            painter.rect_filled(bar, 1.0, theme::ACCENT);
            ui.ctx().request_repaint();
        }
        self.evict();
    }

    /// Floating zoom and recentre buttons at the map's top-right corner.
    fn draw_controls(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let size = vec2(34.0, 34.0);
        let x = rect.right() - size.x - 14.0;
        let mut y = rect.top() + 14.0;
        let buttons: [(&str, &str); 3] =
            [("+", "Zoom in"), ("−", "Zoom out"), ("⌖", "Back to 0, 0")];
        for (i, (label, tip)) in buttons.into_iter().enumerate() {
            let r = Rect::from_min_size(pos2(x, y), size);
            let button = egui::Button::new(
                egui::RichText::new(label)
                    .font(theme::bold(18.0))
                    .color(theme::TEXT),
            )
            .fill(Color32::from_rgba_unmultiplied(23, 26, 32, 235))
            .stroke(Stroke::new(1.0, theme::BORDER))
            .corner_radius(8);
            if ui.put(r, button).on_hover_text(tip).clicked() {
                match i {
                    0 => self.zoom = (self.zoom * 2.0).min(MAX_ZOOM),
                    1 => self.zoom = (self.zoom / 2.0).max(MIN_ZOOM),
                    _ => self.go_to(0.0, 0.0),
                }
            }
            y += size.y + 6.0;
        }
    }

    fn draw_grid(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        wx0: f64,
        wz0: f64,
        wx1: f64,
        wz1: f64,
    ) {
        // A power-of-two spacing that lands 70-140 px apart.
        let mut spacing = 16.0f64;
        while spacing * self.zoom < 70.0 {
            spacing *= 2.0;
        }
        let stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 28));
        let font = FontId::proportional(10.5);
        let label = Color32::from_rgba_unmultiplied(235, 240, 245, 150);
        let mut x = (wx0 / spacing).floor() * spacing;
        while x <= wx1 {
            let p = self.world_to_screen(rect, x, 0.0);
            painter.line_segment([pos2(p.x, rect.top()), pos2(p.x, rect.bottom())], stroke);
            painter.text(
                pos2(p.x + 3.0, rect.top() + 3.0),
                Align2::LEFT_TOP,
                format!("{}", x as i64),
                font.clone(),
                label,
            );
            x += spacing;
        }
        let mut z = (wz0 / spacing).floor() * spacing;
        while z <= wz1 {
            let p = self.world_to_screen(rect, 0.0, z);
            painter.line_segment([pos2(rect.left(), p.y), pos2(rect.right(), p.y)], stroke);
            painter.text(
                pos2(rect.left() + 3.0, p.y + 2.0),
                Align2::LEFT_TOP,
                format!("{}", z as i64),
                font.clone(),
                label,
            );
            z += spacing;
        }
    }

    fn draw_scale(&self, painter: &egui::Painter, rect: Rect) {
        let target = 120.0 / self.zoom;
        let nice = [1.0, 2.0, 5.0];
        let mut best = 1.0;
        let mut mag = 1.0;
        'outer: while mag < 1e8 {
            for n in nice {
                if n * mag > target {
                    break 'outer;
                }
                best = n * mag;
            }
            mag *= 10.0;
        }
        let w = (best * self.zoom) as f32;
        let base = pos2(rect.left() + 16.0, rect.bottom() - 18.0);
        let bg = Rect::from_min_size(base - vec2(8.0, 22.0), vec2(w + 16.0, 30.0));
        painter.rect_filled(bg, 8.0, Color32::from_rgba_unmultiplied(23, 26, 32, 225));
        painter.rect_stroke(
            bg,
            8.0,
            Stroke::new(1.0, theme::BORDER),
            egui::StrokeKind::Inside,
        );
        let s = Stroke::new(2.0, theme::TEXT);
        painter.line_segment([base, base + vec2(w, 0.0)], s);
        painter.line_segment([base, base - vec2(0.0, 6.0)], s);
        painter.line_segment([base + vec2(w, 0.0), base + vec2(w, -6.0)], s);
        let text = if best >= 1000.0 {
            format!("{} km", best / 1000.0)
        } else {
            format!("{best} blocks")
        };
        painter.text(
            base + vec2(w / 2.0, -8.0),
            Align2::CENTER_BOTTOM,
            text,
            FontId::proportional(11.0),
            theme::TEXT,
        );
    }

    pub fn world_to_screen(&self, rect: Rect, x: f64, z: f64) -> Pos2 {
        let c = rect.center();
        pos2(
            c.x + ((x - self.cx) * self.zoom) as f32,
            c.y + ((z - self.cz) * self.zoom) as f32,
        )
    }
    pub fn screen_to_world(&self, rect: Rect, p: Pos2) -> (f64, f64) {
        let c = rect.center();
        (
            self.cx + f64::from(p.x - c.x) / self.zoom,
            self.cz + f64::from(p.y - c.y) / self.zoom,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_marker(
    painter: &egui::Painter,
    p: Pos2,
    r: f32,
    c: [u8; 3],
    text: &str,
    valid: bool,
    selected: bool,
    diamond: bool,
) {
    let ink = if luminance(c) > 150.0 {
        Color32::from_rgb(24, 24, 28)
    } else {
        Color32::WHITE
    };
    let shape = |center: Pos2, radius: f32, fill: Color32, stroke: Stroke| {
        if diamond {
            let pts = vec![
                center + vec2(0.0, -radius * 1.2),
                center + vec2(radius * 1.2, 0.0),
                center + vec2(0.0, radius * 1.2),
                center + vec2(-radius * 1.2, 0.0),
            ];
            egui::Shape::convex_polygon(pts, fill, stroke)
        } else {
            egui::Shape::circle_filled(center, radius, fill)
        }
    };
    // Soft shadow.
    for (grow, alpha) in [(3.0, 40), (1.5, 70)] {
        painter.add(shape(
            p + vec2(0.0, 2.0),
            r + grow,
            Color32::from_black_alpha(alpha),
            Stroke::NONE,
        ));
    }
    if selected {
        painter.add(shape(
            p,
            r + 5.0,
            Color32::from_rgba_unmultiplied(255, 255, 255, 60),
            Stroke::NONE,
        ));
    }
    let ring = if valid {
        Color32::WHITE
    } else {
        Color32::from_white_alpha(120)
    };
    painter.add(shape(p, r + 1.8, ring, Stroke::NONE));
    let fill = if valid {
        theme::rgb(c)
    } else {
        Color32::from_rgb(c[0] / 3 + 20, c[1] / 3 + 20, c[2] / 3 + 24)
    };
    painter.add(shape(p, r, fill, Stroke::NONE));
    if r >= 6.5 {
        let size = if text.len() > 1 { r * 0.92 } else { r * 1.18 };
        painter.text(
            p + vec2(0.0, 0.5),
            Align2::CENTER_CENTER,
            text,
            theme::bold(size),
            if valid {
                ink
            } else {
                Color32::from_white_alpha(170)
            },
        );
    }
}

fn draw_star(painter: &egui::Painter, p: Pos2, r: f32) {
    let pts: Vec<Pos2> = (0..10)
        .map(|i| {
            let a = std::f32::consts::PI * (i as f32) / 5.0 - std::f32::consts::FRAC_PI_2;
            let rr = if i % 2 == 0 { r } else { r * 0.45 };
            p + vec2(a.cos() * rr, a.sin() * rr)
        })
        .collect();
    let fill = Color32::from_rgb(255, 214, 64);
    // A star is not convex: fill it as ten triangles around the centre.
    for i in 0..10 {
        painter.add(egui::Shape::convex_polygon(
            vec![p, pts[i], pts[(i + 1) % 10]],
            fill,
            Stroke::NONE,
        ));
    }
    painter.add(egui::Shape::closed_line(
        pts,
        Stroke::new(1.5, Color32::from_rgb(40, 30, 0)),
    ));
}
