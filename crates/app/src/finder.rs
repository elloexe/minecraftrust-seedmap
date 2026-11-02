//! The seed finder panel: build conditions, run the search, open results.
use crate::theme;
use egui::RichText;
use seedmap_core::biomes::display_name;
use seedmap_core::search::{Condition, Hit, Search, SearchConfig, SeedSource, start};
use seedmap_core::structures::{BiomeMask, catalog};
use seedmap_core::{Dimension, data};
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Cond {
    Structure {
        set: String,
        count: u32,
        radius: i32,
    },
    Cluster {
        set: String,
        count: u32,
        span: i32,
        radius: i32,
    },
    Biome {
        biome: String,
        radius: i32,
    },
    SpawnBiome {
        biome: String,
    },
    Stronghold {
        radius: i32,
    },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Source {
    Random,
    Random32,
    Sequential,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct FinderSettings {
    pub conditions: Vec<Cond>,
    pub source: Source,
    pub start: String,
    pub threads: usize,
    pub max_results: usize,
}
impl Default for FinderSettings {
    fn default() -> Self {
        Self {
            conditions: vec![
                Cond::Structure {
                    set: "minecraft:villages".into(),
                    count: 2,
                    radius: 250,
                },
                Cond::Structure {
                    set: "minecraft:abandoned_camp".into(),
                    count: 2,
                    radius: 400,
                },
            ],
            source: Source::Random,
            start: "0".into(),
            // One core stays free for the window and the map.
            threads: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
                .saturating_sub(1)
                .max(1),
            max_results: 50,
        }
    }
}

#[derive(Default)]
pub struct Finder {
    search: Option<Search>,
    pub results: Vec<Hit>,
    error: Option<String>,
}

fn biome_mask(name: &str) -> BiomeMask {
    let mut m = BiomeMask::default();
    if let Some(b) = data().biome(name) {
        m.insert(b);
    }
    m
}

/// Dense structure sets (a candidate every chunk or two) get a smaller
/// search radius: past it, one seed alone takes minutes.
pub const DENSE_RADIUS: i32 = 1500;

fn is_dense(set: &str) -> bool {
    catalog()
        .set(set)
        .is_some_and(|s| s.spacing.is_some_and(|spacing| spacing <= 2))
}

fn to_condition(c: &Cond) -> Result<Condition, String> {
    let set = |name: &str| {
        catalog()
            .set(name)
            .map(|s| s.index)
            .ok_or(format!("unknown structure {name}"))
    };
    Ok(match c {
        Cond::Structure {
            set: s,
            count,
            radius,
        } => Condition::Structure {
            set: set(s)?,
            count: *count,
            radius: if is_dense(s) {
                (*radius).min(DENSE_RADIUS)
            } else {
                *radius
            },
        },
        Cond::Cluster {
            set: s,
            count,
            span,
            radius,
        } => Condition::Cluster {
            set: set(s)?,
            count: *count,
            span: *span,
            radius: if is_dense(s) {
                (*radius).min(DENSE_RADIUS)
            } else {
                *radius
            },
        },
        Cond::Biome { biome, radius } => Condition::Biome {
            biomes: biome_mask(biome),
            radius: *radius,
        },
        Cond::SpawnBiome { biome } => Condition::SpawnBiome {
            biomes: biome_mask(biome),
        },
        Cond::Stronghold { radius } => Condition::Stronghold { radius: *radius },
    })
}

fn rate(v: f64) -> String {
    if v >= 1e6 {
        format!("{:.2} M/s", v / 1e6)
    } else if v >= 1e3 {
        format!("{:.1} k/s", v / 1e3)
    } else {
        format!("{v:.0} /s")
    }
}
fn count(v: u64) -> String {
    if v >= 1_000_000_000 {
        format!("{:.2} B", v as f64 / 1e9)
    } else if v >= 1_000_000 {
        format!("{:.2} M", v as f64 / 1e6)
    } else if v >= 1000 {
        format!("{:.1} k", v as f64 / 1e3)
    } else {
        v.to_string()
    }
}

fn structure_combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut String,
) {
    let cat = catalog();
    let label = cat.set(value).map_or("?", |s| s.label);
    egui::ComboBox::from_id_salt(id)
        .selected_text(label)
        .width(170.0)
        .show_ui(ui, |ui| {
            for d in Dimension::ALL {
                ui.label(RichText::new(d.name()).small().color(theme::MUTED));
                for s in cat.sets.iter().filter(|s| s.dimensions.first() == Some(&d)) {
                    let text = if s.new_in_26 {
                        format!("{}  ✦ new", s.label)
                    } else {
                        s.label.to_owned()
                    };
                    ui.selectable_value(value, s.name.clone(), text);
                }
            }
        });
}

pub fn biome_combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut String,
    dim: Option<Dimension>,
) {
    let d = data();
    egui::ComboBox::from_id_salt(id)
        .selected_text(display_name(value))
        .width(170.0)
        .height(360.0)
        .show_ui(ui, |ui| {
            let mut names: Vec<&String> = d.biome_names.iter().collect();
            if dim == Some(Dimension::Overworld) {
                let ow: std::collections::BTreeSet<_> =
                    d.overworld.entries().iter().map(|(_, b)| *b).collect();
                names.retain(|n| d.biome(n).is_some_and(|b| ow.contains(&b)));
            }
            names.sort_by_key(|n| display_name(n));
            for n in names {
                let c = theme::rgb(seedmap_core::biomes::color(n));
                ui.horizontal(|ui| {
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().rect_filled(r, 2.0, c);
                    ui.selectable_value(value, n.clone(), display_name(n));
                });
            }
        });
}

