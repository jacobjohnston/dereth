//! AI Players log in near a real player and out when switched off, and keep to their rules. Not
//! ACE.

#[cfg(feature = "real-content")]
mod real {
    use crate::support::real_content_bot::real::*;
    use empyrean_entity::enums::{MagicSchool, ShareType, SkillAdvancementClass, XpType};
    use empyrean_entity::Vector2;
    use empyrean_world::ai_players::{self, Stage};
    use empyrean_world::entity::position_extensions;
    use empyrean_world::managers::player_manager;
    use empyrean_world::world_objects::player_xp::grant_xp;
    use empyrean_world::world_objects::{
        player, player_character, player_death, player_fellowship, player_inventory,
        player_location, player_trade,
    };
    use std::time::Duration;

    const HOLTBURG: (f64, f64) = (42.28, 33.31);

    fn in_the_world(l: &Loop) -> Vec<ObjectGuid> {
        l.ts.world
            .ai_players
            .bots
            .iter()
            .filter(|(_, b)| b.stage == Stage::InWorld && b.placed)
            .map(|(id, _)| ObjectGuid::new(*id))
            .collect()
    }

    /// A real player standing outdoors in Holtburg, AI Players switched on, and the first of them
    /// in town.
    fn holtburg_with_a_bot() -> (Loop, ObjectGuid) {
        let mut l = create_character(&[44, 6, 21, 22, 24]);
        l.enter_world();
        #[allow(clippy::cast_possible_truncation)]
        let mut holtburg =
            Position::from_map_coordinates(Vector2::new(HOLTBURG.1 as f32, HOLTBURG.0 as f32));
        position_extensions::adjust_map_coords(&mut l.ts.world, &mut holtburg).expect("a spot");
        player_location::teleport(&mut l.ts.world, l.g, &holtburg, false);
        l.advance(0.5);
        l.action(&CharacterLoginCompleteNotification);
        l.advance(1.0);
        assert!(!l.location().indoors(), "outdoors in Holtburg");

        ai_players::set_enabled(&mut l.ts.world, true);
        assert!(
            l.ts.run_until(30.0, |ts| {
                ts.world
                    .ai_players
                    .bots
                    .values()
                    .any(|b| b.stage == Stage::InWorld && b.placed)
            }),
            "an AI Player logs in: {:?}",
            ai_players::describe(&l.ts.world)
        );
        l.advance(2.0);
        let bot = in_the_world(&l)[0];
        (l, bot)
    }

    fn told(l: &Loop, mark: usize, text: &str) -> bool {
        l.since::<CommunicationTextboxString>(mark)
            .iter()
            .any(|m| m.text.contains(text))
    }

    #[test]
    fn ai_players_log_in_near_a_real_player_and_out_when_switched_off() {
        let (mut l, bot) = holtburg_with_a_bot();
        let name = player::player_name(&l.ts.world, bot).expect("a name");
        assert!(name.starts_with(ai_players::MARKER), "{name}");
        let at = ai_players::map_coords(&l.location_of(bot));
        let metres =
            (dereth_primitives::num::math::hypot(at.0 - HOLTBURG.0, at.1 - HOLTBURG.1)) * 240.0;
        assert!(metres < 100.0, "in Holtburg: {at:?}");
        assert!(ai_players::is_ai_player(&l.ts.world, bot.full()));
        assert!(!ai_players::is_ai_player(&l.ts.world, l.g.full()));
        assert_eq!(ai_players::real_online_count(&l.ts.world), 1);
        assert!(player_manager::get_online_count(&l.ts.world) >= 2);
        assert!(
            player_manager::get_online_player_by_name(&l.ts.world, &name).is_some(),
            "found by its marked name"
        );

        // One at a time, at most one every 5 seconds.
        l.advance(20.0);
        let logged_in = l.ts.world.ai_players.bots.len();
        assert!((2..=6).contains(&logged_in), "{logged_in} in 22 seconds");

        ai_players::set_enabled(&mut l.ts.world, false);
        assert!(
            l.ts.run_until(30.0, |ts| ts.world.ai_players.bots.is_empty()),
            "every AI Player logs out: {:?}",
            ai_players::describe(&l.ts.world)
        );
        assert_eq!(player_manager::get_online_count(&l.ts.world), 1);
    }

