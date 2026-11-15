//! Hunters: AI Players out in the land round real players, each on a hunting ground of its level,
//! soloing the monsters there through the handlers a client's attacks and casts reach.
//!
//! A hunter leaves alone any monster a real player has hurt in the last three minutes or that is
//! fighting one, and anything near a real player. It never runs out of arrows or mana, binds its
//! wounds when hurt (its kits never run out), and after a death walks back from its lifestone.

use dereth_primitives::num::math as m;
use empyrean_entity::enums::{AttackHeight, CombatMode, MagicSchool, PropertyInt, Skill};
use empyrean_entity::ObjectGuid;

use super::{levels, towns, walk, Stage};
use crate::entity::spell::Spell;
use crate::world_objects::{
    creature_combat, creature_equipment, monster_awareness, monster_combat, monster_navigation,
    player_combat, player_magic, player_melee, player_missile,
};
use crate::World;

/// At most this many hunters within 5 km of a real player.
pub const PER_PLAYER: usize = 10;
/// Metres round its ground's centre a hunter hunts.
const RANGE: f32 = 70.0;
/// Seconds between looks for a monster.
const LOOK_EVERY: f64 = 2.0;
/// A monster this close to a real player is theirs.
const KEEP_CLEAR: f64 = 40.0;
/// How much above its own level a hunter will take on.
const OVER: u32 = 3;
/// Seconds between a hunter's attacks: melee, missile, spell.
const MELEE_EVERY: f64 = 2.0;
const MISSILE_EVERY: f64 = 2.5;
const CAST_EVERY: f64 = 3.5;
/// How close a hunter stands to attack: melee, missile, spell.
const MELEE_REACH: f32 = 1.5;
const MISSILE_REACH: f32 = 25.0;
const CAST_REACH: f32 = 30.0;
/// Health fractions: bind wounds out of a fight below the first, in one below the second.
const BIND_AT: f32 = 0.7;
const BIND_IN_FIGHT_AT: f32 = 0.35;
/// Seconds between bound wounds, and how much of its health each gives back.
const BIND_EVERY: f64 = 6.0;
const BIND_SHARE: f32 = 0.3;

/// How a hunter fights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Melee,
    Missile,
    Magic(MagicSchool),
}

/// A hunter's state.
#[derive(Debug, Clone)]
pub struct Hunt {
    /// The landblock it hunts, and its centre in world metres.
    pub ground: u16,
    pub centre: [f32; 2],
    /// Whether it has reached its ground since it last left it.
    pub arrived: bool,
    pub target: Option<ObjectGuid>,
    next_look: f64,
    next_attack: f64,
    next_bind: f64,
    /// Attacks made, monsters it has dropped and why the last was dropped (for operators).
    pub attacks: u32,
    pub kills: u32,
    pub dropped: u32,
    pub why: &'static str,
}

impl Hunt {
    #[must_use]
    pub fn new(ground: u16) -> Self {
        Self {
            ground,
            centre: centre_of(ground),
            arrived: false,
            target: None,
            next_look: 0.0,
            next_attack: 0.0,
            next_bind: 0.0,
            attacks: 0,
            kills: 0,
            dropped: 0,
            why: "",
        }
    }
}

/// Landblock `b`'s centre in world metres.
#[must_use]
pub fn centre_of(b: u16) -> [f32; 2] {
    [
        f32::from(b >> 8) * 192.0 + 96.0,
        f32::from(b & 0xFF) * 192.0 + 96.0,
    ]
}

/// A landblock's centre as map coordinates: north, then east.
#[must_use]
pub fn map_centre(b: u16) -> (f64, f64) {
    let c = centre_of(b);
    (
        f64::from(c[1]) / 240.0 - 101.95,
        f64::from(c[0]) / 240.0 - 101.95,
    )
}

/// The level a hunter of ground `b` is: enough to solo what lives there.
pub fn level_for(w: &mut World, b: u16) -> Option<u32> {
    let land = levels::of_block(w, b);
    let median = levels::percentile(&land, 0.5)?;
    let max = crate::world_objects::player_xp::get_max_level(w);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let level = (f64::from(median) * 1.15).round() as u32 + OVER;
    Some(level.clamp(1, max))
}

