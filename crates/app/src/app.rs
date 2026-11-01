//! The window: top bar, layer sidebar, map, seed finder.
use crate::finder::{Finder, FinderSettings, biome_combo};
use crate::jobs::{Height, Pool, WorldKey};
use crate::map::{MapState, View, variant_name};
use crate::theme;
use egui::{RichText, Vec2};
use seedmap_core::biomes::{NEW_IN_26, color, display_name};
use seedmap_core::search::parse_seed;
use seedmap_core::structures::catalog;
use seedmap_core::{Dimension, data};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    seed_text: String,
    dim: u8,
    large_biomes: bool,
    layers: BTreeSet<String>,
    biomes: bool,
    surface: bool,
    y_level: i32,
    grid: bool,
    slime: bool,
    unverified: bool,
    highlight: Option<String>,
    finder_open: bool,
    finder: FinderSettings,
    view: (f64, f64, f64),
}
impl Default for Settings {
    fn default() -> Self {
        let layers = [
            "villages",
            "pillager_outposts",
            "desert_pyramids",
            "jungle_temples",
            "swamp_huts",
            "igloos",
            "ocean_monuments",
            "woodland_mansions",
            "strongholds",
            "ancient_cities",
            "trail_ruins",
            "abandoned_camp",
            "nether_complexes",
            "end_cities",
        ]
        .iter()
        .map(|s| format!("minecraft:{s}"))
        .collect();
        Self {
            seed_text: "26".into(),
            dim: 0,
            large_biomes: false,
            layers,
            biomes: true,
            surface: true,
            y_level: 64,
            grid: false,
            slime: false,
            unverified: false,
            highlight: None,
            finder_open: false,
            finder: FinderSettings::default(),
            view: (0.0, 0.0, 0.25),
        }
    }
}

pub struct SeedmapApp {
    s: Settings,
    pool: Pool,
    map: MapState,
    finder: Finder,
    goto: String,
    applied_seed: String,
}

