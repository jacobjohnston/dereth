//! AI Players: server-controlled characters that play like players. Not ACE; see `AI_PLAYERS.md`.
//!
//! Each AI Player is a character on the `aiplayers` account, in the world as a Player with a stub
//! session: a [`SessionData`] whose id the transport does not know, so what is sent to it is
//! dropped. [`tick`] runs on the world thread: it finishes logins and teleports (no client answers
//! for an AI Player), and every few seconds the director logs AI Players in near real players and
//! out again when none is near.

pub mod names;
pub mod towns;

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use empyrean_entity::enums::{AccessLevel, MagicSchool, SkillAdvancementClass};
use empyrean_entity::{ObjectGuid, Position};
use empyrean_net::{SessionId, SessionState};

use crate::entity::position_extensions;
use crate::managers::{player_manager, world_manager};
use crate::sessions::{self, SessionData};
use crate::world_objects::{player_location, player_networking};
use crate::World;

/// What an AI Player's name starts with, as an admin's starts with `+`.
pub const MARKER: char = '~';
/// The account every AI Player is a character of.
pub const ACCOUNT: &str = "aiplayers";
/// The server property that switches AI Players on.
pub const PROPERTY: &str = "ai_players_enabled";

/// At most one login every this many seconds.
const LOGIN_EVERY: f64 = 5.0;
/// How often the director looks again.
const DIRECTOR_EVERY: f64 = 2.0;
/// AI Players log in within this many metres of a real player.
const SPAWN_RADIUS: f64 = 5000.0;
/// And stay while one is within this many.
const KEEP_RADIUS: f64 = 6000.0;
/// Seconds an AI Player with no real player near waits before it logs out.
const LEAVE_AFTER: f64 = 60.0;
/// Seconds a login may take before it is given up.
const LOGIN_TIMEOUT: f64 = 60.0;
/// Seconds an AI Player stays in portal space, as a client would.
const PORTAL_SPACE: f64 = 1.0;
/// The most AI Players in the world at once.
const MAX_TOTAL: usize = 60;
/// Metres from a town's centre an AI Player may arrive at.
const TOWN_SPREAD: f32 = 25.0;
/// Metres in one map unit.
const METRES_PER_UNIT: f64 = 240.0;

/// A new AI Player to make: its name, heritage, gender and level, and the seed its build and gear
/// are rolled with.
#[derive(Debug, Clone)]
pub struct BotRequest {
    pub name: String,
    pub heritage: u32,
    pub gender: u32,
    pub level: u32,
    pub seed: u64,
}

/// Makes a new AI Player's character on `account`; answers its id. The command crate makes it,
/// with the playtest characters' builds and gear.
pub type BuildFn = fn(&mut World, account: &str, request: &BotRequest) -> Result<u32, String>;

static BUILDER: OnceLock<BuildFn> = OnceLock::new();

/// Sets how new AI Players are made (once, at startup).
pub fn set_builder(build: BuildFn) {
    let _ = BUILDER.set(build);
}

/// Where an AI Player is in its life.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Stage {
    /// Its character is loading.
    Entering { since: f64 },
    /// In portal space, on its way to `since + PORTAL_SPACE`.
    Teleporting { since: f64 },
    InWorld,
    LoggingOut,
}

/// One AI Player in the world.
#[derive(Debug, Clone)]
pub struct Bot {
    pub session: SessionId,
    pub stage: Stage,
    /// The town it was sent to, by [`towns::TOWNS`] index.
    pub town: usize,
    /// When no real player was last near, if none is now.
    pub alone_since: Option<f64>,
    /// Whether it has been sent to its town since it logged in.
    pub placed: bool,
}

/// The world's AI Players.
#[derive(Debug, Default)]
pub struct AiPlayersState {
    account_id: Option<u32>,
    /// By character id.
    pub bots: BTreeMap<u32, Bot>,
    next_login: f64,
    next_director: f64,
    next_stub: u16,
    /// Where each real player last stood outdoors, by character id.
    last_outdoor: HashMap<u32, (f64, f64)>,
    rng: u64,
}

