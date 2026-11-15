//! Divergence: V458
//! The world's map marks, in version 2 of the places: they mark what stands on the land, sum up
//! what a dungeon holds at its entrances, merge like spawn landblocks into areas and keep the towns
//! and landmarks of the world's era, each town where its vendors are; and `/places?version=2`
//! serves them after the places, which `/places` alone serves as before.
//! Fixture: a small in-memory world around Holtburg, a dungeon, two spawn regions and a town moved
//! east of its end-of-retail spot.

use std::sync::Arc;

use dereth_navigation::{MarkKind, PlaceStore};
use empyrean_common::era::EraId;
use empyrean_content::models::world::{
    Encounter, LandblockInstance, Weenie, WeeniePropertiesEmote, WeeniePropertiesGenerator,
};
use empyrean_content::MemContent;
use empyrean_entity::enums::{
    EmoteCategory, PositionType, PropertyBool, PropertyInt, PropertyString, WeenieType,
};
use empyrean_server::place_marks::{self, Mark};
use empyrean_server::portal_table::{self, PortalTable};
use empyrean_server::status_endpoint::{respond_with_portals, StatusSnapshot};

const NORTH: [f32; 4] = [1.0, 0.0, 0.0, 0.0];
const HOLTBURG: u32 = 0xA9B4_0000;
const DUNGEON: u32 = 0x01AC_0000;
const HALL: u32 = 0x01AD_0000;

fn named(id: u32, class: &str, t: WeenieType, name: &str) -> Weenie {
    Weenie::new(id, class, t).with_string(PropertyString::Name, name)
}

fn emote(w: Weenie, category: EmoteCategory) -> Weenie {
    let mut w = w;
    w.weenie_properties_emote.push(WeeniePropertiesEmote {
        category: u32::try_from(category.0).expect("a category"),
        probability: 1.0,
        ..Default::default()
    });
    w
}

fn generates(w: Weenie, wcid: u32) -> Weenie {
    let mut w = w;
    w.weenie_properties_generator
        .push(WeeniePropertiesGenerator {
            weenie_class_id: wcid,
            probability: 1.0,
            max_create: 1,
            ..Default::default()
        });
    w
}

