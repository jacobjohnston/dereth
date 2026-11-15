//! Town buffers: town AI Players who know buffs give real players nearby the ones they lack.
//!
//! A buffer's buffs are its best spell of each kind (stacking category) that it can cast on
//! someone else: beneficial creature and life enchantments, not item ones. Every few seconds an
//! idle buffer looks for a real player within 60 m whom no other buffer is buffing and who has
//! not been buffed in the last ten minutes, then walks near and casts up to five buffs the player lacks,
//! has weaker, or has with little time left; a skill's buff only if the player trains the skill. It reads the player's enchantments directly, as a
//! player inspecting them would see them.

use empyrean_entity::enums::{
    CombatMode, EnchantmentTypeFlags, MagicSchool, Skill, SkillAdvancementClass, SpellType,
};
use empyrean_entity::ObjectGuid;

use super::walk;
use crate::entity::spell::Spell;
use crate::world_objects::managers::enchantment_manager;
use crate::world_objects::{creature_combat, monster_combat, player_combat, player_magic};
use crate::World;

/// Metres within which a buffer notices a real player: across a town square.
const NOTICE: f64 = 60.0;
/// Metres within which it casts.
const CAST_RANGE: f32 = 10.0;
/// Seconds between looks for someone to buff.
const LOOK_EVERY: f64 = 5.0;
/// Seconds between casts.
const CAST_EVERY: f64 = 3.0;
/// Seconds before a real player is buffed again.
const AGAIN_AFTER: f64 = 600.0;
/// A buff with less than this many seconds left is cast again.
const RENEW_UNDER: f64 = 300.0;
/// The most buffs a buffer gives a player at once, as a real player would.
const ROUND: usize = 5;

/// A buffer at work.
#[derive(Debug, Clone)]
pub struct Buffing {
    pub target: ObjectGuid,
    /// The buffs still to cast on it.
    pub left: Vec<u32>,
    next_cast: f64,
    pub cast: u32,
}

/// AI Player `p`'s buffs: its best spell of each stacking category it can cast on someone else.
#[must_use]
pub fn buffs_of(w: &World, p: ObjectGuid) -> Vec<u32> {
    let Some(book) = w
        .objects
        .get(p)
        .and_then(|o| o.biota.properties_spell_book.as_ref())
    else {
        return Vec::new();
    };
    let mut best: std::collections::BTreeMap<u32, (u32, u32)> = std::collections::BTreeMap::new();
    for id in book.keys().filter_map(|k| u32::try_from(*k).ok()) {
        let spell = Spell::new(w, id, false);
        if !spell.is_beneficial()
            || spell.is_self_targeted()
            || spell.is_item_targeted_enchantment()
            || spell.is_fellowship_spell()
            || spell.meta_spell_type() != SpellType::Enchantment
            || !matches!(
                spell.school(),
                MagicSchool::CreatureEnchantment | MagicSchool::LifeMagic
            )
        {
            continue;
        }
        let category = spell.stacking_category().0;
        let power = spell.power();
        if best.get(&category).is_none_or(|(_, bp)| power > *bp) {
            best.insert(category, (id, power));
        }
    }
    best.values().map(|(id, _)| *id).collect()
}

/// Whether `target` lacks `spell`: nothing of its kind, something weaker, or little time left.
#[must_use]
pub fn lacks(w: &World, target: ObjectGuid, spell: u32) -> bool {
    let s = Spell::new(w, spell, false);
    let held = enchantment_manager::get_enchantments_category(w, target, s.stacking_category());
    !held.iter().any(|e| {
        // The registry counts an enchantment's start time down from 0 as it runs.
        let left = if e.duration < 0.0 {
            f64::INFINITY
        } else {
            e.duration + e.start_time
        };
        e.power_level >= s.power() && left > RENEW_UNDER
    })
}