/// `--seed <seed> --dimension <overworld|nether|end> --at <x> <z>
/// --zoom <pixels per block> --finder --layer <name>...`
fn apply_args(s: &mut Settings, args: Vec<String>) {
    let mut i = 0;
    while i < args.len() {
        let next = |k: usize| args.get(i + k).cloned().unwrap_or_default();
        match args[i].as_str() {
            "--seed" => {
                s.seed_text = next(1);
                i += 1;
            }
            "--dimension" => {
                s.dim = match next(1).as_str() {
                    "nether" | "the_nether" => 1,
                    "end" | "the_end" => 2,
                    _ => 0,
                };
                i += 1;
            }
            "--at" => {
                s.view.0 = next(1).parse().unwrap_or(0.0);
                s.view.1 = next(2).parse().unwrap_or(0.0);
                i += 2;
            }
            "--zoom" => {
                s.view.2 = next(1).parse().unwrap_or(0.25);
                i += 1;
            }
            "--finder" => s.finder_open = true,
            "--layer" => {
                s.layers.insert(format!(
                    "minecraft:{}",
                    next(1).trim_start_matches("minecraft:")
                ));
                i += 1;
            }
            "--height" => {
                s.surface = false;
                s.y_level = next(1).parse().unwrap_or(64);
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
}

fn dim_of(i: u8) -> Dimension {
    Dimension::ALL[usize::from(i).min(2)]
}

impl SeedmapApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let mut s: Settings = cc
            .storage
            .and_then(|st| eframe::get_value(st, eframe::APP_KEY))
            .unwrap_or_default();
        apply_args(&mut s, std::env::args().skip(1).collect());
        let mut map = MapState::default();
        map.cx = s.view.0;
        map.cz = s.view.1;
        map.zoom = s.view.2.clamp(crate::map::MIN_ZOOM, crate::map::MAX_ZOOM);
        Self {
            applied_seed: s.seed_text.clone(),
            s,
            pool: Pool::new(cc.egui_ctx.clone()),
            map,
            finder: Finder::default(),
            goto: String::new(),
        }
    }

    fn seed(&self) -> i64 {
        parse_seed(&self.applied_seed)
    }
    fn world_key(&self) -> WorldKey {
        WorldKey {
            seed: self.seed(),
            dim: dim_of(self.s.dim),
            large: self.s.large_biomes && self.s.dim == 0,
        }
    }

    fn set_seed(&mut self, text: String) {
        self.s.seed_text = text.clone();
        self.applied_seed = text;
        self.map.clear_for_new_world();
    }

    fn toast(&mut self, ctx: &egui::Context, text: impl Into<String>) {
        self.map.toast = Some((text.into(), ctx.input(|i| i.time) + 2.0));
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.add_space(4.0);
            ui.label(RichText::new("⛏").size(20.0).color(theme::ACCENT));
            ui.label(RichText::new("Minecraft Rust Seedmap").heading().strong());
            egui::Frame::NONE
                .fill(theme::ACCENT_DIM)
                .corner_radius(4)
                .inner_margin(egui::Margin::symmetric(6, 2))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("26.3")
                            .small()
                            .strong()
                            .color(egui::Color32::WHITE),
                    );
                });
            ui.separator();

            ui.label(RichText::new("Seed").color(theme::MUTED));
            let edit = egui::TextEdit::singleline(&mut self.s.seed_text)
                .desired_width(200.0)
                .font(egui::TextStyle::Monospace)
                .hint_text("number or text");
            let r = ui.add(edit);
            if (r.lost_focus() || (r.changed() && self.s.seed_text.len() > 24))
                && self.s.seed_text != self.applied_seed
            {
                let t = self.s.seed_text.clone();
                self.set_seed(t);
            }
            if ui.button("🎲").on_hover_text("Random seed").clicked() {
                let t = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(0);
                let mut z = t.wrapping_add(0x9E37_79B9_7F4A_7C15);
                z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
                self.set_seed(((z ^ (z >> 31)) as i64).to_string());
            }
            if ui.button("📋").on_hover_text("Copy seed").clicked() {
                ui.ctx().copy_text(self.seed().to_string());
                self.toast(ui.ctx(), "Seed copied");
            }
            if parse_seed(&self.applied_seed).to_string() != self.applied_seed.trim() {
                ui.label(
                    RichText::new(format!("= {}", self.seed()))
                        .small()
                        .monospace()
                        .color(theme::MUTED),
                )
                .on_hover_text("Text seeds are hashed like the game does (String.hashCode)");
            }
            ui.separator();

            for (i, d) in Dimension::ALL.iter().enumerate() {
                let selected = self.s.dim == i as u8;
                let text = RichText::new(d.name());
                if ui
                    .selectable_label(selected, if selected { text.strong() } else { text })
                    .clicked()
                {
                    self.s.dim = i as u8;
                    self.map.selected = None;
                }
            }
            if self.s.dim == 0 {
                ui.checkbox(&mut self.s.large_biomes, "Large biomes");
            }
            ui.separator();

            ui.label(RichText::new("Go to").color(theme::MUTED));
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.goto)
                    .desired_width(110.0)
                    .hint_text("x z"),
            );
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let nums: Vec<f64> = self
                    .goto
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .filter_map(|t| t.trim().parse().ok())
                    .collect();
                match nums.as_slice() {
                    [x, z] | [x, _, z] => self.map.go_to(*x, *z),
                    _ => self.toast(ui.ctx(), "Type coordinates as: x z"),
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                let label = if self.finder.running() {
                    "🔎 Seed Finder ●"
                } else {
                    "🔎 Seed Finder"
                };
                if ui
                    .selectable_label(self.s.finder_open, RichText::new(label).strong())
                    .clicked()
                {
                    self.s.finder_open = !self.s.finder_open;
                }
            });
        });
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let dim = dim_of(self.s.dim);
        let cat = catalog();
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(6.0);
            theme::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    theme::section_title(ui, "Structures");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("None").clicked() {
                            for s in cat.sets_in(dim) {
                                self.s.layers.remove(&s.name);
                            }
                        }
                        if ui.small_button("All").clicked() {
                            for s in cat.sets_in(dim) {
                                self.s.layers.insert(s.name.clone());
                            }
                        }
                    });
                });
                let mut sets: Vec<_> = cat.sets_in(dim).collect();
                sets.sort_by_key(|s| s.label);
                for set in sets {
                    let mut on = self.s.layers.contains(&set.name);
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), egui::Sense::hover());
                        ui.painter().rect_filled(r.shrink(1.0), 4.0, theme::rgb(set.color));
                        if ui.checkbox(&mut on, set.label).changed() {
                            if on {
                                self.s.layers.insert(set.name.clone());
                            } else {
                                self.s.layers.remove(&set.name);
                            }
                        }
                        if set.new_in_26 {
                            ui.label(RichText::new("NEW").small().strong().color(theme::NEW))
                                .on_hover_text("Added in Minecraft 26.x: other seed maps do not show it yet");
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if let Some(n) = self.map.counts.get(&set.index).filter(|_| on) {
                                ui.label(RichText::new(n.to_string()).small().color(theme::MUTED));
                            }
                        });
                    });
                }
                ui.label(RichText::new("Dense layers (treasure, mineshafts, fossils) appear when zoomed in.").small().color(theme::MUTED));
            });

            ui.add_space(6.0);
            theme::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                theme::section_title(ui, "Map");
                ui.checkbox(&mut self.s.biomes, "Biomes");
                if dim == Dimension::Overworld {
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.s.surface, true, "Surface");
                        ui.radio_value(&mut self.s.surface, false, "At height");
                    });
                }
                if dim != Dimension::Overworld || !self.s.surface {
                    ui.add(egui::Slider::new(&mut self.s.y_level, -64..=320).text("Y").step_by(4.0))
                        .on_hover_text("Cave biomes (lush caves, deep dark, sulfur caves) show up underground");
                }
                ui.checkbox(&mut self.s.grid, "Coordinate grid");
                if dim == Dimension::Overworld {
                    ui.checkbox(&mut self.s.slime, "Slime chunks (zoom in)");
                }
                ui.checkbox(&mut self.s.unverified, "Placement-only candidates")
                    .on_hover_text("Also show chunks the structure set picks where the biome or terrain rules then refuse it");
                ui.horizontal(|ui| {
                    ui.label("Highlight");
                    let mut h = self.s.highlight.clone().unwrap_or_default();
                    if h.is_empty() {
                        h = "none".into();
                    }
                    let before = h.clone();
                    biome_combo(ui, "highlight", &mut h, None);
                    if h != before {
                        self.s.highlight = Some(h);
                    }
                    if self.s.highlight.is_some() && ui.small_button("✕").clicked() {
                        self.s.highlight = None;
                    }
                });
            });

            if let Some(sel) = self.map.selected.clone() {
                ui.add_space(6.0);
                theme::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let info = &cat.sets[sel.set];
                    theme::section_title(ui, "Selected");
                    ui.label(RichText::new(info.label).strong().size(16.0).color(theme::rgb(info.color)));
                    if info.members.len() > 1 {
                        ui.label(variant_name(&cat.structures[sel.found.structure].name));
                    }
                    let [x, y, z] = sel.found.pos;
                    ui.label(RichText::new(format!("X {x}   Z {z}")).monospace());
                    ui.label(RichText::new(format!("Chunk {} {}", sel.found.chunk.0, sel.found.chunk.1)).small().color(theme::MUTED));
                    if y != 0 && y != 64 {
                        ui.label(RichText::new(format!("Start height ≈ {y}")).small().color(theme::MUTED));
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Copy /tp").clicked() {
                            ui.ctx().copy_text(format!("/tp @s {x} ~ {z}"));
                            self.toast(ui.ctx(), "Teleport command copied");
                        }
                        if ui.button("Center").clicked() {
                            self.map.go_to(f64::from(x), f64::from(z));
                        }
                    });
                });
            }

            ui.add_space(6.0);
            theme::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                theme::section_title(ui, "World");
                let key = self.world_key();
                ui.label(RichText::new(format!("Seed {}", key.seed)).monospace());
                if dim == Dimension::Overworld {
                    let base = WorldKey { dim: Dimension::Overworld, ..key };
                    match self.map.spawns.get(&base) {
                        Some(&[x, z]) => {
                            ui.horizontal(|ui| {
                                ui.label(format!("★ Spawn  {x}, {z}"));
                                if ui.small_button("Go").clicked() {
                                    self.map.go_to(f64::from(x), f64::from(z));
                                }
                            });
                        }
                        None => {
                            ui.label(RichText::new("Spawn: finding…").color(theme::MUTED));
                        }
                    }
                    if let Some(list) = self.map.strongholds.get(&base)
                        && let Some(n) = list.iter().min_by_key(|f| i64::from(f.pos[0]).pow(2) + i64::from(f.pos[2]).pow(2)) {
                            let (x, z) = (n.pos[0] + 4, n.pos[2] + 4);
                            ui.horizontal(|ui| {
                                ui.label(format!("Nearest stronghold  {x}, {z}"));
                                if ui.small_button("Go").clicked() {
                                    self.map.go_to(f64::from(x), f64::from(z));
                                }
                            });
                        }
                }
                ui.horizontal(|ui| {
                    if ui.small_button("Go to 0, 0").clicked() {
                        self.map.go_to(0.0, 0.0);
                    }
                    if ui.small_button("Zoom out").clicked() {
                        self.map.zoom = (self.map.zoom / 2.0).max(crate::map::MIN_ZOOM);
                    }
                    if ui.small_button("Zoom in").clicked() {
                        self.map.zoom = (self.map.zoom * 2.0).min(crate::map::MAX_ZOOM);
                    }
                });
            });

            ui.add_space(6.0);
            egui::CollapsingHeader::new(RichText::new("BIOME LEGEND").small().strong().color(theme::MUTED))
                .default_open(false)
                .show(ui, |ui| {
                    let d = data();
                    let mut names: Vec<&String> = match dim {
                        Dimension::Overworld => {
                            let set: BTreeSet<_> = d.overworld.entries().iter().map(|(_, b)| *b).collect();
                            d.biome_names.iter().filter(|n| d.biome(n).is_some_and(|b| set.contains(&b))).collect()
                        }
                        Dimension::Nether => {
                            let set: BTreeSet<_> = d.nether.entries().iter().map(|(_, b)| *b).collect();
                            d.biome_names.iter().filter(|n| d.biome(n).is_some_and(|b| set.contains(&b))).collect()
                        }
                        Dimension::End => d.biome_names.iter().filter(|n| n.contains("end") && !n.contains("the_end") || n.ends_with("the_end")).collect(),
                    };
                    names.sort_by_key(|n| display_name(n));
                    for n in names {
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(Vec2::splat(12.0), egui::Sense::hover());
                            ui.painter().rect_filled(r, 3.0, theme::rgb(color(n)));
                            let active = self.s.highlight.as_deref() == Some(n.as_str());
                            if ui.selectable_label(active, display_name(n)).on_hover_text("Click to highlight").clicked() {
                                self.s.highlight = if active { None } else { Some(n.clone()) };
                            }
                            if NEW_IN_26.contains(&n.as_str()) {
                                ui.label(RichText::new("NEW").small().strong().color(theme::NEW));
                            }
                        });
                    }
                });
            ui.add_space(10.0);
            ui.label(
                RichText::new("Powered by Minecraft Rust's own world generator. Verified against vanilla 26.3.")
                    .small()
                    .color(theme::MUTED),
            );
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            match &self.map.cursor {
                Some((x, z, biome)) => {
                    ui.label(RichText::new(format!("X {x}  Z {z}")).monospace());
                    ui.label(RichText::new(format!("Chunk {} {}", x >> 4, z >> 4)).monospace().color(theme::MUTED));
                    ui.separator();
                    ui.label(RichText::new(biome).strong());
                }
                None => {
                    ui.label(RichText::new("Drag to pan · scroll to zoom · double-click to zoom in · right-click to copy").color(theme::MUTED));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("1 px = {:.2} blocks", 1.0 / self.map.zoom)).small().color(theme::MUTED));
                if self.map.loading > 0 || self.pool.busy() > 0 {
                    ui.spinner();
                    ui.label(RichText::new(format!("Generating on {} threads", self.pool.threads)).small().color(theme::MUTED));
                }
            });
        });
    }
}

