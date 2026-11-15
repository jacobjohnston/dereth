//! `aiplayers`: switches the AI Players on and off and says what they are doing, and the builder
//! that makes a new AI Player as a playtest character is made. Not ACE.

use std::collections::BTreeMap;

use dereth_progression::archetypes::{builtin, Archetype};
use dereth_progression::order::{Gear, Order, ORDER_VERSION};
use dereth_progression::Extras;
use empyrean_entity::enums::{AccessLevel, ChatMessageType, PropertyBool};
use empyrean_net::SessionId;
use empyrean_world::ai_players::{self, BotRequest};
use empyrean_world::World;

use super::playtest_character::{create, with_tables};
use crate::command_handler_attribute::CommandHandlerAttribute;
use crate::command_handler_flag::CommandHandlerFlag;
use crate::command_handler_info::CommandHandlerInfo;
use crate::handlers::command_handler_helper::write_output_info;

/// The `aiplayers` row (a new name, added after ACE's rows).
#[must_use]
pub fn command_handlers() -> Vec<CommandHandlerInfo> {
    let (handler, handler_name) = crate::handler!(handle_ai_players);
    vec![CommandHandlerInfo {
        handler,
        handler_name,
        attribute: CommandHandlerAttribute::with_count(
            "aiplayers",
            AccessLevel::Admin,
            CommandHandlerFlag::None,
            0,
            "Switches the AI Players on or off, or says what they are doing.",
            "[on|off|status|list]",
        ),
    }]
}

pub fn handle_ai_players(w: &mut World, session: Option<SessionId>, parameters: &[String]) {
    let word = parameters
        .first()
        .map_or("status".to_owned(), |p| p.to_ascii_lowercase());
    let say = |w: &mut World, text: &str| {
        write_output_info(w, session, text, ChatMessageType::Broadcast);
    };
    match word.as_str() {
        "on" => {
            ai_players::set_enabled(w, true);
            say(w, "AI Players are on. They log in near real players, one every 5 seconds.");
        }
        "off" => {
            ai_players::set_enabled(w, false);
            say(w, "AI Players are off. Every AI Player is logging out.");
        }
        "list" => {
            let lines = ai_players::describe(w);
            if lines.is_empty() {
                say(w, "No AI Players are in the world.");
            }
            for line in lines {
                say(w, &line);
            }
        }
        "status" => {
            let on = if ai_players::enabled(w) { "on" } else { "off" };
            let n = w.ai_players.bots.len();
            say(w, &format!("AI Players are {on}, with {n} in the world."));
        }
        _ => say(w, "Use: aiplayers on, off, status or list."),
    }
}

/// The archetypes a world offers, in a stable order.
fn offered(w: &World) -> Vec<Archetype> {
    with_tables(w, |t| builtin().into_iter().filter(|a| a.offered(&t)).collect())
}

/// Makes a new AI Player: the playtest character an order of its level makes, as one of the
/// world's archetypes whose skills its era has, wearing gear of its level.
///
/// # Errors
/// When no archetype can be made.
pub fn build(w: &mut World, account: &str, request: &BotRequest) -> Result<u32, String> {
    let archetypes = offered(w);
    if archetypes.is_empty() {
        return Err("the world offers no archetype".into());
    }
    let start = usize::try_from(request.seed % archetypes.len() as u64).unwrap_or(0);
    let mut last = String::new();
    for i in 0..archetypes.len() {
        let archetype = &archetypes[(start + i) % archetypes.len()];
        let order = Order {
            version: ORDER_VERSION,
            hash: format!("ai-player {}", archetype.id),
            level: request.level,
            plan: archetype.plan.clone(),
            heritage: Some(request.heritage),
            gender: Some(request.gender),
            gear: Gear::Level,
            seed: request.seed,
            items: Vec::new(),
            place_tier: None,
            slots: BTreeMap::new(),
            extras: Extras::default(),
        };
        match create::create_from_order(w, account, &request.name, &order) {
            Ok(made) => {
                supplied(w, made.id);
                return Ok(made.id);
            }
            // Refused before anything is saved (an era without its skills): the next one.
            Err(e) => last = e,
        }
        if empyrean_world::managers::player_manager::find_by_name(w, &request.name)
            .0
            .is_some()
        {
            // Saved, then refused: keep it rather than make a second of the same name.
            break;
        }
    }
    Err(last)
}

/// An AI Player never runs out of spell components.
fn supplied(w: &mut World, id: u32) {
    if let Some(p) = w.player_manager.offline_players.get_mut(&id) {
        p.set_property(PropertyBool::SpellComponentsRequired, false);
        p.set_property(PropertyBool::SafeSpellComponents, true);
        p.changes_detected = true;
    }
}

/// Sets the builder the world makes new AI Players with.
pub fn install() {
    ai_players::set_builder(build);
}