impl Finder {
    pub fn running(&self) -> bool {
        self.search.as_ref().is_some_and(Search::is_running)
    }

    /// The panel. Returns a seed to open on the map.
    /// The panel, scrollable as a whole. Returns a seed to open on the map.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        s: &mut FinderSettings,
        large_biomes: bool,
    ) -> Option<i64> {
        egui::ScrollArea::vertical()
            .id_salt("finder")
            .show(ui, |ui| self.body(ui, s, large_biomes))
            .inner
    }

    fn body(
        &mut self,
        ui: &mut egui::Ui,
        s: &mut FinderSettings,
        large_biomes: bool,
    ) -> Option<i64> {
        let mut open = None;
        let running = self.running();
        ui.horizontal(|ui| {
            ui.heading("Seed Finder");
            ui.label(RichText::new("multi-threaded").small().color(theme::MUTED));
        });
        ui.label(
            RichText::new("Seeds pass stage 1 (structure placement, millions/s), then the real biome and terrain checks.")
                .small()
                .color(theme::MUTED),
        );
        ui.add_space(4.0);

        ui.add_enabled_ui(!running, |ui| {
            theme::section_title(ui, "Presets");
            ui.horizontal_wrapped(|ui| {
                let mut preset = |ui: &mut egui::Ui, name: &str, conds: Vec<Cond>| {
                    if ui.button(name).clicked() {
                        s.conditions = conds;
                    }
                };
                preset(
                    ui,
                    "Village at spawn",
                    vec![Cond::Structure {
                        set: "minecraft:villages".into(),
                        count: 1,
                        radius: 200,
                    }],
                );
                preset(
                    ui,
                    "Quad witch huts",
                    vec![Cond::Cluster {
                        set: "minecraft:swamp_huts".into(),
                        count: 4,
                        span: 200,
                        radius: 3000,
                    }],
                );
                preset(
                    ui,
                    "Mushroom island",
                    vec![Cond::Biome {
                        biome: "minecraft:mushroom_fields".into(),
                        radius: 800,
                    }],
                );
                preset(
                    ui,
                    "Cherry spawn",
                    vec![Cond::SpawnBiome {
                        biome: "minecraft:cherry_grove".into(),
                    }],
                );
                preset(
                    ui,
                    "Close stronghold",
                    vec![Cond::Stronghold { radius: 1600 }],
                );
                preset(
                    ui,
                    "26.x: Abandoned camp + Dappled forest",
                    vec![
                        Cond::Structure {
                            set: "minecraft:abandoned_camp".into(),
                            count: 1,
                            radius: 400,
                        },
                        Cond::Biome {
                            biome: "minecraft:dappled_forest".into(),
                            radius: 400,
                        },
                    ],
                );
            });
            ui.add_space(6.0);
            theme::section_title(ui, "Conditions (all must hold, around 0 0)");
            let mut remove = None;
            for (i, c) in s.conditions.iter_mut().enumerate() {
                theme::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let title = match c {
                            Cond::Structure { .. } => "Structure nearby",
                            Cond::Cluster { .. } => "Structure cluster",
                            Cond::Biome { .. } => "Biome nearby",
                            Cond::SpawnBiome { .. } => "Spawn biome",
                            Cond::Stronghold { .. } => "Stronghold nearby",
                        };
                        ui.label(RichText::new(title).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("✕").on_hover_text("Remove").clicked() {
                                remove = Some(i);
                            }
                        });
                    });
                    match c {
                        Cond::Structure { set, count, radius } => {
                            ui.horizontal(|ui| {
                                ui.label("At least");
                                ui.add(egui::DragValue::new(count).range(1..=16));
                                structure_combo(ui, ("sc", i), set);
                            });
                            ui.horizontal(|ui| {
                                ui.label("within");
                                ui.add(
                                    egui::DragValue::new(radius)
                                        .range(16..=30000)
                                        .speed(10)
                                        .suffix(" blocks"),
                                );
                            });
                            if is_dense(set) && *radius > DENSE_RADIUS {
                                ui.label(
                                    RichText::new(format!(
                                        "Dense structure: searched up to {DENSE_RADIUS} blocks."
                                    ))
                                    .small()
                                    .color(theme::WARN),
                                );
                            }
                        }
                        Cond::Cluster {
                            set,
                            count,
                            span,
                            radius,
                        } => {
                            ui.horizontal(|ui| {
                                ui.add(egui::DragValue::new(count).range(2..=6));
                                structure_combo(ui, ("cl", i), set);
                            });
                            ui.horizontal(|ui| {
                                ui.label("all within");
                                ui.add(
                                    egui::DragValue::new(span)
                                        .range(16..=2000)
                                        .speed(4)
                                        .suffix(" blocks"),
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.label("of each other, within");
                                ui.add(
                                    egui::DragValue::new(radius)
                                        .range(100..=30000)
                                        .speed(20)
                                        .suffix(" blocks"),
                                );
                            });
                            if is_dense(set) && *radius > DENSE_RADIUS {
                                ui.label(
                                    RichText::new(format!(
                                        "Dense structure: searched up to {DENSE_RADIUS} blocks."
                                    ))
                                    .small()
                                    .color(theme::WARN),
                                );
                            }
                        }
                        Cond::Biome { biome, radius } => {
                            ui.horizontal(|ui| {
                                biome_combo(ui, ("bi", i), biome, Some(Dimension::Overworld));
                            });
                            ui.horizontal(|ui| {
                                ui.label("within");
                                ui.add(
                                    egui::DragValue::new(radius)
                                        .range(16..=8000)
                                        .speed(10)
                                        .suffix(" blocks"),
                                );
                            });
                        }
                        Cond::SpawnBiome { biome } => {
                            biome_combo(ui, ("sp", i), biome, Some(Dimension::Overworld));
                        }
                        Cond::Stronghold { radius } => {
                            ui.horizontal(|ui| {
                                ui.label("within");
                                ui.add(
                                    egui::DragValue::new(radius)
                                        .range(100..=10000)
                                        .speed(10)
                                        .suffix(" blocks"),
                                );
                            });
                        }
                    }
                });
            }
            if let Some(i) = remove {
                s.conditions.remove(i);
            }
            ui.menu_button("＋ Add condition", |ui| {
                if ui.button("Structure nearby").clicked() {
                    s.conditions.push(Cond::Structure {
                        set: "minecraft:villages".into(),
                        count: 1,
                        radius: 500,
                    });
                }
                if ui.button("Structure cluster (quad huts…)").clicked() {
                    s.conditions.push(Cond::Cluster {
                        set: "minecraft:swamp_huts".into(),
                        count: 3,
                        span: 200,
                        radius: 3000,
                    });
                }
                if ui.button("Biome nearby").clicked() {
                    s.conditions.push(Cond::Biome {
                        biome: "minecraft:cherry_grove".into(),
                        radius: 500,
                    });
                }
                if ui.button("Spawn biome").clicked() {
                    s.conditions.push(Cond::SpawnBiome {
                        biome: "minecraft:plains".into(),
                    });
                }
                if ui.button("Stronghold nearby").clicked() {
                    s.conditions.push(Cond::Stronghold { radius: 1800 });
                }
            });
            ui.add_space(6.0);
            theme::section_title(ui, "Search");
            egui::Grid::new("search opts")
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Seeds");
                    egui::ComboBox::from_id_salt("source")
                        .selected_text(match s.source {
                            Source::Random => "Random (64-bit)",
                            Source::Random32 => "Random (32-bit, text seeds)",
                            Source::Sequential => "Counting up",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut s.source, Source::Random, "Random (64-bit)");
                            ui.selectable_value(
                                &mut s.source,
                                Source::Random32,
                                "Random (32-bit, text seeds)",
                            );
                            ui.selectable_value(&mut s.source, Source::Sequential, "Counting up");
                        });
                    ui.end_row();
                    if s.source == Source::Sequential {
                        ui.label("From");
                        ui.text_edit_singleline(&mut s.start);
                        ui.end_row();
                    }
                    let max = std::thread::available_parallelism()
                        .map(|n| n.get())
                        .unwrap_or(4);
                    ui.label("Threads");
                    ui.add(egui::Slider::new(&mut s.threads, 1..=max));
                    ui.end_row();
                    ui.label("Stop after");
                    ui.add(
                        egui::DragValue::new(&mut s.max_results)
                            .range(1..=10000)
                            .suffix(" seeds"),
                    );
                    ui.end_row();
                });
        });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if running {
                let stop = egui::Button::new(RichText::new("■  Stop").strong())
                    .fill(egui::Color32::from_rgb(150, 50, 50));
                if ui.add_sized([120.0, 32.0], stop).clicked()
                    && let Some(search) = &self.search
                {
                    search.stop();
                }
            } else {
                let go = egui::Button::new(
                    RichText::new("▶  Search")
                        .strong()
                        .color(egui::Color32::BLACK),
                )
                .fill(theme::ACCENT);
                if ui.add_sized([120.0, 32.0], go).clicked() {
                    self.error = None;
                    match s
                        .conditions
                        .iter()
                        .map(to_condition)
                        .collect::<Result<Vec<_>, _>>()
                    {
                        Ok(conditions) if conditions.is_empty() => {
                            self.error = Some("Add a condition first.".into())
                        }
                        Ok(conditions) => {
                            let source = match s.source {
                                Source::Random => SeedSource::Random,
                                Source::Random32 => SeedSource::Random32,
                                Source::Sequential => SeedSource::Sequential {
                                    start: seedmap_core::search::parse_seed(&s.start),
                                },
                            };
                            self.results.clear();
                            self.search = Some(start(SearchConfig {
                                conditions,
                                source,
                                threads: s.threads,
                                large_biomes,
                                max_results: s.max_results,
                            }));
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
            }
            if !self.results.is_empty() && ui.button("Copy all").clicked() {
                let text = self
                    .results
                    .iter()
                    .map(|h| h.seed.to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                ui.ctx().copy_text(text);
            }
        });
        if let Some(e) = &self.error {
            ui.colored_label(theme::WARN, e);
        }
        if let Some(search) = &self.search {
            let skipped = search.progress.skipped.load(Ordering::Relaxed);
            if skipped > 0 {
                let errors = search
                    .progress
                    .errors
                    .lock()
                    .map(|e| e.join("\n"))
                    .unwrap_or_default();
                let log = crate::crash_log_path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                ui.colored_label(
                    theme::WARN,
                    format!("{skipped} seed(s) hit an internal error and were skipped."),
                )
                .on_hover_text(format!("{errors}\n\nDetails: {log}"));
            }
        }

        if let Some(search) = &self.search {
            let p = &search.progress;
            {
                let results = p.results.lock().unwrap();
                if results.len() != self.results.len() {
                    self.results = results.clone();
                }
            }
            let checked = p.checked.load(Ordering::Relaxed);
            let stage = p.stage_one.load(Ordering::Relaxed);
            let running = search.is_running();
            let tiles = [
                ("Speed", rate(search.seeds_per_second()), true),
                ("Found", self.results.len().to_string(), false),
                ("Checked", count(checked), false),
                ("Passed stage 1", count(stage), false),
            ];
            egui::Grid::new("stats")
                .num_columns(2)
                .spacing([6.0, 6.0])
                .min_col_width((ui.available_width() - 6.0) / 2.0)
                .show(ui, |ui| {
                    for (i, (label, value, accent)) in tiles.iter().enumerate() {
                        theme::card().show(ui, |ui| {
                            ui.set_min_width((ui.available_width()).max(120.0));
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                ui.label(RichText::new(*label).size(11.0).color(theme::MUTED));
                                ui.label(
                                    RichText::new(value)
                                        .font(theme::bold(19.0))
                                        .color(if *accent { theme::ACCENT } else { theme::TEXT }),
                                );
                            });
                        });
                        if i % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
            ui.label(
                RichText::new(format!(
                    "{} {:.1} s on {} threads",
                    if running {
                        "Searching for"
                    } else {
                        "Searched for"
                    },
                    search.elapsed().as_secs_f64(),
                    s.threads
                ))
                .small()
                .color(theme::MUTED),
            );
            if search.is_running() {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(150));
            }
        }

        ui.add_space(6.0);
        if !self.results.is_empty() {
            theme::section_title(ui, "Results");
        }
        {
            {
                for hit in &self.results {
                    theme::card().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(hit.seed.to_string()).monospace().strong());
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button("Open").clicked() {
                                        open = Some(hit.seed);
                                    }
                                    if ui.small_button("Copy").clicked() {
                                        ui.ctx().copy_text(hit.seed.to_string());
                                    }
                                },
                            );
                        });
                        for n in &hit.notes {
                            ui.label(RichText::new(n).small().color(theme::MUTED));
                        }
                    });
                }
            }
        }
        open
    }
}