/// Whether `target` wants `spell`: a skill's buff only for a skill it trains; any other buff
/// (attributes, vitals, protections, regeneration) always.
#[must_use]
pub fn wanted(w: &World, target: ObjectGuid, spell: u32) -> bool {
    // The stat a buff raises is in the spell's database row.
    let s = Spell::new(w, spell, true);
    if s.not_found() {
        return true;
    }
    if s.stat_mod_type().0 & EnchantmentTypeFlags::Skill.0 == 0 {
        return true;
    }
    let skill = Skill(i32::try_from(s.stat_mod_key()).unwrap_or(-1));
    w.objects
        .get(target)
        .and_then(|o| o.biota.properties_skill.as_ref())
        .and_then(|k| k.get(&skill))
        .is_some_and(|k| {
            matches!(
                k.sac,
                SkillAdvancementClass::Trained | SkillAdvancementClass::Specialized
            )
        })
}

/// How much `target` would want `spell`: a buff for a skill it specializes most, then a skill it
/// trains, then an attribute, then anything else.
fn worth(w: &World, target: ObjectGuid, spell: u32) -> f32 {
    let s = Spell::new(w, spell, true);
    if s.not_found() {
        return 1.0;
    }
    let flags = s.stat_mod_type().0;
    if flags & EnchantmentTypeFlags::Skill.0 != 0 {
        let skill = Skill(i32::try_from(s.stat_mod_key()).unwrap_or(-1));
        let sac = w
            .objects
            .get(target)
            .and_then(|o| o.biota.properties_skill.as_ref())
            .and_then(|k| k.get(&skill))
            .map(|k| k.sac);
        return if sac == Some(SkillAdvancementClass::Specialized) {
            4.0
        } else {
            3.0
        };
    }
    if flags & EnchantmentTypeFlags::Attribute.0 != 0 {
        return 2.0;
    }
    1.0
}

/// The buffs of `left` a buffer gives `target` this round: the five it wants most, a little at
/// random among equals.
fn round_of(w: &mut World, target: ObjectGuid, left: Vec<u32>) -> Vec<u32> {
    let mut scored: Vec<(f32, u32)> = left
        .into_iter()
        .map(|s| (worth(w, target, s) + w.ai_players.unit() * 0.9, s))
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.into_iter().take(ROUND).map(|(_, s)| s).collect()
}

/// A town AI Player's buffing: carry on with a player, or look for one. Answers whether it is
/// buffing (and so busy).
pub fn live(w: &mut World, id: u32, p: ObjectGuid, now: f64) -> bool {
    let Some(bot) = w.ai_players.bots.get(&id).cloned() else {
        return false;
    };
    if let Some(mut b) = bot.buffing {
        carry_on(w, id, p, &mut b, now);
        return true;
    }
    if now < bot.next_buff_look {
        return false;
    }
    if let Some(b) = w.ai_players.bots.get_mut(&id) {
        b.next_buff_look = now + LOOK_EVERY;
    }
    let buffs = buffs_of(w, p);
    if buffs.is_empty() {
        // No buffs to give: look again much later.
        if let Some(b) = w.ai_players.bots.get_mut(&id) {
            b.next_buff_look = now + 60.0;
        }
        return false;
    }
    let Some((target, left)) = someone(w, id, p, &buffs, now) else {
        return false;
    };
    let left = round_of(w, target, left);
    w.ai_players.buffed.insert(target.full(), now + AGAIN_AFTER);
    if let Some(b) = w.ai_players.bots.get_mut(&id) {
        b.sitting = false;
        b.walk = None;
        b.buffing = Some(Buffing {
            target,
            left,
            next_cast: now,
            cast: 0,
        });
    }
    true
}