impl AiPlayersState {
    fn next_random(&mut self) -> u64 {
        if self.rng == 0 {
            self.rng = 0x9E37_79B9_7F4A_7C15;
        }
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next_random() % (n.max(1) as u64)).unwrap_or(0)
    }

    fn unit(&mut self) -> f32 {
        #[allow(clippy::cast_precision_loss)]
        let v = (self.next_random() >> 40) as f32 / (1u64 << 24) as f32;
        v
    }
}

/// Whether AI Players are switched on.
#[must_use]
pub fn enabled(w: &World) -> bool {
    player_manager::property_manager_get_bool(w, PROPERTY)
}

/// Switches AI Players on or off: off logs every one out, on lets the director log them in.
pub fn set_enabled(w: &mut World, on: bool) {
    crate::managers::property_manager::modify_bool(w, PROPERTY, on);
    w.ai_players.next_director = 0.0;
}

/// Whether character `guid` is an AI Player.
#[must_use]
pub fn is_ai_player(w: &World, guid: u32) -> bool {
    let Some(account) = w.ai_players.account_id else {
        return false;
    };
    let pm = &w.player_manager;
    pm.online_players
        .get(&guid)
        .and_then(|p| p.account.as_ref())
        .or_else(|| {
            pm.offline_players
                .get(&guid)
                .and_then(|p| p.account.as_ref())
        })
        .is_some_and(|a| a.account_id == account)
}

/// When `bot` is an AI Player, tells `asker` it can't do `what` and answers true.
pub fn turn_away(w: &mut World, asker: ObjectGuid, bot: u32, what: &str) -> bool {
    if !is_ai_player(w, bot) {
        return false;
    }
    let name = crate::dispatch::name::name(w, ObjectGuid::new(bot)).unwrap_or_default();
    let session = crate::world_objects::world_object_networking::shims::player_session(w, asker);
    crate::network::chat_packet::send_server_message(
        w,
        session,
        &format!("{name} is an AI Player and can't {what}."),
        empyrean_entity::enums::ChatMessageType::Broadcast,
    );
    true
}

/// Whether `session` is an AI Player's, still loading, which the session sweep must keep.
#[must_use]
pub fn holds(w: &World, session: SessionId) -> bool {
    w.ai_players
        .bots
        .values()
        .any(|b| b.session == session && matches!(b.stage, Stage::Entering { .. }))
}

/// Every online player who is not an AI Player.
#[must_use]
pub fn real_players_online(w: &World) -> Vec<ObjectGuid> {
    player_manager::get_all_online(w)
        .into_iter()
        .filter(|g| !is_ai_player(w, g.full()))
        .collect()
}

/// How many players are online, not counting AI Players.
#[must_use]
pub fn real_online_count(w: &World) -> i32 {
    i32::try_from(real_players_online(w).len()).unwrap_or(i32::MAX)
}

fn now(w: &World) -> f64 {
    w.now.monotonic.as_secs_f64()
}

/// The AI Players' turn on the world thread, each tick.
pub fn tick(w: &mut World) {
    let now = now(w);
    if w.ai_players.account_id.is_none() && !ensure_account(w) {
        return;
    }
    advance(w, now);
    if now < w.ai_players.next_director {
        return;
    }
    w.ai_players.next_director = now + DIRECTOR_EVERY;
    direct(w, now);
}

/// Finds or makes the AI Players' account.
fn ensure_account(w: &mut World) -> bool {
    let mut auth = w.auth.lock();
    let account = auth.get_account_by_name(ACCOUNT).or_else(|| {
        let password = format!("{:032x}", w.ai_players.next_random() as u128 * 31 + 7);
        auth.create_account(
            ACCOUNT,
            &password,
            AccessLevel::Player,
            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        )
        .ok()
    });
    drop(auth);
    match account {
        Some(a) => {
            w.ai_players.account_id = Some(a.account_id);
            true
        }
        None => {
            log::warn!("AI Players: the {ACCOUNT} account could not be made");
            false
        }
    }
}