fn world() -> MemContent {
    let mut guid = 0x7000_0000;
    let mut at = |content: MemContent, wcid: u32, cell: u32, x: f32, y: f32| {
        guid += 1;
        content.landblock_instance(LandblockInstance::new(guid, wcid, cell, [x, y, 0.0]))
    };
    let mut c = MemContent::new()
        .weenie(named(
            1,
            "vendor",
            WeenieType::Vendor,
            "Jonathan the Grocer",
        ))
        .weenie(named(2, "lifestone", WeenieType::LifeStone, "Life Stone"))
        .weenie(
            named(3, "drudge", WeenieType::Creature, "Drudge Skulker")
                .with_int(PropertyInt::Level, 10),
        )
        .weenie(
            named(4, "portaldungeon", WeenieType::Portal, "Drudge Hideout")
                .with_int(PropertyInt::MinLevel, 5)
                .with_position(
                    PositionType::Destination,
                    DUNGEON | 0x118,
                    [60.0, -50.0, 0.0],
                    NORTH,
                ),
        )
        .weenie(
            named(5, "portalland", WeenieType::Portal, "Arwic Portal").with_position(
                PositionType::Destination,
                0xC6A9_0000,
                [30.0, 100.0, 12.5],
                NORTH,
            ),
        )
        .weenie(
            named(
                6,
                "portalhall",
                WeenieType::Portal,
                "Holtburg Meeting Hall Portal",
            )
            .with_position(
                PositionType::Destination,
                HALL | 0x100,
                [10.0, 10.0, 0.0],
                NORTH,
            ),
        )
        .weenie(emote(
            named(7, "collector", WeenieType::Creature, "Ilsin the Collector")
                .with_bool(PropertyBool::Attackable, false),
            EmoteCategory::Refuse,
        ))
        .weenie(emote(
            named(8, "talker", WeenieType::Creature, "Town Crier")
                .with_bool(PropertyBool::Attackable, false),
            EmoteCategory::Use,
        ))
        .weenie(
            named(9, "barrier", WeenieType::Creature, "Mana Barrier")
                .with_bool(PropertyBool::Attackable, false),
        )
        .weenie(
            named(10, "cottage", WeenieType::SlumLord, "Cottage")
                .with_int(PropertyInt::HouseType, 1),
        )
        .weenie(
            named(11, "mosswart", WeenieType::Creature, "Mosswart").with_int(PropertyInt::Level, 5),
        )
        .weenie(generates(
            Weenie::new(12, "mosswartgen", WeenieType::Generic),
            11,
        ))
        // A generator of generators.
        .weenie(generates(
            named(
                13,
                "regiongen",
                WeenieType::Generic,
                "Low Swamp Mix Generator",
            ),
            12,
        ))
        .weenie(
            named(14, "golem", WeenieType::Creature, "Mud Golem").with_int(PropertyInt::Level, 30),
        )
        .weenie(generates(
            Weenie::new(15, "golemgen", WeenieType::Generic),
            14,
        ));
    // Holtburg: three vendors and a lifestone at its spot, one vendor in a building's room.
    for (x, y) in [(14.0, 55.0), (20.0, 60.0), (24.0, 50.0)] {
        c = at(c, 1, HOLTBURG | 0x03, x, y);
    }
    c = at(c, 1, HOLTBURG | 0x105, 30.0, 70.0);
    c = at(c, 2, HOLTBURG | 0x03, 16.0, 58.0);
    c = at(c, 4, HOLTBURG | 0x03, 40.0, 40.0);
    c = at(c, 5, HOLTBURG | 0x03, 42.0, 40.0);
    c = at(c, 6, HOLTBURG | 0x03, 44.0, 40.0);
    c = at(c, 7, HOLTBURG | 0x03, 50.0, 50.0);
    c = at(c, 8, HOLTBURG | 0x03, 52.0, 50.0);
    c = at(c, 9, HOLTBURG | 0x03, 54.0, 50.0);
    c = at(c, 10, HOLTBURG | 0x11, 100.0, 100.0);
    // The dungeon: a lifestone, a vendor and a drudge, none of them placed.
    c = at(c, 2, DUNGEON | 0x118, 60.0, -50.0);
    c = at(c, 1, DUNGEON | 0x119, 70.0, -50.0);
    c = at(c, 3, DUNGEON | 0x11A, 80.0, -50.0);
    // Yanshi's vendors, three units east of its end-of-retail spot (-12.13, 42.58).
    for x in [190.0, 186.0, 182.0] {
        c = at(c, 1, 0xB770_0001, x, 82.0);
    }
    let encounter = |landblock: i32, wcid: u32| Encounter {
        landblock,
        weenie_class_id: wcid,
        cell_x: 2,
        cell_y: 3,
        ..Default::default()
    };
    // A golem camp placed outdoors.
    c = at(c, 15, 0x5050_0001, 10.0, 10.0);
    // Three landblocks of a swamp region, and one too few to mark.
    c.encounter(encounter(0x1010, 13))
        .encounter(encounter(0x1011, 13))
        .encounter(encounter(0x1111, 13))
        .encounter(encounter(0x2020, 12))
}

/// The building's rooms are seen from outside; the dungeon's and the hall's are not.
fn rooms(cell: u32) -> Option<bool> {
    match cell >> 16 {
        0xA9B4 => Some(true),
        0x01AC | 0x01AD => Some(false),
        _ => None,
    }
}

fn places(era: EraId) -> Vec<Mark> {
    place_marks::marks(era, &world(), Some(&rooms))
}

fn of<'a>(places: &'a [Mark], kind: &str) -> Vec<&'a Mark> {
    places.iter().filter(|p| p.kind == kind).collect()
}