/// A hunting ground within `radius` metres of map coordinates `at`, with monsters to hunt, at
/// least `clear` metres from every real player: tried `tries` times at random.
pub fn pick_ground(
    w: &mut World,
    at: (f64, f64),
    radius: f64,
    anchors: &[(f64, f64)],
    clear: f64,
) -> Option<u16> {
    for _ in 0..40 {
        let angle = w.ai_players.unit() * std::f32::consts::TAU;
        let r = f64::from(w.ai_players.unit().sqrt()) * radius / 240.0;
        let ns = at.0 + r * f64::from(m::sinf(angle));
        let ew = at.1 + r * f64::from(m::cosf(angle));
        let b = levels::block_at(ns, ew);
        let c = map_centre(b);
        if anchors.iter().any(|a| super::distance_m(*a, c) < clear) {
            continue;
        }
        if !levels::of_block(w, b).is_empty() {
            return Some(b);
        }
    }
    None
}

/// The town nearest map coordinates `at`, of the world's era.
pub fn nearest_town(w: &World, at: (f64, f64)) -> usize {
    towns::of_era(w.era.id)
        .map(|(i, t)| (i, super::distance_m(at, (t.ns, t.ew))))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(0, |(i, _)| i)
}

/// How hunter `p` fights: its wielded weapon's way, or its best attacking school.
pub fn style(w: &mut World, p: ObjectGuid) -> Style {
    if creature_equipment::get_equipped_missile_weapon(w, p).is_some() {
        return Style::Missile;
    }
    if creature_equipment::get_equipped_wand(w, p).is_some() {
        for (school, skill) in [
            (MagicSchool::WarMagic, Skill::WarMagic),
            (MagicSchool::VoidMagic, Skill::VoidMagic),
        ] {
            let trained = w
                .objects
                .get_mut(p)
                .and_then(|o| o.get_creature_skill(skill, true))
                .zip(w.objects.get(p))
                .is_some_and(|(s, o)| {
                    s.advancement_class(o) >= empyrean_entity::enums::SkillAdvancementClass::Trained
                });
            if trained {
                return Style::Magic(school);
            }
        }
    }
    Style::Melee
}

/// Whether real players have a claim on `monster`: one hurt it lately, or it is fighting one.
#[must_use]
pub fn claimed_by_real(w: &World, monster: ObjectGuid) -> bool {
    let hurt = crate::entity::damage_history::of(w, monster)
        .damagers()
        .iter()
        .any(|d| {
            w.objects.get(d.guid).is_some_and(|o| o.is_player())
                && !super::is_ai_player(w, d.guid.full())
        });
    let fighting = monster_awareness::is_awake(w, monster)
        && monster_combat::attack_target(w, monster).is_some_and(|t| {
            w.objects.get(t).is_some_and(|o| o.is_player()) && !super::is_ai_player(w, t.full())
        });
    hurt || fighting
}

/// Whether `monster` is one hunter `p` of level `level` takes on.
fn huntable(w: &World, p: ObjectGuid, monster: ObjectGuid, level: u32) -> bool {
    let Some(o) = w.objects.get(monster) else {
        return false;
    };
    if o.is_player() || !o.attackable() || monster_combat::is_dead(o) {
        return false;
    }
    let lvl = o.get_property(PropertyInt::Level).unwrap_or(0);
    if lvl <= 0 || u32::try_from(lvl).unwrap_or(u32::MAX) > level + OVER {
        return false;
    }
    if o.get_property(empyrean_entity::enums::PropertyBool::NpcLooksLikeObject)
        .unwrap_or(false)
    {
        return false;
    }
    // Only on awake land: a sleeping landblock's monsters do not fight back.
    let awake = o
        .current_landblock
        .and_then(|lb| w.landblock_manager.landblocks.get(lb))
        .is_some_and(|l| !l.is_dormant);
    if !awake {
        return false;
    }
    let _ = p;
    !claimed_by_real(w, monster)
}

/// The nearest monster hunter `p` may take on, within its ground, not another hunter's, and clear
/// of real players.
fn look(
    w: &mut World,
    id: u32,
    p: ObjectGuid,
    hunt: &Hunt,
    anchors: &[(f64, f64)],
) -> Option<ObjectGuid> {
    let level = w
        .objects
        .get(p)
        .and_then(|o| o.level())
        .and_then(|l| u32::try_from(l).ok())
        .unwrap_or(1);
    let h = w.objects.get(p)?.phys?;
    let taken: Vec<ObjectGuid> = w
        .ai_players
        .bots
        .iter()
        .filter(|(other, _)| **other != id)
        .filter_map(|(_, b)| b.hunt.as_ref().and_then(|h| h.target))
        .collect();
    let mut best: Option<(ObjectGuid, f64)> = None;
    for monster in crate::physics::object_maint::get_visible_objects_values_of_type_creature(w, h) {
        if taken.contains(&monster) || !huntable(w, p, monster, level) {
            continue;
        }
        let Some(at) = w.objects.get(monster).and_then(|o| o.location()) else {
            continue;
        };
        if at.indoors() {
            continue;
        }
        let g = walk::global(&at);
        if m::hypotf(g[0] - hunt.centre[0], g[1] - hunt.centre[1]) > RANGE {
            continue;
        }
        let mc = super::map_coords(&at);
        if anchors
            .iter()
            .any(|a| super::distance_m(*a, mc) < KEEP_CLEAR)
        {
            continue;
        }
        let d = monster_navigation::get_distance_to_object(w, p, monster, true);
        if best.is_none_or(|(_, bd)| d < bd) {
            best = Some((monster, d));
        }
    }
    best.map(|(m, _)| m)
}

