//! What level of monster the land holds: for each outdoor landblock, the creatures its encounters
//! and its outdoor generators spawn, followed through generators of generators, each with its
//! level and a weight (the chance it is the one spawned). Built a landblock at a time, as asked.

use std::collections::HashMap;
use std::sync::Arc;

use empyrean_entity::enums::{PropertyBool, PropertyInt, WeenieType};

use super::towns;
use crate::World;

/// How deep generators of generators are followed.
const GENERATOR_DEPTH: u32 = 4;
/// Landblocks round a town whose creatures set its level: its own and the ring next to it.
const TOWN_RINGS: i32 = 1;
/// The share of those creatures below a town's level: their median.
const TOWN_PERCENTILE: f32 = 0.5;

/// Levels, each with its weight.
pub type Spawned = Arc<Vec<(u32, f32)>>;

/// What has been worked out so far.
#[derive(Debug, Default)]
pub struct Levels {
    blocks: HashMap<u16, Spawned>,
    weenies: HashMap<u32, Spawned>,
    towns: HashMap<usize, u32>,
}

/// The creatures weenie `wcid` stands for: itself when it is one anyone may fight, else what it
/// generates.
fn creatures(w: &mut World, wcid: u32, depth: u32) -> Spawned {
    if let Some(known) = w.ai_players.levels.weenies.get(&wcid) {
        return Arc::clone(known);
    }
    let mut out = Vec::new();
    if let Some(weenie) = w.content.get_cached_weenie(wcid) {
        if weenie.weenie_type == WeenieType::Creature {
            let attackable = weenie
                .properties_bool
                .as_ref()
                .and_then(|b| b.get(&PropertyBool::Attackable).copied())
                .unwrap_or(true);
            let level = weenie
                .properties_int
                .as_ref()
                .and_then(|i| i.get(&PropertyInt::Level).copied())
                .and_then(|l| u32::try_from(l).ok())
                .filter(|&l| l > 0);
            if let (true, Some(level)) = (attackable, level) {
                out.push((level, 1.0));
            }
        } else if depth < GENERATOR_DEPTH {
            for g in weenie.properties_generator.iter().flat_map(|g| g.iter()) {
                if g.weenie_class_id == wcid {
                    continue;
                }
                let chance = if g.probability > 0.0 && g.probability <= 1.0 {
                    g.probability
                } else {
                    1.0
                };
                for &(level, weight) in creatures(w, g.weenie_class_id, depth + 1).iter() {
                    out.push((level, weight * chance));
                }
            }
        }
    }
    let out = Arc::new(out);
    w.ai_players.levels.weenies.insert(wcid, Arc::clone(&out));
    out
}

/// The creatures outdoor landblock `block` spawns.
pub fn of_block(w: &mut World, block: u16) -> Spawned {
    if let Some(known) = w.ai_players.levels.blocks.get(&block) {
        return Arc::clone(known);
    }
    let mut out = Vec::new();
    let encounters = w.content.get_cached_encounters_by_landblock(block);
    for e in encounters.iter() {
        out.extend(creatures(w, e.weenie_class_id, 0).iter().copied());
    }
    let instances = w.content.get_cached_instances_by_landblock(block);
    for i in instances.iter().filter(|i| i.obj_cell_id & 0xFFFF < 0x100) {
        out.extend(creatures(w, i.weenie_class_id, 0).iter().copied());
    }
    let out = Arc::new(out);
    w.ai_players.levels.blocks.insert(block, Arc::clone(&out));
    out
}

/// The creatures of the landblocks within `rings` of `block`.
pub fn around(w: &mut World, block: u16, rings: i32) -> Vec<(u32, f32)> {
    let (bx, by) = (i32::from(block >> 8), i32::from(block & 0xFF));
    let mut out = Vec::new();
    for x in (bx - rings).max(0)..=(bx + rings).min(0xFF) {
        for y in (by - rings).max(0)..=(by + rings).min(0xFF) {
            let b = u16::try_from((x << 8) | y).unwrap_or(0);
            out.extend(of_block(w, b).iter().copied());
        }
    }
    out
}

/// The level below which share `p` of the weighted `levels` fall.
#[must_use]
pub fn percentile(levels: &[(u32, f32)], p: f32) -> Option<u32> {
    let mut sorted: Vec<(u32, f32)> = levels.iter().copied().filter(|l| l.1 > 0.0).collect();
    sorted.sort_by_key(|l| l.0);
    let total: f32 = sorted.iter().map(|l| l.1).sum();
    if total <= 0.0 {
        return None;
    }
    let mut seen = 0.0;
    for (level, weight) in &sorted {
        seen += weight;
        if seen >= total * p {
            return Some(*level);
        }
    }
    sorted.last().map(|l| l.0)
}

/// The landblock at map coordinates `ns`, `ew`.
#[must_use]
pub fn block_at(ns: f64, ew: f64) -> u16 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let at = |v: f64| (((v + 101.95) * 240.0 / 192.0).floor().clamp(0.0, 255.0)) as u16;
    at(ew) << 8 | at(ns)
}

/// Town levels the operator set (`[ai_players] town_levels`), by name.
fn overrides() -> HashMap<String, u32> {
    let text = empyrean_common::config_manager::ConfigManager::try_config()
        .map(|c| c.ai_players.town_levels.clone())
        .unwrap_or_default();
    parse_overrides(&text)
}

/// `Holtburg=5, Arwic=12` as names (lower case) and levels.
#[must_use]
pub fn parse_overrides(text: &str) -> HashMap<String, u32> {
    text.split(',')
        .filter_map(|pair| {
            let (name, level) = pair.split_once('=')?;
            Some((name.trim().to_lowercase(), level.trim().parse().ok()?))
        })
        .collect()
}

/// Town `town`'s level: the operator's, else the median level of the creatures in and next to its
/// landblock, else the first guess.
pub fn town_level(w: &mut World, town: usize) -> u32 {
    if let Some(&known) = w.ai_players.levels.towns.get(&town) {
        return known;
    }
    let t = towns::TOWNS[town];
    let level = overrides()
        .get(&t.name.to_lowercase())
        .copied()
        .or_else(|| {
            let land = around(w, block_at(t.ns, t.ew), TOWN_RINGS);
            percentile(&land, TOWN_PERCENTILE)
        })
        .unwrap_or(t.level)
        .max(1);
    w.ai_players.levels.towns.insert(town, level);
    level
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_percentile_weighs_each_level() {
        let levels = [(5, 1.0), (10, 1.0), (20, 2.0)];
        assert_eq!(percentile(&levels, 0.25), Some(5));
        assert_eq!(percentile(&levels, 0.5), Some(10));
        assert_eq!(percentile(&levels, 0.6), Some(20));
        assert_eq!(percentile(&[], 0.5), None);
    }

    #[test]
    fn overrides_are_names_and_levels() {
        let o = parse_overrides("Holtburg=5, Arwic = 12,bad, Yaraq=x");
        assert_eq!(o.get("holtburg"), Some(&5));
        assert_eq!(o.get("arwic"), Some(&12));
        assert_eq!(o.len(), 2);
    }

    #[test]
    fn holtburg_is_landblock_a9b4() {
        assert_eq!(block_at(42.28, 33.31), 0xA9B4);
    }
}