    #[test]
    fn ai_players_know_their_spells_keep_their_mana_and_never_change() {
        let (mut l, _) = holtburg_with_a_bot();
        l.advance(15.0);
        for bot in in_the_world(&l) {
            let w = &mut l.ts.world;
            // Spells: the trained schools, up to the level each casts reliably.
            let mut castable = 0;
            for school in [
                MagicSchool::CreatureEnchantment,
                MagicSchool::ItemEnchantment,
                MagicSchool::LifeMagic,
                MagicSchool::WarMagic,
                MagicSchool::VoidMagic,
            ] {
                let skill = w
                    .objects
                    .get_mut(bot)
                    .and_then(|o| o.get_creature_skill_school(school))
                    .expect("a skill");
                let o = w.objects.get(bot).expect("in the world");
                if matches!(
                    skill.advancement_class(o),
                    SkillAdvancementClass::Trained | SkillAdvancementClass::Specialized
                ) {
                    castable += ai_players::reliable_level(skill.current(w, bot));
                }
            }
            let known = w
                .objects
                .get(bot)
                .and_then(|o| o.biota.properties_spell_book.as_ref())
                .map_or(0, |b| b.len());
            assert_eq!(castable > 0, known > 0, "{castable} levels, {known} spells");
            println!("AI Player {bot:?}: {castable} spell levels, {known} spells");

            // Mana: full.
            let mana = w.objects.get(bot).expect("in the world").mana();
            let max = mana.max_value(&mut StatCtx::in_world(w, bot));
            let o = w.objects.get(bot).expect("in the world");
            assert_eq!(mana.current(o), max, "full mana");

            // No experience, and no vitae.
            let xp = o.total_experience();
            grant_xp(w, bot, 1_000_000, XpType::Kill, ShareType::All);
            let o = w.objects.get(bot).expect("in the world");
            assert_eq!(o.total_experience(), xp, "no experience");
            player_death::inflict_vitae_penalty(w, bot, 5);
            let o = w.objects.get(bot).expect("in the world");
            assert!(o.death_level().is_none(), "no vitae");
        }
    }

    #[test]
    fn ai_players_turn_away_trades_gifts_friends_and_fellowships() {
        let (mut l, bot) = holtburg_with_a_bot();
        let me = l.g;
        let name = player::player_name(&l.ts.world, bot).expect("a name");
        let bare = name.trim_start_matches(ai_players::MARKER).to_owned();

        let mark = l.mark();
        player_trade::handle_action_open_trade_negotiations(&mut l.ts.world, me, bot.full(), true);
        l.advance(0.5);
        assert!(told(&l, mark, "is an AI Player and can't trade"));

        let mark = l.mark();
        player_inventory::handle_action_give_object_request(&mut l.ts.world, me, bot.full(), 1, 1);
        l.advance(0.5);
        assert!(told(&l, mark, "can't take items"));

        let mark = l.mark();
        player_character::handle_action_add_friend(&mut l.ts.world, me, &bare);
        l.advance(0.5);
        assert!(told(&l, mark, "can't be added as a friend"));

        let mark = l.mark();
        player_fellowship::fellowship_recruit(&mut l.ts.world, me, Some(bot));
        l.advance(0.5);
        assert!(told(&l, mark, "can't join a fellowship"));

        let mark = l.mark();
        l.action(&dereth_protocol::comms::CommunicationTalkDirectByName {
            message: "hello".into(),
            target_name: bare,
        });
        l.advance(0.5);
        assert!(told(&l, mark, "is an AI Player and can't reply"));
    }

    #[test]
    fn ai_players_walk_about_town_as_a_client_moves_and_the_player_sees_them() {
        let (mut l, _) = holtburg_with_a_bot();
        let mark = l.mark();
        let bots = in_the_world(&l);
        let start: Vec<Position> = bots.iter().map(|b| l.location_of(*b)).collect();
        let mut farthest = vec![0.0f64; bots.len()];
        for _ in 0..60 {
            ai_players::planner::Planner::wait(&mut l.ts.world, Duration::from_secs(60));
            l.advance(1.0);
            for (i, b) in bots.iter().enumerate() {
                let Some(at) = l.ts.world.objects.get(*b).and_then(WorldObject::location) else {
                    continue;
                };
                let (a, s) = (
                    ai_players::map_coords(&at),
                    ai_players::map_coords(&start[i]),
                );
                farthest[i] = farthest[i]
                    .max(dereth_primitives::num::math::hypot(a.0 - s.0, a.1 - s.1) * 240.0);
                let from_centre =
                    dereth_primitives::num::math::hypot(a.0 - HOLTBURG.0, a.1 - HOLTBURG.1) * 240.0;
                assert!(from_centre < 80.0, "stays in town: {from_centre} m");
                assert!(!at.indoors(), "stays outdoors");
            }
        }
        println!("AI Players walked: {farthest:?} m");
        assert!(
            farthest.iter().any(|d| *d > 5.0),
            "someone walked: {farthest:?}"
        );
        let seen: Vec<MovementPositionEvent> = l.since(mark);
        assert!(
            seen.iter().any(|m| bots.iter().any(|b| m.id.0 == b.full())),
            "the player saw an AI Player move"
        );
    }

    #[test]
    fn town_levels_come_from_the_land_round_each_town() {
        let mut l = create_character(&[44, 6, 21, 22, 24]);
        let w = &mut l.ts.world;
        let mut by_name = std::collections::BTreeMap::new();
        for (i, t) in ai_players::towns::TOWNS.iter().enumerate() {
            let level = ai_players::levels::town_level(w, i);
            println!("{:>22}: {level:>3} (first guess {})", t.name, t.level);
            by_name.insert(t.name, level);
        }
        assert!(
            by_name["Holtburg"] <= 15,
            "a starting town is for new characters"
        );
        assert!(by_name["Yaraq"] <= 15 && by_name["Shoushi"] <= 15);
        for far in ["Fort Tethana", "Wai Jhou", "Ayan Baqur", "Kara"] {
            assert!(by_name[far] >= 60, "{far} is for seasoned characters");
        }
        let holtburg = ai_players::levels::block_at(42.28, 33.31);
        assert!(
            !ai_players::levels::around(w, holtburg, 1).is_empty(),
            "creatures round Holtburg"
        );
    }
}
