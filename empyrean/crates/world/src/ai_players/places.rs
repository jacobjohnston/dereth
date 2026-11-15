//! The places in a town an AI Player has errands at: its shops, its lifestone, its portals and its
//! townsfolk, read from the world's own spawns round the town, and spots near them that no other
//! AI Player is standing on or heading to.

use std::sync::Arc;

use empyrean_entity::enums::{PropertyBool, WeenieType};

use super::{towns, walk};
use crate::World;
use dereth_primitives::num::math as m;

/// Metres from a town's centre its places may stand.
const REACH: f32 = 160.0;
/// No two AI Players stop closer than this.
const ROOM: f32 = 2.5;

/// What a place is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Shop,
    Lifestone,
    Portal,
    Townsfolk,
}

impl Kind {
    /// Seconds an AI Player spends at one: shopping takes longest.
    #[must_use]
    pub const fn dwell(self) -> (f64, f64) {
        match self {
            Self::Shop => (20.0, 60.0),
            Self::Lifestone => (5.0, 15.0),
            Self::Portal => (5.0, 20.0),
            Self::Townsfolk => (8.0, 25.0),
        }
    }

    /// How often it is chosen, against the others.
    #[must_use]
    pub const fn weight(self) -> f32 {
        match self {
            Self::Shop => 5.0,
            Self::Lifestone => 1.5,
            Self::Portal => 1.0,
            Self::Townsfolk => 1.5,
        }
    }
}

/// One place: what it is, where (world metres) and whether it is inside a building.
#[derive(Debug, Clone, Copy)]
pub struct Place {
    pub kind: Kind,
    pub at: [f32; 2],
    pub indoors: bool,
}

/// Town `town`'s places, read once.
pub fn of_town(w: &mut World, town: usize) -> Arc<Vec<Place>> {
    if let Some(known) = w.ai_players.places.get(&town) {
        return Arc::clone(known);
    }
    let t = towns::TOWNS[town];
    #[allow(clippy::cast_possible_truncation)]
    let centre = [
        ((t.ew + 101.95) * 240.0) as f32,
        ((t.ns + 101.95) * 240.0) as f32,
    ];
    let block = super::levels::block_at(t.ns, t.ew);
    let (bx, by) = (i32::from(block >> 8), i32::from(block & 0xFF));
    let mut out = Vec::new();
    for x in (bx - 1).max(0)..=(bx + 1).min(0xFF) {
        for y in (by - 1).max(0)..=(by + 1).min(0xFF) {
            let b = u16::try_from((x << 8) | y).unwrap_or(0);
            let instances = w.content.get_cached_instances_by_landblock(b);
            for i in instances.iter() {
                #[allow(clippy::cast_precision_loss)]
                let at = [x as f32 * 192.0 + i.origin_x, y as f32 * 192.0 + i.origin_y];
                if m::hypotf(at[0] - centre[0], at[1] - centre[1]) > REACH {
                    continue;
                }
                let Some(weenie) = w.content.get_cached_weenie(i.weenie_class_id) else {
                    continue;
                };
                let kind = match weenie.weenie_type {
                    WeenieType::Vendor => Kind::Shop,
                    WeenieType::LifeStone => Kind::Lifestone,
                    WeenieType::Portal => Kind::Portal,
                    WeenieType::Creature => {
                        let attackable = weenie
                            .properties_bool
                            .as_ref()
                            .and_then(|b| b.get(&PropertyBool::Attackable).copied())
                            .unwrap_or(true);
                        if attackable {
                            continue;
                        }
                        Kind::Townsfolk
                    }
                    _ => continue,
                };
                out.push(Place {
                    kind,
                    at,
                    indoors: i.obj_cell_id & 0xFFFF >= 0x100,
                });
            }
        }
    }
    let out = Arc::new(out);
    w.ai_players.places.insert(town, Arc::clone(&out));
    out
}

/// A place of town `town` to go to, by the kinds' weights, not `not` (the last one).
pub fn pick(w: &mut World, town: usize, not: Option<[f32; 2]>) -> Option<Place> {
    let places = of_town(w, town);
    let choices: Vec<&Place> = places
        .iter()
        .filter(|p| not.is_none_or(|n| m::hypotf(p.at[0] - n[0], p.at[1] - n[1]) > 3.0))
        .collect();
    let total: f32 = choices.iter().map(|p| p.kind.weight()).sum();
    if total <= 0.0 {
        return None;
    }
    let mut roll = w.ai_players.unit() * total;
    for p in &choices {
        roll -= p.kind.weight();
        if roll <= 0.0 {
            return Some(**p);
        }
    }
    choices.last().map(|p| **p)
}

/// A spot from `near` to `far` metres round `at` that no other AI Player stands on or heads for.
pub fn free_spot(w: &mut World, me: u32, at: [f32; 2], near: f32, far: f32) -> [f32; 2] {
    let taken: Vec<[f32; 2]> = w
        .ai_players
        .bots
        .iter()
        .filter(|(id, _)| **id != me)
        .flat_map(|(_, b)| {
            let heading = b.walk.as_ref().map(|wk| wk.to);
            let standing = w
                .sessions
                .player(b.session)
                .and_then(|p| w.objects.get(p))
                .and_then(crate::world_objects::world_object::WorldObject::location)
                .map(|l| {
                    let g = walk::global(&l);
                    [g[0], g[1]]
                });
            heading.into_iter().chain(standing)
        })
        .collect();
    let mut spot = at;
    for _ in 0..8 {
        let angle = w.ai_players.unit() * std::f32::consts::TAU;
        let r = near + w.ai_players.unit() * (far - near);
        spot = [at[0] + m::cosf(angle) * r, at[1] + m::sinf(angle) * r];
        if taken
            .iter()
            .all(|t| m::hypotf(t[0] - spot[0], t[1] - spot[1]) >= ROOM)
        {
            break;
        }
    }
    spot
}