/// The nearest real player within reach whom no buffer is buffing, not buffed lately, who lacks
/// some of `buffs`; with the ones they lack.
fn someone(
    w: &World,
    id: u32,
    p: ObjectGuid,
    buffs: &[u32],
    now: f64,
) -> Option<(ObjectGuid, Vec<u32>)> {
    let at = super::map_coords(&w.objects.get(p)?.location()?);
    let taken: Vec<ObjectGuid> = w
        .ai_players
        .bots
        .iter()
        .filter(|(other, _)| **other != id)
        .filter_map(|(_, b)| b.buffing.as_ref().map(|x| x.target))
        .collect();
    let mut best: Option<(ObjectGuid, Vec<u32>, f64)> = None;
    for g in super::real_players_online(w) {
        if taken.contains(&g)
            || w.ai_players
                .buffed
                .get(&g.full())
                .is_some_and(|until| *until > now)
        {
            continue;
        }
        let Some(o) = w.objects.get(g) else { continue };
        if monster_combat::is_dead(o) || o.wo.world_object.teleporting {
            continue;
        }
        let Some(pos) = o.location() else { continue };
        if pos.indoors() {
            continue;
        }
        let d = super::distance_m(at, super::map_coords(&pos));
        if d > NOTICE || best.as_ref().is_some_and(|(_, _, bd)| d >= *bd) {
            continue;
        }
        let left: Vec<u32> = buffs
            .iter()
            .copied()
            .filter(|s| wanted(w, g, *s) && lacks(w, g, *s))
            .collect();
        if !left.is_empty() {
            best = Some((g, left, d));
        }
    }
    best.map(|(g, left, _)| (g, left))
}

/// Next step with a player: close in, ready magic, cast the next buff, or finish.
fn carry_on(w: &mut World, id: u32, p: ObjectGuid, b: &mut Buffing, now: f64) {
    let target = b.target;
    let gone = w
        .objects
        .get(target)
        .is_none_or(|o| monster_combat::is_dead(o) || o.wo.world_object.teleporting);
    let far = !gone
        && crate::world_objects::monster_navigation::get_distance_to_object(w, p, target, true)
            > NOTICE * 1.5;
    if gone || far || b.left.is_empty() {
        finish(w, id, p);
        return;
    }
    #[allow(clippy::cast_possible_truncation)]
    let d =
        crate::world_objects::monster_navigation::get_distance_to_object(w, p, target, true) as f32;
    if d > CAST_RANGE {
        if let Some(at) = w.objects.get(target).and_then(|o| o.location()) {
            let g = walk::global(&at);
            walk::chase(w, id, p, [g[0], g[1]], CAST_RANGE * 0.7, now);
        }
        set(w, id, b.clone());
        return;
    }
    walk::halt_here(w, id, p);
    if creature_combat::combat_mode(w, p) != CombatMode::Magic {
        player_combat::handle_action_change_combat_mode(w, p, CombatMode::Magic, false, None);
        b.next_cast = now + 1.5;
        set(w, id, b.clone());
        return;
    }
    if now < b.next_cast || player_magic::is_busy(w, p) {
        set(w, id, b.clone());
        return;
    }
    // The next buff the player still lacks.
    while let Some(spell) = b.left.first().copied() {
        b.left.remove(0);
        if lacks(w, target, spell) {
            player_magic::handle_action_cast_targeted_spell(w, p, target.full(), spell, None);
            b.cast += 1;
            w.ai_players.buffs_cast += 1;
            b.next_cast = now + CAST_EVERY;
            break;
        }
    }
    set(w, id, b.clone());
}

fn set(w: &mut World, id: u32, b: Buffing) {
    if let Some(bot) = w.ai_players.bots.get_mut(&id) {
        bot.buffing = Some(b);
    }
}

/// Done with a player: back to peace.
fn finish(w: &mut World, id: u32, p: ObjectGuid) {
    if let Some(bot) = w.ai_players.bots.get_mut(&id) {
        bot.buffing = None;
    }
    walk::halt_here(w, id, p);
    if creature_combat::combat_mode(w, p) != CombatMode::NonCombat {
        player_combat::handle_action_change_combat_mode(w, p, CombatMode::NonCombat, false, None);
    }
}