/// The highest-level attacking spell of `school` hunter `p` knows, one aimed at one target.
fn attack_spell(w: &World, p: ObjectGuid, school: MagicSchool) -> Option<u32> {
    let book = w.objects.get(p)?.biota.properties_spell_book.as_ref()?;
    let mut best: Option<(u32, u32, bool)> = None;
    for id in book.keys().filter_map(|k| u32::try_from(*k).ok()) {
        let spell = Spell::new(w, id, false);
        if spell.school() != school || spell.is_beneficial() || spell.is_self_targeted() {
            continue;
        }
        let name = spell.name();
        let single = ["Bolt", "Arc", "Streak", "Lightning", "Nether"]
            .iter()
            .any(|n| name.contains(n))
            && !["Blast", "Volley", "Ring", "Wall"]
                .iter()
                .any(|n| name.contains(n));
        let level = spell.formula_ref().level();
        let better = best.is_none_or(|(_, bl, bs)| (single, level) > (bs, bl));
        if better {
            best = Some((id, level, single));
        }
    }
    best.map(|(id, _, _)| id)
}

/// A hunter's turn: to its ground, then look, close in and fight, binding its wounds.
pub fn live(w: &mut World, id: u32, p: ObjectGuid, now: f64, anchors: &[(f64, f64)]) {
    let Some(mut hunt) = w.ai_players.bots.get(&id).and_then(|b| b.hunt.clone()) else {
        return;
    };
    bind(w, p, &mut hunt, now);

    if !hunt.arrived {
        travel(w, id, p, &mut hunt, now);
        set(w, id, hunt);
        return;
    }

    // The target: still there, alive, and not a real player's?
    if let Some(t) = hunt.target {
        let why = if w.objects.get(t).is_none() {
            Some("gone")
        } else if w.objects.get(t).is_some_and(monster_combat::is_dead) {
            Some("dead")
        } else if claimed_by_real(w, t) {
            Some("a real player's")
        } else {
            None
        };
        if let Some(why) = why {
            if why == "dead" {
                hunt.kills += 1;
            }
            hunt.target = None;
            hunt.dropped += 1;
            hunt.why = why;
            hunt.next_look = now + 1.0;
        }
    }
    if hunt.target.is_none() && now >= hunt.next_look {
        hunt.next_look = now + LOOK_EVERY;
        hunt.target = look(w, id, p, &hunt, anchors);
        if hunt.target.is_none() {
            wander(w, id, p, &hunt, now);
        }
    }
    if let Some(t) = hunt.target {
        fight(w, id, p, t, &mut hunt, now);
    } else if let Some(b) = w.ai_players.bots.get(&id) {
        if b.walk.as_ref().is_some_and(|wk| wk.outcome.is_none()) {
            walk::step(w, id, p, now);
        }
    }
    set(w, id, hunt);
}

fn set(w: &mut World, id: u32, hunt: Hunt) {
    if let Some(b) = w.ai_players.bots.get_mut(&id) {
        b.hunt = Some(hunt);
    }
}

/// Walking out to its ground.
fn travel(w: &mut World, id: u32, p: ObjectGuid, hunt: &mut Hunt, now: f64) {
    let walking = w.ai_players.bots.get(&id).and_then(|b| b.walk.clone());
    match walking.map(|wk| wk.outcome) {
        None if now < hunt.next_look => {}
        None => walk::go(w, id, hunt.centre, 40.0),
        Some(None) => walk::step(w, id, p, now),
        Some(Some(walk::Outcome::Arrived)) => {
            hunt.arrived = true;
            clear_walk(w, id);
        }
        Some(Some(walk::Outcome::Failed)) => {
            // Somewhere it cannot walk: try again from where it is, in a while.
            clear_walk(w, id);
            hunt.next_look = now + 5.0;
        }
    }
}