/// Finishes what no client answers for: the login, and each trip through portal space.
fn advance(w: &mut World, now: f64) {
    let ids: Vec<u32> = w.ai_players.bots.keys().copied().collect();
    for id in ids {
        let Some(bot) = w.ai_players.bots.get(&id).cloned() else {
            continue;
        };
        if w.sessions.get(bot.session).is_none() {
            w.ai_players.bots.remove(&id);
            continue;
        }
        let player = w
            .sessions
            .player(bot.session)
            .filter(|p| w.objects.get(*p).is_some());
        match bot.stage {
            Stage::Entering { since } => {
                if let Some(p) = player {
                    login_complete(w, p);
                    learn_spells(w, p);
                    resupply(w, p);
                    set_stage(w, id, Stage::InWorld);
                } else if now - since > LOGIN_TIMEOUT {
                    log::warn!("AI Players: character {id:#x} did not load; given up");
                    w.sessions.remove(bot.session);
                    w.ai_players.bots.remove(&id);
                }
            }
            Stage::Teleporting { since } => {
                if let Some(p) = player {
                    if now - since >= PORTAL_SPACE {
                        if teleporting(w, p) {
                            player_location::on_teleport_complete(w, p);
                        }
                        set_stage(w, id, Stage::InWorld);
                    }
                }
            }
            Stage::InWorld => {
                let Some(p) = player else { continue };
                if teleporting(w, p) {
                    // Died, or sent somewhere: it comes out the other side as a client would.
                    set_stage(w, id, Stage::Teleporting { since: now });
                } else if !bot.placed {
                    send_to_town(w, id, p, bot.town);
                    set_stage(w, id, Stage::Teleporting { since: now });
                }
            }
            Stage::LoggingOut => {}
        }
    }
}

fn set_stage(w: &mut World, id: u32, stage: Stage) {
    if let Some(b) = w.ai_players.bots.get_mut(&id) {
        b.stage = stage;
    }
}

fn teleporting(w: &World, p: ObjectGuid) -> bool {
    w.objects
        .get(p)
        .is_some_and(|o| o.wo.world_object.teleporting)
}

/// What `GameActionLoginComplete` does for a client that has left portal space.
fn login_complete(w: &mut World, p: ObjectGuid) {
    player_location::on_teleport_complete(w, p);
    let first = w
        .objects
        .get(p)
        .is_some_and(|o| !o.first_enter_world_done());
    if first {
        if let Some(o) = w.objects.get_mut(p) {
            o.set_first_enter_world_done(true);
        }
        player_networking::send_property_updates_and_overrides(w, p);
    }
}

/// The magic schools, and how sure of a cast an AI Player must be to learn a level's spells.
const SCHOOLS: [MagicSchool; 5] = [
    MagicSchool::CreatureEnchantment,
    MagicSchool::ItemEnchantment,
    MagicSchool::LifeMagic,
    MagicSchool::WarMagic,
    MagicSchool::VoidMagic,
];
const RELIABLE: f64 = 0.9;

/// Every AI Player in the world, ready.
fn bots_in_world(w: &World) -> Vec<ObjectGuid> {
    w.ai_players
        .bots
        .values()
        .filter(|b| b.stage == Stage::InWorld)
        .filter_map(|b| w.sessions.player(b.session))
        .filter(|p| w.objects.get(*p).is_some())
        .collect()
}

