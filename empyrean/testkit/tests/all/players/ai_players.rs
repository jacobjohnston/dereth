//! AI Players log in near a real player and out when switched off. Not ACE.

#[cfg(feature = "real-content")]
mod real {
    use crate::support::real_content_bot::real::*;
    use empyrean_entity::Vector2;
    use empyrean_world::ai_players::{self, Stage};
    use empyrean_world::entity::position_extensions;
    use empyrean_world::managers::player_manager;
    use empyrean_world::world_objects::{player, player_location};

    const HOLTBURG: (f64, f64) = (42.28, 33.31);

    fn in_the_world(l: &Loop) -> Vec<u32> {
        l.ts.world
            .ai_players
            .bots
            .iter()
            .filter(|(_, b)| b.stage == Stage::InWorld && b.placed)
            .map(|(id, _)| *id)
            .collect()
    }

    #[test]
    fn ai_players_log_in_near_a_real_player_and_out_when_switched_off() {
        let mut l = create_character(&[44, 6, 21, 22, 24]);
        l.enter_world();

        // The real player stands outdoors in Holtburg.
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
        let first = in_the_world(&l)[0];
        let bot = ObjectGuid::new(first);
        let name = player::player_name(&l.ts.world, bot).expect("a name");
        assert!(name.starts_with(ai_players::MARKER), "{name}");
        let at = ai_players::map_coords(&l.location_of(bot));
        let metres = ((at.0 - HOLTBURG.0).hypot(at.1 - HOLTBURG.1)) * 240.0;
        assert!(metres < 100.0, "in Holtburg: {at:?}");
        assert!(ai_players::is_ai_player(&l.ts.world, first));
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
}