impl eframe::App for SeedmapApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.map.absorb(self.pool.finished());

        egui::Panel::top("top")
            .exact_size(48.0)
            .frame(
                egui::Frame::NONE
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(8, 6)),
            )
            .show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("status")
            .exact_size(28.0)
            .frame(
                egui::Frame::NONE
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(10, 4)),
            )
            .show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("layers")
            .default_size(268.0)
            .min_size(220.0)
            .frame(
                egui::Frame::NONE
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(10, 4)),
            )
            .show(ui, |ui| self.sidebar(ui));
        if self.s.finder_open {
            egui::Panel::right("finder")
                .default_size(350.0)
                .min_size(300.0)
                .frame(
                    egui::Frame::NONE
                        .fill(theme::PANEL)
                        .inner_margin(egui::Margin::same(12)),
                )
                .show(ui, |ui| {
                    if let Some(seed) = self.finder.ui(ui, &mut self.s.finder, self.s.large_biomes)
                    {
                        self.set_seed(seed.to_string());
                        self.s.dim = 0;
                    }
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(theme::BG))
            .show(ui, |ui| {
                let highlight = self.s.highlight.as_deref().and_then(|h| data().biome(h));
                let height = if dim_of(self.s.dim) == Dimension::Overworld && self.s.surface {
                    Height::Surface
                } else {
                    Height::Fixed(self.s.y_level)
                };
                let view = View {
                    world: self.world_key(),
                    height,
                    layers: &self.s.layers,
                    biomes: self.s.biomes,
                    grid: self.s.grid,
                    slime: self.s.slime,
                    unverified: self.s.unverified,
                    highlight,
                };
                self.map.ui(ui, &self.pool, &view);

                // Toast.
                if let Some((text, until)) = self.map.toast.clone() {
                    let now = ctx.input(|i| i.time);
                    if now < until {
                        let rect = ui.max_rect();
                        let pos = egui::pos2(rect.center().x, rect.bottom() - 40.0);
                        let galley = ui.painter().layout_no_wrap(
                            text,
                            egui::FontId::proportional(14.0),
                            theme::TEXT,
                        );
                        let bg = egui::Rect::from_center_size(
                            pos,
                            galley.size() + egui::vec2(24.0, 14.0),
                        );
                        ui.painter().rect_filled(bg, 8.0, theme::CARD);
                        ui.painter().rect_stroke(
                            bg,
                            8.0,
                            egui::Stroke::new(1.0, theme::ACCENT_DIM),
                            egui::StrokeKind::Inside,
                        );
                        ui.painter()
                            .galley(bg.center() - galley.size() / 2.0, galley, theme::TEXT);
                        ctx.request_repaint_after(std::time::Duration::from_millis(100));
                    } else {
                        self.map.toast = None;
                    }
                }
            });
        self.s.view = (self.map.cx, self.map.cz, self.map.zoom);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.s);
    }
}