/// The highest spell level an AI Player of `skill` casts reliably: one whose hardest spells it
/// casts at least nine times in ten.
#[must_use]
pub fn reliable_level(skill: u32) -> u32 {
    use crate::entity::spell_formula::{min_power, MAX_SPELL_LEVEL};
    (1..=MAX_SPELL_LEVEL)
        .rev()
        .find(|&level| {
            let hardest = if level < MAX_SPELL_LEVEL {
                min_power(level + 1) - 1
            } else {
                min_power(level) + 50
            };
            let skill = i32::try_from(skill).unwrap_or(i32::MAX);
            let hardest = i32::try_from(hardest).unwrap_or(i32::MAX);
            skill >= hardest - 50
                && crate::world_objects::skill_check::get_magic_skill_chance(skill, hardest)
                    >= RELIABLE
        })
        .unwrap_or(0)
}

/// An AI Player knows every spell of the schools it trains, up to the level it casts reliably.
fn learn_spells(w: &mut World, p: ObjectGuid) {
    for school in SCHOOLS {
        let Some(skill) = w
            .objects
            .get_mut(p)
            .and_then(|o| o.get_creature_skill_school(school))
        else {
            continue;
        };
        let trained = w.objects.get(p).is_some_and(|o| {
            matches!(
                skill.advancement_class(o),
                SkillAdvancementClass::Trained | SkillAdvancementClass::Specialized
            )
        });
        if !trained {
            continue;
        }
        let top = reliable_level(skill.current(w, p));
        for level in 1..=top {
            crate::world_objects::player_spells::learn_spells_in_bulk(w, p, school, level, false);
        }
    }
}

/// An AI Player never runs short of mana.
fn resupply(w: &mut World, p: ObjectGuid) {
    let Some(o) = w.objects.get(p) else { return };
    if crate::world_objects::monster_combat::is_dead(o) {
        return;
    }
    let mana = o.mana();
    crate::world_objects::creature_vitals::update_vital_delta(w, p, mana, 1_000_000);
}

/// Sends AI Player `id` through a portal to a spot near the centre of town `town`.
fn send_to_town(w: &mut World, id: u32, p: ObjectGuid, town: usize) {
    if let Some(b) = w.ai_players.bots.get_mut(&id) {
        b.placed = true;
    }
    let t = towns::TOWNS[town];
    let (dx, dy) = {
        let s = &mut w.ai_players;
        (
            (s.unit() * 2.0 - 1.0) * TOWN_SPREAD,
            (s.unit() * 2.0 - 1.0) * TOWN_SPREAD,
        )
    };
    #[allow(clippy::cast_possible_truncation)]
    let spot = empyrean_entity::Vector2::new(
        t.ew as f32 + dx / METRES_PER_UNIT as f32,
        t.ns as f32 + dy / METRES_PER_UNIT as f32,
    );
    let mut pos = Position::from_map_coordinates(spot);
    if let Err(e) = position_extensions::adjust_map_coords(w, &mut pos) {
        log::warn!("AI Players: no spot in {}: {e}", t.name);
        return;
    }
    player_location::teleport(w, p, &pos, false);
}

/// `pos` as map coordinates: north, then east.
#[must_use]
pub fn map_coords(pos: &Position) -> (f64, f64) {
    let raw = pos.landblock_id().raw();
    let p = pos.pos();
    let at = |block: u32, v: f32| (f64::from(block) * 192.0 + f64::from(v)) / METRES_PER_UNIT - 101.95;
    (at((raw >> 16) & 0xFF, p.y), at(raw >> 24, p.x))
}

fn distance_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt() * METRES_PER_UNIT
}

/// Where each real player counts from: where they stand, or where they last stood outdoors.
fn anchors(w: &mut World) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for g in real_players_online(w) {
        let Some(pos) = w.objects.get(g).and_then(|o| o.location()) else {
            continue;
        };
        if pos.indoors() {
            if let Some(at) = w.ai_players.last_outdoor.get(&g.full()) {
                out.push(*at);
            }
        } else {
            let at = map_coords(&pos);
            w.ai_players.last_outdoor.insert(g.full(), at);
            out.push(at);
        }
    }
    out
}

