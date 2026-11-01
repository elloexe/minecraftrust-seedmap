//! The Minecraft Rust 26.3 seed engine.
//!
//! Biomes come from Minecraft Rust's own world generator (the `claudium-world`
//! crate): its density-function compiler, climate index and End source, fed
//! with the 26.3 worldgen data. Structure placement, spawn and seed search
//! are built on its structure registry and random sources.
// Grid samplers take their lattice as plain numbers, like the density API.
#![allow(clippy::too_many_arguments)]

pub mod biomes;
pub mod data;
pub mod search;
pub mod structures;
pub mod world;

pub use biomes::BiomeId;
pub use data::{Data, Preset, data};
pub use world::{Dimension, Sampler, World, WorldSpec};
