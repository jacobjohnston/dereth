//! `set-characterplus`: puts the `+` before a character's name, or takes it away. Not ACE.
//!
//! The `+` is the character's own `is_plussed` flag, which ACE sets only for a character made as
//! staff or, with `override_character_permissions`, from its account's level at each login. This
//! sets it on any character, online or not. An online character shows it from its next login.

use empyrean_entity::enums::{AccessLevel, ChatMessageType};
use empyrean_net::SessionId;
use empyrean_store::models::shard::Character;
use empyrean_world::entity::i_player::IPlayer;
use empyrean_world::managers::player_manager;
use empyrean_world::world_objects::player_database;
use empyrean_world::World;

use crate::command_handler_attribute::CommandHandlerAttribute;
use crate::command_handler_flag::CommandHandlerFlag;
use crate::command_handler_info::CommandHandlerInfo;
use crate::handlers::command_handler_helper::write_output_info;

/// The `set-characterplus` row (a new name, added after ACE's rows).
#[must_use]
pub fn command_handlers() -> Vec<CommandHandlerInfo> {
    let (handler, handler_name) = crate::handler!(handle_set_character_plus);
    vec![CommandHandlerInfo {
        handler,
        handler_name,
        attribute: CommandHandlerAttribute::with_count(
            "set-characterplus",
            AccessLevel::Admin,
            CommandHandlerFlag::None,
            1,
            "Puts the + before a character's name, or takes it away with off.",
            "charactername [on|off]",
        ),
    }]
}

/// The name and the wanted flag from the parameters: a last word `on` or `off` is the flag (on
/// when absent), and the rest is the name, without a leading `+`.
#[must_use]
pub fn parse(parameters: &[String]) -> (String, bool) {
    let (plussed, words) = match parameters.last().map(|p| p.to_ascii_lowercase()) {
        Some(last) if parameters.len() > 1 && (last == "on" || last == "off") => {
            (last == "on", &parameters[..parameters.len() - 1])
        }
        _ => (true, parameters),
    };
    let name = words.join(" ");
    (name.trim_start_matches('+').to_owned(), plussed)
}

fn done_message(name: &str, plussed: bool, online: bool) -> String {
    let what = if plussed {
        "now has the +"
    } else {
        "no longer has the +"
    };
    let when = if online { " from its next login" } else { "" };
    format!("{name} {what}{when}.")
}

/// `set-characterplus <name> [on|off]`.
pub fn handle_set_character_plus(w: &mut World, session: Option<SessionId>, parameters: &[String]) {
    let (name, plussed) = parse(parameters);
    let (found, _) = player_manager::find_by_name(w, &name);
    match found {
        Some(IPlayer::Online(guid)) => {
            let Some(p) = w.objects.get_mut(guid).and_then(|o| o.player.as_mut()) else {
                return;
            };
            let Some(character) = p.player.character.as_mut() else {
                return;
            };
            character.is_plussed = plussed;
            let name = character.name.clone();
            p.player_database.character_changes_detected = true;
            player_database::save_character_to_database(w, guid);
            write_output_info(
                w,
                session,
                &done_message(&name, plussed, true),
                ChatMessageType::Broadcast,
            );
        }
        Some(IPlayer::Offline(_)) => {
            let Some(stub) = w.shard.base_database().get_character_stub_by_name(&name) else {
                write_output_info(
                    w,
                    session,
                    &format!("There is no character named {name}."),
                    ChatMessageType::Broadcast,
                );
                return;
            };
            let id = stub.id;
            w.shard.get_characters(
                stub.account_id,
                false,
                Some(Box::new(
                    move |w: &mut World, characters: Vec<Character>| {
                        let Some(mut character) = characters.into_iter().find(|c| c.id == id)
                        else {
                            write_output_info(
                                w,
                                session,
                                &format!("There is no character named {name}."),
                                ChatMessageType::Broadcast,
                            );
                            return;
                        };
                        character.is_plussed = plussed;
                        let name = character.name.clone();
                        w.shard.save_character(
                            character,
                            Some(Box::new(move |w: &mut World, saved: bool| {
                                let message = if saved {
                                    done_message(&name, plussed, false)
                                } else {
                                    format!("Could not save {name}.")
                                };
                                write_output_info(w, session, &message, ChatMessageType::Broadcast);
                            })),
                        );
                    },
                )),
            );
        }
        None => {
            write_output_info(
                w,
                session,
                &format!("There is no character named {name}."),
                ChatMessageType::Broadcast,
            );
        }
    }
}