/// The director: log AI Players out where no real player is near, and one in where a town near a
/// real player is short of them.
fn direct(w: &mut World, now: f64) {
    if !enabled(w) || w.server_manager.shutdown_in_progress {
        let leaving: Vec<u32> = w
            .ai_players
            .bots
            .iter()
            .filter(|(_, b)| b.stage != Stage::LoggingOut)
            .map(|(id, _)| *id)
            .collect();
        for id in leaving {
            log_out(w, id);
        }
        return;
    }

    for p in bots_in_world(w) {
        resupply(w, p);
    }

    let anchors = anchors(w);
    let near = |t: &towns::Town, r: f64| anchors.iter().any(|a| distance_m(*a, (t.ns, t.ew)) <= r);

    // Out: AI Players whose town has had no real player near for a while.
    let ids: Vec<u32> = w.ai_players.bots.keys().copied().collect();
    for id in ids {
        let Some(bot) = w.ai_players.bots.get(&id).cloned() else {
            continue;
        };
        if bot.stage == Stage::LoggingOut {
            continue;
        }
        if near(&towns::TOWNS[bot.town], KEEP_RADIUS) {
            if let Some(b) = w.ai_players.bots.get_mut(&id) {
                b.alone_since = None;
            }
        } else {
            let since = bot.alone_since.unwrap_or(now);
            if let Some(b) = w.ai_players.bots.get_mut(&id) {
                b.alone_since = Some(since);
            }
            if now - since >= LEAVE_AFTER {
                log_out(w, id);
            }
        }
    }

    // In: one at a time, at most one every few seconds.
    let loading = w
        .ai_players
        .bots
        .values()
        .any(|b| matches!(b.stage, Stage::Entering { .. }));
    let active = w
        .ai_players
        .bots
        .values()
        .filter(|b| b.stage != Stage::LoggingOut)
        .count();
    if loading || now < w.ai_players.next_login || active >= MAX_TOTAL {
        return;
    }
    let mut short: Vec<(usize, usize)> = towns::of_era(w.era.id)
        .filter(|(_, t)| near(t, SPAWN_RADIUS))
        .map(|(i, t)| {
            let have = w
                .ai_players
                .bots
                .values()
                .filter(|b| b.town == i && b.stage != Stage::LoggingOut)
                .count();
            (i, t.size.capacity().saturating_sub(have))
        })
        .filter(|(_, short)| *short > 0)
        .collect();
    short.sort_by_key(|s| std::cmp::Reverse(s.1));
    if let Some(&(town, _)) = short.first() {
        w.ai_players.next_login = now + LOGIN_EVERY;
        if let Err(e) = log_in(w, town, now) {
            log::warn!("AI Players: no login for {}: {e}", towns::TOWNS[town].name);
        }
    }
}

/// Logs AI Player `id` out, as a player who asks to.
pub fn log_out(w: &mut World, id: u32) {
    let Some(bot) = w.ai_players.bots.get_mut(&id) else {
        return;
    };
    bot.stage = Stage::LoggingOut;
    let session = bot.session;
    if w.sessions.player(session).is_some() {
        sessions::log_off_player(w, session, true);
    } else {
        w.sessions.remove(session);
        w.ai_players.bots.remove(&id);
    }
}

/// The level range town `town` wants: its level to 20 above it, within the world's.
fn levels_for(w: &World, town: usize) -> (u32, u32) {
    let max = crate::world_objects::player_xp::get_max_level(w);
    let low = towns::TOWNS[town].level.clamp(1, max);
    (low, (low + 20).min(max))
}

