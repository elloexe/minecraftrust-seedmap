//! The map: biome tiles, structure markers, grid, slime chunks, spawn.
use crate::jobs::{
    Done, Height, Job, Marker, Pool, StructKey, TILE, TileKey, WorldKey, struct_tile_chunks,
};
use crate::theme;
use egui::{
    Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, Vec2, pos2,
    vec2,
};
use seedmap_core::biomes::{color, display_name};
use seedmap_core::structures::{Found, catalog, is_slime_chunk};
use seedmap_core::{BiomeId, Dimension, data};
use std::collections::{BTreeSet, HashMap};

pub const MIN_ZOOM: f64 = 1.0 / 512.0;
pub const MAX_ZOOM: f64 = 8.0;

struct Tile {
    ids: Vec<BiomeId>,
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
    c.map(|v| (f32::from(v) * f) as u8)
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
                Done::Biome(key, ids) => {
                    self.tiles.insert(
                        key,
                        Tile {
                            ids,
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
    ) -> Option<egui::TextureId> {
        let style = self.style;
        let frame = self.frame;
        let tile = self.tiles.get_mut(&key)?;
        tile.used = frame;
        if tile.tex.is_none() || tile.style != style {
            let data = data();
            let palette: Vec<[u8; 3]> = data.biome_names.iter().map(|n| color(n)).collect();
            let mut pixels = Vec::with_capacity(TILE * TILE);
            for z in 0..TILE {
                for x in 0..TILE {
                    let id = tile.ids.get(z * TILE + x).copied().unwrap_or_default();
                    let mut c = palette.get(usize::from(id.0)).copied().unwrap_or([0, 0, 0]);
                    // Soft outlines where the biome changes.
                    let right = if x + 1 < TILE {
                        tile.ids.get(z * TILE + x + 1)
                    } else {
                        None
                    };
                    let down = if z + 1 < TILE {
                        tile.ids.get((z + 1) * TILE + x)
                    } else {
                        None
                    };
                    if right.is_some_and(|r| *r != id) || down.is_some_and(|d| *d != id) {
                        c = darken(c, 0.82);
                    }
                    if let Some(h) = highlight
                        && id != h
                    {
                        let g = (luminance(c) * 0.25) as u8 + 12;
                        c = [g, g, g + 4];
                    }
                    pixels.push(Color32::from_rgb(c[0], c[1], c[2]));
                }
            }
            let image = egui::ColorImage::new([TILE, TILE], pixels);
            let name = format!("tile {key:?}");
            match &mut tile.tex {
                Some(t) => t.set(image, TextureOptions::NEAREST),
                None => tile.tex = Some(ctx.load_texture(name, image, TextureOptions::NEAREST)),
            }
            tile.style = style;
        }
        tile.tex.as_ref().map(TextureHandle::id)
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, pool: &Pool, view: &View<'_>) {
        self.frame += 1;
        if view.highlight != self.last_highlight {
            self.last_highlight = view.highlight;
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
                        if let Some(tex) = self.texture(ui.ctx(), key, view.highlight) {
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
        let radius = (7.0 + (self.zoom.log2() as f32 + 4.0).clamp(0.0, 6.0)).min(13.0);
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
                radius * if big { 1.25 } else { 1.0 },
                info.color,
                short,
                *valid,
                selected,
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
        self.evict();
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
        painter.rect_filled(bg, 6.0, Color32::from_rgba_unmultiplied(15, 17, 21, 190));
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

fn draw_marker(
    painter: &egui::Painter,
    p: Pos2,
    r: f32,
    c: [u8; 3],
    text: &str,
    valid: bool,
    selected: bool,
) {
    let fill = if valid {
        theme::rgb(c)
    } else {
        Color32::from_rgba_unmultiplied(c[0], c[1], c[2], 70)
    };
    let rect = Rect::from_center_size(p, Vec2::splat(r * 2.0));
    painter.rect_filled(
        rect.translate(vec2(1.0, 2.0)),
        r * 0.45,
        Color32::from_black_alpha(110),
    );
    painter.rect_filled(rect, r * 0.45, fill);
    let outline = if selected {
        Color32::WHITE
    } else {
        Color32::from_rgb(15, 17, 21)
    };
    painter.rect_stroke(
        rect,
        r * 0.45,
        Stroke::new(if selected { 2.5 } else { 1.5 }, outline),
        egui::StrokeKind::Outside,
    );
    let ink = if luminance(c) > 140.0 {
        Color32::from_rgb(20, 20, 24)
    } else {
        Color32::WHITE
    };
    let size = if text.len() > 1 { r * 0.95 } else { r * 1.2 };
    painter.text(
        p + vec2(0.0, 0.5),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(size),
        if valid {
            ink
        } else {
            Color32::from_white_alpha(160)
        },
    );
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