#[test]
fn what_stands_on_the_land_is_placed_and_a_dungeon_is_summed_up_at_its_entrance() {
    let all = places(EraId::Eor);
    let names =
        |kind: &str| -> Vec<&str> { of(&all, kind).iter().map(|p| p.name.as_str()).collect() };
    assert_eq!(
        names("vendor").len(),
        7,
        "Holtburg's four and Yanshi's three, none in the dungeon"
    );
    assert_eq!(
        names("lifestone"),
        ["Life Stone"],
        "the dungeon's is not placed"
    );
    assert_eq!(names("collector"), ["Ilsin the Collector"]);
    assert_eq!(
        names("npc"),
        ["Town Crier"],
        "a creature with nothing to say is no NPC"
    );
    assert_eq!(names("cottage"), ["Cottage"]);
    assert_eq!(names("meeting_hall"), ["Holtburg Meeting Hall Portal"]);

    let dungeon = of(&all, "dungeon");
    assert_eq!(dungeon.len(), 1);
    assert_eq!(dungeon[0].name, "Drudge Hideout");
    assert_eq!(
        dungeon[0].detail.as_deref(),
        Some("For levels 5 and up.\nHolds a lifestone and a vendor.\nDrudge Skulker spawns here at level 10.")
    );

    let portal = of(&all, "portal");
    assert_eq!(portal.len(), 1);
    assert_eq!(portal[0].detail.as_deref(), Some("Leads to 33.7N 56.6E."));
    let drop = of(&all, "drop");
    assert_eq!(drop.len(), 1);
    assert_eq!(drop[0].name, "Arwic Portal");
    assert!(
        (drop[0].ns - 33.67).abs() < 0.011 && (drop[0].ew - 56.57).abs() < 0.011,
        "{drop:?}"
    );
}

#[test]
fn a_region_s_landblocks_are_one_spawn_area_and_what_is_placed_outdoors_is_a_camp() {
    let all = places(EraId::Eor);
    let spawns = of(&all, "spawn");
    assert_eq!(
        spawns.len(),
        1,
        "a lone landblock is not marked: {spawns:?}"
    );
    assert_eq!(spawns[0].name, "Low Swamp");
    assert_eq!(
        spawns[0].detail.as_deref(),
        Some("Mosswart spawns here at level 5.")
    );
    let camps = of(&all, "camp");
    assert_eq!(camps.len(), 1, "{camps:?}");
    assert_eq!(camps[0].name, "Mud Golem");
    assert_eq!(
        camps[0].detail.as_deref(),
        Some("Mud Golem spawns here at level 30.")
    );
}

#[test]
fn towns_and_landmarks_are_those_of_the_world_s_era_and_a_town_stands_where_its_vendors_are() {
    let launch = places(EraId::Launch);
    let eor = places(EraId::Eor);
    let has = |all: &[Mark], name: &str| all.iter().any(|p| p.name == name);
    assert!(!has(&launch, "Ayan Baqur") && has(&eor, "Ayan Baqur"));
    assert!(!has(&launch, "Fiun Outpost") && has(&eor, "Fiun Outpost"));
    assert!(!has(&places(EraId::Infiltration), "Silyun"));
    assert!(has(&launch, "Stonehold") && !has(&launch, "Bandit Castle"));
    assert!(has(&launch, "The Pit") && !has(&places(EraId::Infiltration), "Westwatch"));
    let castle = eor
        .iter()
        .find(|p| p.name == "Bandit Castle")
        .expect("Bandit Castle");
    assert_eq!(
        (castle.ns, castle.ew),
        (66.4, 49.9),
        "where the wiki puts it"
    );

    let town = |name: &str| eor.iter().find(|p| p.name == name).expect(name).clone();
    let holtburg = town("Holtburg");
    assert_eq!(
        (holtburg.ns, holtburg.ew),
        (42.28, 33.31),
        "its vendors are at its spot"
    );
    let yanshi = town("Yanshi");
    assert!(
        yanshi.ew > 45.0 && (yanshi.ns + 12.0).abs() < 0.5,
        "moved to its vendors: {yanshi:?}"
    );
    let qalabar = town("Qalaba'r");
    assert_eq!(
        (qalabar.ns, qalabar.ew),
        (-74.34, 19.16),
        "no vendors here, so the curated spot"
    );
}

#[test]
fn without_the_cell_dat_a_room_in_a_landblock_with_land_is_a_building_s() {
    let all = place_marks::marks(EraId::Eor, &world(), None);
    assert_eq!(of(&all, "vendor").len(), 7);
    assert_eq!(of(&all, "dungeon").len(), 1);
}