/// Logs in an AI Player for town `town`: one of the pool of its level when there is one, a new
/// one otherwise.
fn log_in(w: &mut World, town: usize, now: f64) -> Result<(), String> {
    let account_id = w.ai_players.account_id.ok_or("no account")?;
    let (low, high) = levels_for(w, town);
    let pool: Vec<u32> = w
        .player_manager
        .offline_players
        .values()
        .filter(|p| p.account.as_ref().is_some_and(|a| a.account_id == account_id))
        .filter(|p| {
            p.level()
                .and_then(|l| u32::try_from(l).ok())
                .is_some_and(|l| (low..=high).contains(&l))
        })
        .map(|p| p.guid.full())
        .filter(|id| !w.ai_players.bots.contains_key(id))
        .collect();
    let id = if pool.is_empty() {
        build(w, low, high)?
    } else {
        let i = w.ai_players.below(pool.len());
        pool[i]
    };
    let character = w
        .shard
        .base_database()
        .get_character_stub_by_guid(id)
        .ok_or("the character has no record")?;

    w.ai_players.next_stub = w.ai_players.next_stub.wrapping_add(1);
    let session = SessionId {
        client_id: u16::MAX - (w.ai_players.next_stub % 4096),
        generation: u32::from(w.ai_players.next_stub) + 1,
    };
    let mut data = SessionData {
        account_id,
        account: Some(ACCOUNT.to_owned()),
        access_level: AccessLevel::Player,
        characters: vec![character.clone()],
        ..SessionData::default()
    };
    data.init_session_for_world_login();
    w.sessions.insert(session, data);
    sessions::set_state(w, session, SessionState::WorldConnected);
    w.ai_players.bots.insert(
        id,
        Bot {
            session,
            stage: Stage::Entering { since: now },
            town,
            alone_since: None,
            placed: false,
        },
    );
    world_manager::player_enter_world(w, session, character);
    Ok(())
}

/// Makes a new AI Player of a level from `low` to `high`, with a free name.
fn build(w: &mut World, low: u32, high: u32) -> Result<u32, String> {
    let builder = *BUILDER.get().ok_or("no builder is set")?;
    let all = names::all();
    for _ in 0..50 {
        let n = all[w.ai_players.below(all.len())];
        let taken = player_manager::find_by_name(w, n.name).0.is_some()
            || !w
                .shard
                .base_database()
                .is_character_name_available(n.name);
        if taken {
            continue;
        }
        let span = usize::try_from(high - low + 1).unwrap_or(1);
        let level = low + u32::try_from(w.ai_players.below(span)).unwrap_or(0);
        let request = BotRequest {
            name: n.name.to_owned(),
            heritage: n.heritage,
            gender: n.gender,
            level,
            seed: w.ai_players.next_random(),
        };
        return builder(w, ACCOUNT, &request);
    }
    Err("no free name".into())
}

/// One line per AI Player: name, level, town and stage.
#[must_use]
pub fn describe(w: &World) -> Vec<String> {
    w.ai_players
        .bots
        .iter()
        .map(|(id, b)| {
            let o = w.player_manager.offline_players.get(id);
            let name = w
                .sessions
                .player(b.session)
                .and_then(|p| crate::world_objects::world_object::world_object_name(w, p))
                .or_else(|| o.and_then(|o| o.name()))
                .unwrap_or_else(|| format!("{id:#x}"));
            let stage = match b.stage {
                Stage::Entering { .. } => "logging in",
                Stage::Teleporting { .. } => "in portal space",
                Stage::InWorld => "in the world",
                Stage::LoggingOut => "logging out",
            };
            format!("{MARKER}{name}: {}, {stage}", towns::TOWNS[b.town].name)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spell_level_is_learned_when_its_hardest_spells_succeed_nine_times_in_ten() {
        assert_eq!(reliable_level(0), 0);
        assert_eq!(reliable_level(70), 0);
        assert_eq!(reliable_level(90), 1);
        assert_eq!(reliable_level(300), 5);
        assert_eq!(reliable_level(440), 7);
        assert_eq!(reliable_level(500), 8);
    }
}