fn clear_walk(w: &mut World, id: u32) {
    if let Some(b) = w.ai_players.bots.get_mut(&id) {
        b.walk = None;
    }
}

/// No monster in sight: walk somewhere else on the ground.
fn wander(w: &mut World, id: u32, p: ObjectGuid, hunt: &Hunt, now: f64) {
    let busy = w
        .ai_players
        .bots
        .get(&id)
        .and_then(|b| b.walk.as_ref())
        .is_some_and(|wk| wk.outcome.is_none());
    if busy {
        return;
    }
    if creature_combat::combat_mode(w, p) != CombatMode::NonCombat {
        player_combat::handle_action_change_combat_mode(w, p, CombatMode::NonCombat, false, None);
    }
    let angle = w.ai_players.unit() * std::f32::consts::TAU;
    let r = w.ai_players.unit().sqrt() * RANGE * 0.8;
    let to = [
        hunt.centre[0] + m::cosf(angle) * r,
        hunt.centre[1] + m::sinf(angle) * r,
    ];
    let _ = now;
    walk::go(w, id, to, 5.0);
}

/// Close in on `t` and attack it the hunter's way.
fn fight(w: &mut World, id: u32, p: ObjectGuid, t: ObjectGuid, hunt: &mut Hunt, now: f64) {
    let style = style(w, p);
    let (mode, reach, every) = match style {
        Style::Melee => (CombatMode::Melee, MELEE_REACH, MELEE_EVERY),
        Style::Missile => (CombatMode::Missile, MISSILE_REACH, MISSILE_EVERY),
        Style::Magic(_) => (CombatMode::Magic, CAST_REACH, CAST_EVERY),
    };
    #[allow(clippy::cast_possible_truncation)]
    let d = monster_navigation::get_distance_to_object(w, p, t, true) as f32;
    if d > reach {
        // Run at it, straight.
        let Some(at) = w.objects.get(t).and_then(|o| o.location()) else {
            return;
        };
        let g = walk::global(&at);
        walk::chase(w, id, p, [g[0], g[1]], reach * 0.8, now);
        return;
    }
    walk::halt_here(w, id, p);
    if creature_combat::combat_mode(w, p) != mode {
        player_combat::handle_action_change_combat_mode(w, p, mode, false, None);
        hunt.next_attack = now + 1.5;
        return;
    }
    if now < hunt.next_attack || player_magic::is_busy(w, p) {
        return;
    }
    hunt.next_attack = now + every;
    hunt.attacks += 1;
    let height = u32::try_from(AttackHeight::Medium.0).unwrap_or(2);
    match style {
        Style::Melee => {
            player_melee::handle_action_targeted_melee_attack(w, p, t.full(), height, 0.75);
        }
        Style::Missile => {
            player_missile::handle_action_targeted_missile_attack(w, p, t.full(), height, 0.75);
        }
        Style::Magic(school) => {
            if let Some(spell) = attack_spell(w, p, school) {
                player_magic::handle_action_cast_targeted_spell(w, p, t.full(), spell, None);
            }
        }
    }
}

/// Bind its wounds when hurt: out of a fight below 70% health, in one below 35%.
fn bind(w: &mut World, p: ObjectGuid, hunt: &mut Hunt, now: f64) {
    if now < hunt.next_bind {
        return;
    }
    let Some(o) = w.objects.get(p) else { return };
    if monster_combat::is_dead(o) {
        return;
    }
    let health = o.health();
    let current = health.current(o);
    let max = health
        .max_value(&mut crate::world_objects::entity::creature_attribute::StatCtx::in_world(w, p));
    if max == 0 {
        return;
    }
    #[allow(clippy::cast_precision_loss)]
    let share = current as f32 / max as f32;
    let at = if hunt.target.is_some() {
        BIND_IN_FIGHT_AT
    } else {
        BIND_AT
    };
    if share >= at {
        return;
    }
    hunt.next_bind = now + BIND_EVERY;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let heal = (max as f32 * BIND_SHARE) as i32;
    crate::world_objects::creature_vitals::update_vital_delta(w, p, health, heal);
}

/// Whether hunter `id` is in the world and out of portal space.
#[must_use]
pub fn ready(w: &World, id: u32) -> bool {
    w.ai_players
        .bots
        .get(&id)
        .is_some_and(|b| b.stage == Stage::InWorld && b.placed)
}
