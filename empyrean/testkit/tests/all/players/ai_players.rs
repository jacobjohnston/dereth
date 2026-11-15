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

    /// A load run: AI Players logged in across every town in steps (`AI_PLAYERS_LOAD`, default
    /// 50,100,200,300), each step played for `AI_PLAYERS_SECONDS` (default 120) at real speed,
    /// and what the world spent on it. Run it in release:
    /// `cargo test --release -p empyrean-testkit --features real-content --test all ai_players_load -- --ignored --nocapture`.
    #[test]
    #[ignore = "a load run, by hand"]
    fn ai_players_load() {
        let steps: Vec<usize> = std::env::var("AI_PLAYERS_LOAD")
            .unwrap_or_else(|_| "50,100,200,300".into())
            .split(',')
            .filter_map(|n| n.trim().parse().ok())
            .collect();
        let seconds: usize = std::env::var("AI_PLAYERS_SECONDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(120);
        let mut l = create_character(&[44, 6, 21, 22, 24]);
        l.enter_world();
        let towns: Vec<usize> = ai_players::towns::of_era(l.ts.world.era.id)
            .map(|(i, _)| i)
            .collect();
        {
            let w = &mut l.ts.world;
            w.ai_players.pinned = true;
            w.ai_players.max_total_override = Some(10_000);
            ai_players::set_enabled(w, true);
        }
        let rss = || {
            std::process::Command::new("ps")
                .args(["-o", "rss=", "-p", &std::process::id().to_string()])
                .output()
                .ok()
                .and_then(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .trim()
                        .parse::<u64>()
                        .ok()
                })
                .map_or(0, |kb| kb / 1024)
        };
        println!(
            "AI Players load run: {} towns, {seconds} s a step",
            towns.len()
        );
        println!(
            "{:>5} {:>9} {:>9} {:>9} {:>9} {:>10} {:>9} {:>8}",
            "bots",
            "build ms",
            "tick avg",
            "tick p95",
            "tick max",
            "landblocks",
            "objects",
            "RSS MB"
        );
        let mut made = 0;
        for &n in &steps {
            // Log in up to n, one town after another.
            let started = std::time::Instant::now();
            let before = made;
            while made < n {
                let town = towns[made % towns.len()];
                ai_players::log_in_at(&mut l.ts.world, town).expect("a login");
                made += 1;
                for _ in 0..3 {
                    l.ts.step();
                }
            }
            #[allow(clippy::cast_precision_loss)]
            let build = started.elapsed().as_secs_f64() * 1000.0 / (made - before).max(1) as f64;
            // Settle: every login done and placed.
            l.advance(10.0);

            // Play at real speed and time each tick.
            let mut ticks = Vec::new();
            for _ in 0..seconds {
                let second = std::time::Instant::now();
                for _ in 0..60 {
                    let t = std::time::Instant::now();
                    l.ts.step();
                    ticks.push(t.elapsed().as_secs_f64() * 1000.0);
                }
                if let Some(rest) = Duration::from_secs(1).checked_sub(second.elapsed()) {
                    std::thread::sleep(rest);
                }
            }
            ticks.sort_by(f64::total_cmp);
            #[allow(clippy::cast_precision_loss)]
            let avg = ticks.iter().sum::<f64>() / ticks.len() as f64;
            let p95 = ticks[ticks.len() * 95 / 100];
            let max = ticks.last().copied().unwrap_or(0.0);
            let w = &l.ts.world;
            let in_world = w
                .ai_players
                .bots
                .values()
                .filter(|b| b.stage == Stage::InWorld)
                .count();
            println!(
                "{:>5} {:>9.1} {:>9.2} {:>9.2} {:>9.2} {:>10} {:>9} {:>8}",
                in_world,
                build,
                avg,
                p95,
                max,
                empyrean_world::managers::landblock_manager::get_loaded_landblocks(w).len(),
                w.objects.len(),
                rss()
            );
        }
    }

    #[test]
    fn a_hunter_walks_out_from_town_and_fights_monsters_of_its_level() {
        let (mut l, _) = holtburg_with_a_bot();
        let w = &mut l.ts.world;
        ai_players::set_enabled(w, false);
        w.ai_players.pinned = true;
        let me = ai_players::map_coords(&l.location());
        let w = &mut l.ts.world;
        let ground = ai_players::hunt::pick_ground(w, me, 1500.0, &[me], 400.0).expect("a ground");
        let level = ai_players::hunt::level_for(w, ground).expect("a level");
        // Only this hunter: no more logins round the real player.
        let have = u32::try_from(w.ai_players.bots.len()).expect("few");
        w.ai_players.max_total_override = Some(have + 1);
        ai_players::set_enabled(w, true);
        ai_players::log_in_hunter_at(w, ground).expect("a hunter");
        let mut hunter = None;
        let mut fought = false;
        let mut arrived = false;
        for second in 0..300 {
            ai_players::planner::Planner::wait(&mut l.ts.world, Duration::from_secs(60));
            l.advance(1.0);
            let w = &l.ts.world;
            if second % 15 == 0 {
                println!("{second} s: {:?}", ai_players::describe(w));
            }
            let Some((id, b)) = w.ai_players.bots.iter().find(|(_, b)| b.hunt.is_some()) else {
                continue;
            };
            hunter = Some(*id);
            let h = b.hunt.as_ref().expect("a hunter");
            arrived |= h.arrived;
            if h.kills > 0 {
                fought = true;
                break;
            }
        }
        let id = hunter.expect("the hunter logged in");
        let made =
            l.ts.world
                .objects
                .get(ObjectGuid::new(id))
                .and_then(WorldObject::level);
        println!(
            "Hunter level {made:?} for ground {ground:04X} (level {level}): arrived {arrived}, fought {fought}: {:?}",
            ai_players::describe(&l.ts.world)
        );
        assert!(arrived, "the hunter reached its ground");
        assert!(fought, "the hunter killed a monster");
    }

    /// `PROGRESS`/`RESULT` lines for a load run: the world's tick times over `ticks`, its
    /// landblocks, objects and memory, the AI Players by kind, and the route planner's backlog.
    fn load_line(kind: &str, l: &Loop, ticks: &mut [f64], extra: &str) -> String {
        ticks.sort_by(f64::total_cmp);
        #[allow(clippy::cast_precision_loss)]
        let avg = ticks.iter().sum::<f64>() / ticks.len().max(1) as f64;
        let p95 = ticks.get(ticks.len() * 95 / 100).copied().unwrap_or(0.0);
        let max = ticks.last().copied().unwrap_or(0.0);
        let w = &l.ts.world;
        let bots = &w.ai_players.bots;
        let in_world = bots.values().filter(|b| b.stage == Stage::InWorld).count();
        let hunters = bots.values().filter(|b| b.hunt.is_some()).count();
        let out = bots
            .values()
            .filter(|b| b.hunt.as_ref().is_some_and(|h| h.arrived))
            .count();
        let fighting = bots
            .values()
            .filter(|b| b.hunt.as_ref().is_some_and(|h| h.target.is_some()))
            .count();
        let (backlog, answered, busy) = ai_players::planner::Planner::stats(w);
        let rss = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .parse::<u64>()
                    .ok()
            })
            .map_or(0, |kb| kb / 1024);
        format!(
            "{kind} bots {in_world}/{} hunters {hunters} (at ground {out}, fighting {fighting}) \
             tick avg {avg:.2} p95 {p95:.2} max {max:.1} ms, landblocks {}, objects {}, RSS {rss} MB, \
             routes {answered} (planner {busy:.0} s, backlog {backlog}), failures {:?}{extra}",
            bots.len(),
            empyrean_world::managers::landblock_manager::get_loaded_landblocks(w).len(),
            w.objects.len(),
            w.ai_players.failures,
        )
    }

    /// Runs the world one second at real speed, timing each tick.
    fn real_second(l: &mut Loop, ticks: &mut Vec<f64>) {
        let second = std::time::Instant::now();
        for _ in 0..60 {
            let t = std::time::Instant::now();
            l.ts.step();
            ticks.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        if let Some(rest) = Duration::from_secs(1).checked_sub(second.elapsed()) {
            std::thread::sleep(rest);
        }
    }

    /// A load run with hunters: 299 stand-in players spread as far apart as the land with
    /// monsters allows, and the director filling towns and hunting grounds round them up to each
    /// cap in `AI_PLAYERS_CAPS` (default 100,200,300,500), each then played for
    /// `AI_PLAYERS_SECONDS` (default 120) at real speed. Prints a `PROGRESS` line every 10 s and
    /// a `RESULT` line per cap. In release:
    /// `cargo test --release -p empyrean-testkit --features real-content --test all ai_players_hunter_load -- --ignored --nocapture`.
    #[test]
    #[ignore = "a load run, by hand"]
    fn ai_players_hunter_load() {
        let caps: Vec<u32> = std::env::var("AI_PLAYERS_CAPS")
            .unwrap_or_else(|_| "100,200,300,500".into())
            .split(',')
            .filter_map(|n| n.trim().parse().ok())
            .collect();
        let seconds: usize = std::env::var("AI_PLAYERS_SECONDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(120);
        let players: usize = std::env::var("AI_PLAYERS_PLAYERS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(299);
        let mut l = create_character(&[44, 6, 21, 22, 24]);
        l.enter_world();

        // The stand-ins: farthest-point picks over every landblock with monsters outdoors.
        let started = std::time::Instant::now();
        let w = &mut l.ts.world;
        let land: Vec<u16> = (0..=0xFFFFu16)
            .filter(|&b| !ai_players::levels::of_block(w, b).is_empty())
            .collect();
        let at = |b: u16| ai_players::hunt::map_centre(b);
        let mut picked = vec![ai_players::levels::block_at(42.28, 33.31)];
        let mut nearest: Vec<f64> = land
            .iter()
            .map(|&b| ai_players::levels::distance_between(at(b), at(picked[0])))
            .collect();
        while picked.len() < players {
            let (i, _) = nearest
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .expect("land");
            let b = land[i];
            picked.push(b);
            for (j, &o) in land.iter().enumerate() {
                nearest[j] = nearest[j].min(ai_players::levels::distance_between(at(o), at(b)));
            }
        }
        let spacing = nearest.iter().copied().fold(0.0f64, f64::max);
        println!(
            "PROGRESS {} landblocks with monsters; {} stand-ins, none farther than {spacing:.0} m from one, in {:.1} s",
            land.len(),
            picked.len(),
            started.elapsed().as_secs_f64()
        );
        w.ai_players.extra_anchors = picked.iter().map(|&b| at(b)).collect();
        w.ai_players.fast_fill = true;
        ai_players::set_enabled(w, true);

        for &cap in &caps {
            l.ts.world.ai_players.max_total_override = Some(cap);
            // Fill to the cap at real speed, reporting every 10 s.
            let fill = std::time::Instant::now();
            let mut ticks = Vec::new();
            let mut last = std::time::Instant::now();
            // The most there have been, and since when: demand met is no more logins for 20 s.
            let mut most = (0, std::time::Instant::now());
            loop {
                real_second(&mut l, &mut ticks);
                let w = &l.ts.world;
                let active = w
                    .ai_players
                    .bots
                    .values()
                    .filter(|b| b.stage == Stage::InWorld)
                    .count();
                let total = w.ai_players.bots.len();
                if total > most.0 {
                    most = (total, std::time::Instant::now());
                }
                let met = most.1.elapsed() >= Duration::from_secs(20);
                let full = active >= usize::try_from(cap).unwrap_or(usize::MAX) || met;
                if last.elapsed() >= Duration::from_secs(10) || full {
                    last = std::time::Instant::now();
                    println!(
                        "{}",
                        load_line(
                            &format!(
                                "PROGRESS cap {cap} filling {:.0} s:",
                                fill.elapsed().as_secs_f64()
                            ),
                            &l,
                            &mut ticks,
                            ""
                        )
                    );
                    ticks.clear();
                }
                if full || fill.elapsed() > Duration::from_secs(900) {
                    break;
                }
            }
            // Play at the cap.
            let mut ticks = Vec::new();
            let mut window = Vec::new();
            let mut last = std::time::Instant::now();
            for _ in 0..seconds {
                let mut one = Vec::new();
                real_second(&mut l, &mut one);
                window.extend_from_slice(&one);
                ticks.extend(one);
                if last.elapsed() >= Duration::from_secs(10) {
                    last = std::time::Instant::now();
                    println!(
                        "{}",
                        load_line(&format!("PROGRESS cap {cap} playing:"), &l, &mut window, "")
                    );
                    window.clear();
                }
            }
            println!(
                "{}",
                load_line(
                    &format!("RESULT cap {cap}:"),
                    &l,
                    &mut ticks,
                    &format!(
                        ", filled in {:.0} s",
                        fill.elapsed().as_secs_f64() - seconds as f64
                    )
                )
            );
        }
    }
}