#[test]
fn version_two_is_version_one_with_the_marks_after_the_places_in_kind_order() {
    let content = world();
    let marks = places(EraId::Launch);
    let one = portal_table::places_document("Test World", &content);
    let two = portal_table::places_document_2("Test World", &content, &marks);
    assert!(two.starts_with("dereth-places-store\t2\nworld\tTest World\n"));
    let places = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|l| l.starts_with("place\t"))
            .map(str::to_owned)
            .collect()
    };
    assert_eq!(places(&two), places(&one), "the same places");
    let store = PlaceStore::from_text(&two).expect("the client reads version 2");
    assert_eq!(store.places().len(), places(&one).len());
    assert_eq!(store.marks().len(), marks.len());
    let order = |k: MarkKind| MarkKind::ALL.iter().position(|&x| x == k).unwrap();
    assert!(store
        .marks()
        .windows(2)
        .all(|w| order(w[0].kind) <= order(w[1].kind)));
    for (m, read) in marks.iter().zip(store.marks()) {
        assert_eq!(read.kind.word(), m.kind);
        assert_eq!(read.name, m.name);
        assert_eq!(read.detail, m.detail, "{}", m.name);
        // The spot comes back as the game's coordinates within a hundredth.
        let o = read.at.frame.origin;
        let (ns, ew) = place_marks::coords(read.at.cell.0, o.x, o.y);
        assert!(
            (ns - m.ns).abs() < 0.001 && (ew - m.ew).abs() < 0.001,
            "{}: {ns} {ew} for {} {}",
            m.name,
            m.ns,
            m.ew
        );
        assert!(read.at.cell.0 & 0xFFFF < 0x100, "out of doors");
    }
    let dungeon = store
        .marks()
        .iter()
        .find(|m| m.kind == MarkKind::Dungeon)
        .expect("a dungeon");
    assert_eq!(
        dungeon.detail.as_deref(),
        Some("For levels 5 and up.\nHolds a lifestone and a vendor.\nDrudge Skulker spawns here at level 10.")
    );
}

#[test]
fn a_spot_is_written_as_the_land_cell_it_falls_in() {
    assert_eq!(
        place_marks::spot_position((42.28, 33.31)),
        "0xA9B40003 14.400 55.200 0.000",
        "Holtburg"
    );
    assert_eq!(
        place_marks::spot_position((-101.95, -101.95)),
        "0x00000001 0.000 0.000 0.000"
    );
}

fn snapshot() -> Option<StatusSnapshot> {
    None
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("utf-8 response")
}

#[test]
fn places_answer_version_two_only_to_a_client_that_asks() {
    let table = PortalTable::with_marks("Test World", EraId::Eor, Arc::new(world()), None);
    let (one, two) = (
        table.places().to_owned(),
        table.places_2_ready().expect("built").to_owned(),
    );
    assert!(one.starts_with("dereth-places-store\t1\n") && !one.contains("\nmark\t"));
    assert!(two.starts_with("dereth-places-store\t2\n") && two.contains("\nmark\ttown\t"));
    for (request, body) in [
        ("GET /places HTTP/1.1", &one),
        ("GET /places?version=1 HTTP/1.1", &one),
        ("GET /places?version=2 HTTP/1.1", &two),
        ("GET /places?x=1&version=2 HTTP/1.1", &two),
    ] {
        let r = text(&respond_with_portals(request, snapshot, Some(&table)));
        assert!(r.starts_with("HTTP/1.0 200 OK\r\n"), "{request}: {r}");
        assert!(r.ends_with(&format!("\r\n\r\n{body}")), "{request}");
    }
    let off = text(&respond_with_portals(
        "GET /places?version=2 HTTP/1.1",
        snapshot,
        None,
    ));
    assert!(off.starts_with("HTTP/1.0 404 "), "{off}");
    // A table made without marks still answers version 2, with none.
    let plain = PortalTable::new("Test World", Arc::new(world()));
    let _ = plain.places();
    let r = text(&respond_with_portals(
        "GET /places?version=2 HTTP/1.1",
        snapshot,
        Some(&plain),
    ));
    assert!(
        r.contains("dereth-places-store\t2\n") && !r.contains("\nmark\t"),
        "{r}"
    );
}
