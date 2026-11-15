//! Not ACE: the world's places for a map, published by the status endpoint as `GET /places` when
//! `server.publish_places` is on, and written by `empyrean-server --write-places <file>`.
//!
//! ACE publishes nothing like it. A client's map needs to know what stands where in this world,
//! which differs by era and by world; this is that list, read from the world content the server
//! loaded (its landblock instances, encounters and weenies) and the cell dat, never from the live
//! world, so no landblock is loaded to build it.
//!
//! The document is JSON: `{"version": 2, "world", "era", "content_hash", "places": [{"kind",
//! "name", "ns", "ew", "detail"?}]}`, `ns` and `ew` the game's coordinates (north and east
//! positive). The kinds:
//!
//! - `town` and `landmark`: [`TOWNS`] and [`LANDMARKS`], those of the world's era. A town stands
//!   where its vendors and lifestone are in this world, which moved for some between eras.
//! - `cottage`, `villa`, `mansion`: each house's slumlord.
//! - `lifestone`, `vendor`, `bindstone`.
//! - `npc`: a creature that cannot be attacked and has something to say; `collector` one that
//!   also takes items (a `Refuse` emote), which is how quests are turned in.
//! - `portal`: a portal on the land that leads to the land, `drop` where such a portal leads,
//!   `meeting_hall` a town's meeting hall portal.
//! - `dungeon`: a portal on the land that leads into a dungeon, its `detail` what the dungeon
//!   holds; `apartments` one into a block of apartments.
//! - `spawn`: an area of neighbouring landblocks whose encounters come mostly from one region
//!   generator, named for it, its detail the levels and commonest creatures; `camp` the
//!   generators and creatures placed outdoors in one landblock, named for the commonest.
//!
//! What stands in a dungeon is not placed, since a dungeon has no spot on the land; it is summed
//! up in the detail of the dungeon's entrances. A room the outside can be seen from (the cell
//! dat's `SeenOutside`) is a building's, and what stands in it is placed; without the cell dat, a
//! room in a landblock that has anything outdoors is taken for a building's.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use empyrean_common::era::EraId;
use empyrean_content::models::world::{LandblockInstance, Weenie};
use empyrean_content::WorldDatabase;
use empyrean_dat::file_types::EnvCell;
use empyrean_dat::DatManager;
use empyrean_entity::enums::{
    EmoteCategory, PositionType, PropertyBool, PropertyInt, PropertyString, WeenieType,
};
use serde::Serialize;

use crate::portal_table::{Documents, PortalTable};

/// The document's version; the client's bundled `map-places.json` was version 1.
pub const VERSION: u32 = 2;

/// The major towns, at their end-of-retail spots, and the first era each stands in.
pub const TOWNS: [(&str, f64, f64, EraId); 33] = [
    ("Al-Arqas", -31.17, 13.9, EraId::Launch),
    ("Al-Jalima", 7.44, 4.83, EraId::Launch),
    ("Arwic", 33.49, 56.78, EraId::Launch),
    ("Ayan Baqur", -59.79, -87.91, EraId::Infiltration),
    ("Baishi", -49.43, 62.5, EraId::Launch),
    ("Cragstone", 25.58, 49.19, EraId::Launch),
    ("Dryreach", -7.9, 72.97, EraId::Launch),
    ("Eastham", 17.73, 63.37, EraId::Launch),
    ("Fort Tethana", 1.55, -71.77, EraId::Launch),
    ("Glenden Wood", 29.74, 27.46, EraId::Launch),
    ("Hebian-to", -39.17, 83.13, EraId::Launch),
    ("Holtburg", 42.28, 33.31, EraId::Launch),
    ("Kara", -83.29, 47.06, EraId::Launch),
    ("Khayyaban", -47.55, 24.71, EraId::Launch),
    ("Kryst", -74.31, 84.41, EraId::Infiltration),
    ("Lin", -54.53, 73.21, EraId::Launch),
    ("Lytelthorpe", 0.74, 51.43, EraId::Launch),
    ("MacNiall's Freehold", -73.96, 92.19, EraId::Infiltration),
    ("Mayoi", -61.8, 82.09, EraId::Launch),
    ("Nanto", -52.68, 82.27, EraId::Launch),
    ("Qalaba'r", -74.34, 19.16, EraId::Launch),
    ("Rithwic", 10.55, 58.27, EraId::Launch),
    ("Samsur", -2.93, 19.44, EraId::Launch),
    ("Sawato", -28.73, 59.39, EraId::Launch),
    ("Shoushi", -33.57, 72.73, EraId::Launch),
    ("Tou-Tou", -30.28, 94.77, EraId::Launch),
    ("Tufa", -14.0, 5.3, EraId::Launch),
    ("Uziz", -25.14, 28.19, EraId::Launch),
    ("Wai Jhou", -62.06, -51.3, EraId::Infiltration),
    ("Xarabydun", -41.87, 15.81, EraId::Infiltration),
    ("Yanshi", -12.13, 42.58, EraId::Launch),
    ("Yaraq", -21.51, -1.52, EraId::Launch),
    ("Zaikhal", 13.6, 0.61, EraId::Launch),
];

/// The retail map's points of interest and smaller settlements, at the spots the Asheron's Call
/// wiki (loresraat.org) gives, and the first era each stands in: the patch that brought it, else
/// the first era whose world has anything there.
pub const LANDMARKS: [(&str, f64, f64, EraId); 90] = [
    ("Adventurer's Haven", 4.6, 35.4, EraId::Launch),
    ("Aerlinthe Island", 87.61, 47.81, EraId::Infiltration),
    ("Ahurenga", 47.1, -90.3, EraId::Infiltration),
    (
        "Ancient Town, Dungeon of Tatters",
        -61.9,
        40.0,
        EraId::Launch,
    ),
    ("Banderling Castle", 66.2, -2.3, EraId::Launch),
    ("Banderling Fort", 22.5, -10.5, EraId::Launch),
    ("Banderling Tower", 9.3, 63.7, EraId::Launch),
    ("Bandit Castle", 66.4, 49.9, EraId::Infiltration),
    ("Beach Fort", 76.0, -49.0, EraId::Launch),
    ("Black Death Catacombs", 59.9, 71.2, EraId::Launch),
    ("Black Hill", -62.4, -52.3, EraId::Launch),
    ("Bluespire", 39.4, -75.4, EraId::Infiltration),
    ("Candeth Keep", -87.5, -67.2, EraId::Infiltration),
    ("Castle of Baron Nuvillus", -24.2, 76.8, EraId::Launch),
    ("Cataracts of Sabella", 82.7, -74.7, EraId::Eor),
    ("Caulcano", -94.8, -94.8, EraId::Infiltration),
    ("Center of the Obsidian Plains", -64.1, -63.8, EraId::Launch),
    ("Chalicmere Castle", -17.0, -57.3, EraId::Launch),
    ("Chapel of Xi Ru", -90.6, 87.5, EraId::Infiltration),
    ("Corcima Castle", 83.2, -50.4, EraId::Eor),
    ("Crater Lake Village", 64.8, 13.5, EraId::Launch),
    ("Danby's Outpost", 23.4, -28.7, EraId::Launch),
    ("Desert Mine", -4.5, 26.6, EraId::Launch),
    ("East Fork Dam", 48.3, -76.6, EraId::Infiltration),
    ("Eastwatch", 90.3, -43.1, EraId::Eor),
    ("Empyrean Foundry", -25.2, 54.2, EraId::Infiltration),
    ("Empyrean Ruins", -71.1, 52.3, EraId::Launch),
    ("Empyrean Stone Tower", -22.8, -91.9, EraId::Launch),
    ("Empyrean Thermae", -31.8, 94.1, EraId::Launch),
    ("Fenmalain Vestibule", -46.9, 55.2, EraId::Infiltration),
    ("Fiun Outpost", 95.9, -56.8, EraId::Eor),
    ("Forking Trail", -65.6, 16.7, EraId::Launch),
    ("Fort Aimaru", -34.2, 80.7, EraId::Launch),
    ("Fort Witshire", 25.0, 37.8, EraId::Launch),
    ("Fycheck Ruins", -34.4, 64.9, EraId::Launch),
    ("Greenspire", 43.2, -66.8, EraId::Infiltration),
    (
        "High Mountain Valley, Lost Garden Ruins",
        -65.3,
        60.3,
        EraId::Launch,
    ),
    ("Ice Golem Shrine", 61.0, -10.2, EraId::Launch),
    ("Imuth Maer Doquin", -11.6, 57.9, EraId::Launch),
    ("Ithaenc Cathedral", -82.4, 93.4, EraId::Infiltration),
    ("Kivik Lir's Temple", -38.1, -47.3, EraId::Infiltration),
    ("Leikotha's Crypt", -10.1, 31.3, EraId::Launch),
    ("Linvak Tukal", -77.6, 28.0, EraId::Infiltration),
    ("Lugian Pillars", -61.2, 61.1, EraId::Launch),
    ("Mar'uun", -10.6, 17.1, EraId::Infiltration),
    ("Merchant Cellar", 26.3, 63.9, EraId::Launch),
    ("Merwart Village", 80.2, 59.4, EraId::Eor),
    ("Mines of Despair", -75.4, 57.2, EraId::Launch),
    ("Mountain Keep", 54.7, 32.9, EraId::Launch),
    ("Mountain Retreat", 59.7, -4.1, EraId::Launch),
    ("Neydisa Castle", 69.6, 17.6, EraId::Infiltration),
    ("Northern Pass Watchtower", 52.0, 3.1, EraId::Launch),
    ("Oolutanga's Refuge", 2.3, 95.5, EraId::Infiltration),
    ("Planetarium", -78.8, -73.8, EraId::Launch),
    ("Plateau Village", 44.2, -43.4, EraId::Launch),
    ("Power Plant", 38.6, 9.6, EraId::Launch),
    ("Redspire", 40.6, -83.0, EraId::Infiltration),
    ("Relic Town", 86.0, 45.4, EraId::Infiltration),
    ("Ruschk Ice Tower", 96.4, 54.6, EraId::Eor),
    ("Sanamar", 71.9, -61.1, EraId::Eor),
    ("Silyun", 87.3, -70.5, EraId::Eor),
    (
        "Singularity Caul Island",
        -92.71,
        -96.05,
        EraId::Infiltration,
    ),
    ("Stonehold", 68.8, -21.6, EraId::Launch),
    ("Swamp Temple", -23.4, 47.9, EraId::Launch),
    ("Temple of Xik Minru", 73.3, 69.0, EraId::Eor),
    ("Tenkarrdun", 90.3, 46.3, EraId::Infiltration),
    ("Thasali", -29.0, 26.6, EraId::Launch),
    ("The Pit", 64.3, -23.4, EraId::Launch),
    ("Timaru", 44.2, -78.0, EraId::Infiltration),
    ("Trialos", -58.7, 55.3, EraId::Launch),
    ("Tumerok Fort", -51.1, -82.0, EraId::Launch),
    ("Tumerok Fort", -45.2, -83.2, EraId::Launch),
    ("Tumerok Fort", 3.1, -60.5, EraId::Launch),
    ("Tumerok Fort", 10.3, -68.8, EraId::Launch),
    (
        "Tumerok Fort and Tumerok Fortress",
        12.3,
        -73.7,
        EraId::Launch,
    ),
    (
        "Tumerok Fort with Tumerok Dungeon",
        -9.7,
        -63.3,
        EraId::Launch,
    ),
    ("Tumerok Outpost", 1.9, -56.7, EraId::Launch),
    (
        "Tumerok Outpost with Tumerok Base",
        9.0,
        -65.7,
        EraId::Launch,
    ),
    ("Tumerok Stronghold", -40.0, -54.9, EraId::Launch),
    ("Tumerok Tower", 15.6, -49.9, EraId::Launch),
    ("Tumerok Towers", -1.4, -86.2, EraId::Launch),
    ("Tumerok Treehouse", 43.9, 10.8, EraId::Launch),
    ("Ulgrim's Island", -94.8, -9.7, EraId::Infiltration),
    ("Undead Shrine", 51.9, 0.6, EraId::Launch),
    ("Undead Village", 21.4, -24.8, EraId::Launch),
    ("Underground City", 21.4, 53.9, EraId::Launch),
    ("Vaeshok's Cave", 86.9, 59.8, EraId::Eor),
    ("Walled Portals", -21.6, -74.5, EraId::Launch),
    ("Westwatch", 72.8, -73.4, EraId::Eor),
    ("Wilderness Fort", 23.5, -7.7, EraId::Launch),
];

/// How near a town's spot its vendors and lifestone must be for it to stay there.
const TOWN_HERE: f64 = 1.5;
/// How far a town's vendors may have moved from its end-of-retail spot.
const TOWN_MOVED: f64 = 4.0;
/// How many vendors and lifestones make a town.
const TOWN_ANCHORS: usize = 3;
/// How many landblocks make a spawn area worth marking.
const SPAWN_BLOCKS: usize = 3;
/// How deep generators that spawn generators are followed.
const GENERATOR_DEPTH: u32 = 4;
/// How many creatures a spawn area is named for.
const SPAWN_NAMED: usize = 3;

/// The places for one world, built once on a thread of its own from the moment it is made, like
/// [`PortalTable`].
#[derive(Debug)]
pub struct PlaceTable(PortalTable);

impl PlaceTable {
    /// The places of `content` for the world named `world_name`, playing `era`, its rooms told
    /// apart by `dats` when given; its building begun.
    #[must_use]
    pub fn new(
        world_name: &str,
        era: EraId,
        content: Arc<dyn WorldDatabase>,
        dats: Option<Arc<DatManager>>,
    ) -> Self {
        let world_name = world_name.to_owned();
        Self::building(move || {
            let rooms = dats.as_deref().map(dat_rooms);
            document(
                &world_name,
                era,
                &*content,
                rooms.as_ref().map(|r| r as &dyn Fn(u32) -> Option<bool>),
            )
        })
    }

    /// The table `build` makes, its building begun on a thread of its own.
    #[must_use]
    pub fn building(build: impl FnOnce() -> String + Send + 'static) -> Self {
        // The document rides in the portal table's `portals` slot.
        Self(PortalTable::building(move || Documents {
            portals: build(),
            places: String::new(),
        }))
    }

    /// The document once it is built; `None` while it is being built.
    #[must_use]
    pub fn ready(&self) -> Option<&str> {
        self.0.ready()
    }

    /// The document, waiting for it to be built.
    #[must_use]
    pub fn document(&self) -> &str {
        self.0.document()
    }
}

/// Whether a room is a building's (seen from outside), read from `dats`' cell dat.
pub fn dat_rooms(dats: &DatManager) -> impl Fn(u32) -> Option<bool> + '_ {
    move |cell| {
        dats.cell_dat()
            .read_from_dat::<EnvCell>(cell)
            .map(|c| c.flags & 1 != 0)
    }
}

/// A place on the map.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Place {
    pub kind: &'static str,
    pub name: String,
    pub ns: f64,
    pub ew: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Serialize)]
struct Document<'a> {
    version: u32,
    world: &'a str,
    era: &'static str,
    content_hash: Option<String>,
    places: Vec<Place>,
}

/// The places document of `content` for the world named `world_name`, playing `era`; `rooms`
/// answers whether a room is a building's, `None` for one it does not know.
#[must_use]
pub fn document(
    world_name: &str,
    era: EraId,
    content: &dyn WorldDatabase,
    rooms: Option<&dyn Fn(u32) -> Option<bool>>,
) -> String {
    let doc = Document {
        version: VERSION,
        world: world_name,
        era: era.name(),
        content_hash: content.content_hash(),
        places: places(era, content, rooms),
    };
    let mut out = serde_json::to_string(&doc).unwrap_or_default();
    out.push('\n');
    out
}

/// `cell`'s spot at `x`, `y` within its landblock, as the game's coordinates: north, then east.
#[must_use]
pub fn coords(cell: u32, x: f32, y: f32) -> (f64, f64) {
    let block = |n: u32, at: f32| (f64::from(n) * 192.0 + f64::from(at)) / 240.0 - 101.95;
    (block((cell >> 16) & 0xFF, y), block(cell >> 24, x))
}

fn round(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn place(kind: &'static str, name: &str, (ns, ew): (f64, f64), detail: Option<String>) -> Place {
    Place {
        kind,
        name: name.to_owned(),
        ns: round(ns),
        ew: round(ew),
        detail,
    }
}

fn name_of(w: &Weenie) -> &str {
    w.weenie_properties_string
        .iter()
        .find(|p| p.r#type == PropertyString::Name.0)
        .map_or(w.class_name.as_str(), |p| p.value.as_str())
}

fn int_of(w: &Weenie, p: PropertyInt) -> Option<i32> {
    w.weenie_properties_int
        .iter()
        .find(|r| r.r#type == p.0)
        .map(|r| r.value)
}

fn is(w: &Weenie, t: WeenieType) -> bool {
    i64::from(w.r#type) == i64::from(t.0)
}

/// A house's kind by its slumlord: the house type when the weenie has one, else its name.
fn house_kind(w: &Weenie) -> Option<&'static str> {
    match int_of(w, PropertyInt::HouseType) {
        Some(1) => return Some("cottage"),
        Some(2) => return Some("villa"),
        Some(3) => return Some("mansion"),
        Some(4) => return Some("apartment"),
        _ => {}
    }
    match name_of(w).to_ascii_lowercase().as_str() {
        "cottage" => Some("cottage"),
        "villa" => Some("villa"),
        "mansion" => Some("mansion"),
        "apartment" => Some("apartment"),
        _ => None,
    }
}

/// A creature anyone may fight: not marked unattackable.
fn attackable(w: &Weenie) -> bool {
    !w.weenie_properties_bool
        .iter()
        .any(|r| r.r#type == PropertyBool::Attackable.0 && !r.value)
}

/// `w`'s destination cell and spot, when it has one.
fn destination(w: &Weenie) -> Option<(u32, f32, f32)> {
    w.weenie_properties_position
        .iter()
        .find(|p| p.position_type == PositionType::Destination.0 && p.obj_cell_id != 0)
        .map(|p| (p.obj_cell_id, p.origin_x, p.origin_y))
}

/// `ns`, `ew` as the game writes a place: `42.2N 33.8E`.
fn coords_text((ns, ew): (f64, f64)) -> String {
    format!(
        "{:.1}{} {:.1}{}",
        ns.abs(),
        if ns >= 0.0 { "N" } else { "S" },
        ew.abs(),
        if ew >= 0.0 { "E" } else { "W" }
    )
}

/// A portal's level limits as a sentence: `For levels 30 to 60.`
fn limits_text(w: &Weenie) -> Option<String> {
    match (
        int_of(w, PropertyInt::MinLevel).filter(|&l| l > 0),
        int_of(w, PropertyInt::MaxLevel).filter(|&l| l > 0),
    ) {
        (Some(lo), Some(hi)) => Some(format!("For levels {lo} to {hi}.")),
        (Some(lo), None) => Some(format!("For levels {lo} and up.")),
        (None, Some(hi)) => Some(format!("For levels up to {hi}.")),
        (None, None) => None,
    }
}

/// `lines` as a detail, one sentence a line; `None` for none.
fn lines(lines: impl IntoIterator<Item = String>) -> Option<String> {
    let text: Vec<String> = lines.into_iter().collect();
    (!text.is_empty()).then(|| text.join("\n"))
}

/// Creatures, each with its level.
type Made = Arc<Vec<(String, Option<i32>)>>;

/// The creatures a weenie stands for: itself when it is one anyone may fight, else what it
/// generates, followed through generators of generators; each with its level.
struct Spawns<'a> {
    content: &'a dyn WorldDatabase,
    known: HashMap<u32, Made>,
}

impl Spawns<'_> {
    fn of(&mut self, wcid: u32) -> Made {
        self.follow(wcid, 0)
    }

    fn follow(&mut self, wcid: u32, depth: u32) -> Made {
        if let Some(found) = self.known.get(&wcid) {
            return Arc::clone(found);
        }
        let mut out = Vec::new();
        if let Some(w) = self.content.get_weenie(wcid) {
            out = self.of_weenie(&w, depth);
        }
        let out = Arc::new(out);
        self.known.insert(wcid, Arc::clone(&out));
        out
    }

    fn of_weenie(&mut self, w: &Weenie, depth: u32) -> Vec<(String, Option<i32>)> {
        if is(w, WeenieType::Creature) {
            return if attackable(w) {
                vec![(name_of(w).to_owned(), int_of(w, PropertyInt::Level))]
            } else {
                Vec::new()
            };
        }
        let mut out = Vec::new();
        if depth < GENERATOR_DEPTH {
            for g in &w.weenie_properties_generator {
                if g.weenie_class_id != w.class_id {
                    out.extend(self.follow(g.weenie_class_id, depth + 1).iter().cloned());
                }
            }
        }
        out
    }
}

/// What a dungeon holds, for its entrances' detail.
#[derive(Debug, Default)]
struct Inside {
    lifestones: u32,
    vendors: u32,
    npcs: u32,
    apartments: u32,
    creatures: BTreeMap<String, u32>,
    levels: Option<(i32, i32)>,
}

impl Inside {
    fn creature(&mut self, name: &str, level: Option<i32>) {
        *self.creatures.entry(name.to_owned()).or_default() += 1;
        if let Some(l) = level.filter(|&l| l > 0) {
            self.levels = Some(self.levels.map_or((l, l), |(a, b)| (a.min(l), b.max(l))));
        }
    }

    /// What it holds as sentences, one a line: `Holds a lifestone and 3 vendors.` and
    /// `Drudge Skulker and Mosswart spawn here at levels 2 to 6.`
    fn text(&self) -> Option<String> {
        let count = |n: u32, one: &str, many: &str| match n {
            0 => None,
            1 => Some(one.to_owned()),
            n => Some(format!("{n} {many}")),
        };
        let held: Vec<String> = [
            count(self.lifestones, "a lifestone", "lifestones"),
            count(self.vendors, "a vendor", "vendors"),
            count(self.npcs, "a person to talk to", "people to talk to"),
        ]
        .into_iter()
        .flatten()
        .collect();
        let mut out = Vec::new();
        if !held.is_empty() {
            out.push(format!("Holds {}.", and_list(&held)));
        }
        out.extend(self.spawn_text());
        lines(out)
    }

    /// Its creatures as a sentence: `Drudge Skulker and Mosswart spawn here at levels 2 to 6.`
    fn spawn_text(&self) -> Option<String> {
        if self.creatures.is_empty() {
            return None;
        }
        let named = most_common(&self.creatures, SPAWN_NAMED);
        let verb = if self.creatures.len() == 1 {
            "spawns"
        } else {
            "spawn"
        };
        Some(match self.levels {
            Some((lo, hi)) if lo == hi => format!("{named} {verb} here at level {lo}."),
            Some((lo, hi)) => format!("{named} {verb} here at levels {lo} to {hi}."),
            None => format!("{named} {verb} here."),
        })
    }
}

/// `items` joined as a list in a sentence: `a, b and c`.
fn and_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The `n` commonest names of `counts`, then how many more: `Drudge, Mosswart and 4 more`.
fn most_common(counts: &BTreeMap<String, u32>, n: usize) -> String {
    let mut names: Vec<(&String, &u32)> = counts.iter().collect();
    names.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let mut shown: Vec<String> = names.iter().take(n).map(|(s, _)| (*s).clone()).collect();
    if names.len() > n {
        shown.push(format!("{} more", names.len() - n));
    }
    and_list(&shown)
}

/// Every place of `content` in a world playing `era`, in kind, then name, then spot order.
#[must_use]
pub fn places(
    era: EraId,
    content: &dyn WorldDatabase,
    rooms: Option<&dyn Fn(u32) -> Option<bool>>,
) -> Vec<Place> {
    let of_type = |t: WeenieType| content.get_landblock_instances_of_type(t);
    let portals = of_type(WeenieType::Portal);
    let lifestones = of_type(WeenieType::LifeStone);
    let vendors = of_type(WeenieType::Vendor);
    let creatures = of_type(WeenieType::Creature);
    let houses = of_type(WeenieType::SlumLord);
    let bindstones = of_type(WeenieType::AllegianceBindstone);
    let generics = of_type(WeenieType::Generic);
    let all = [
        &portals,
        &lifestones,
        &vendors,
        &creatures,
        &houses,
        &bindstones,
        &generics,
    ];

    // A landblock with anything outdoors has land; a room in one is a building's when there is
    // no cell dat to ask.
    let land_blocks: HashSet<u32> = all
        .iter()
        .flat_map(|v| v.iter())
        .filter(|(i, _)| i.obj_cell_id & 0xFFFF < 0x100)
        .map(|(i, _)| i.obj_cell_id >> 16)
        .collect();
    let on_land = |cell: u32| {
        if cell & 0xFFFF < 0x100 {
            return true;
        }
        rooms
            .and_then(|r| r(cell))
            .unwrap_or_else(|| land_blocks.contains(&(cell >> 16)))
    };
    let at = |i: &LandblockInstance| coords(i.obj_cell_id, i.origin_x, i.origin_y);

    let mut spawns = Spawns {
        content,
        known: HashMap::new(),
    };
    let mut out = Vec::new();

    // What each dungeon holds.
    let mut inside: HashMap<u32, Inside> = HashMap::new();
    for (i, _) in vendors.iter().filter(|(i, _)| !on_land(i.obj_cell_id)) {
        inside.entry(i.obj_cell_id >> 16).or_default().vendors += 1;
    }
    for (i, _) in lifestones.iter().filter(|(i, _)| !on_land(i.obj_cell_id)) {
        inside.entry(i.obj_cell_id >> 16).or_default().lifestones += 1;
    }
    for (i, w) in houses.iter().filter(|(i, _)| !on_land(i.obj_cell_id)) {
        if house_kind(w) == Some("apartment") {
            inside.entry(i.obj_cell_id >> 16).or_default().apartments += 1;
        }
    }
    for (i, w) in creatures.iter().filter(|(i, _)| !on_land(i.obj_cell_id)) {
        let here = inside.entry(i.obj_cell_id >> 16).or_default();
        if attackable(w) {
            here.creature(name_of(w), int_of(w, PropertyInt::Level));
        } else if !w.weenie_properties_emote.is_empty() {
            here.npcs += 1;
        }
    }
    for (i, w) in generics
        .iter()
        .filter(|(i, w)| !w.weenie_properties_generator.is_empty() && !on_land(i.obj_cell_id))
    {
        let made = spawns.of_weenie(w, 0);
        let here = inside.entry(i.obj_cell_id >> 16).or_default();
        for (name, level) in made {
            here.creature(&name, level);
        }
    }

    // The apartments a dungeon leads to: its own, and those one portal further in (a block of
    // apartments is reached through a hall of portals).
    let mut onward: HashMap<u32, HashSet<u32>> = HashMap::new();
    for (i, w) in portals.iter().filter(|(i, _)| !on_land(i.obj_cell_id)) {
        if let Some((to, _, _)) = destination(w) {
            onward
                .entry(i.obj_cell_id >> 16)
                .or_default()
                .insert(to >> 16);
        }
    }
    let apartments = |block: u32| -> u32 {
        let own = |b: &u32| inside.get(b).map_or(0, |h| h.apartments);
        own(&block)
            + onward.get(&block).map_or(0, |next| {
                next.iter().filter(|&&b| b != block).map(own).sum()
            })
    };

    // Portals on the land: into dungeons, apartments and meeting halls, or across the land.
    let mut drops: BTreeMap<(i64, i64), Place> = BTreeMap::new();
    for (i, w) in portals.iter().filter(|(i, _)| on_land(i.obj_cell_id)) {
        let Some((to, x, y)) = destination(w) else {
            continue;
        };
        let name = name_of(w);
        let limits = limits_text(w);
        let hall = name.contains("Meeting Hall");
        if !on_land(to) {
            let held = inside.get(&(to >> 16));
            let flats = apartments(to >> 16);
            let (kind, detail) = if hall {
                ("meeting_hall", None)
            } else if flats > 0 {
                ("apartments", Some(format!("Leads to {flats} apartments.")))
            } else {
                ("dungeon", held.and_then(Inside::text))
            };
            let detail = lines(limits.into_iter().chain(detail));
            out.push(place(kind, name, at(i), detail));
            continue;
        }
        let there = coords(to, x, y);
        let detail =
            lines(std::iter::once(format!("Leads to {}.", coords_text(there))).chain(limits));
        out.push(place(
            if hall { "meeting_hall" } else { "portal" },
            name,
            at(i),
            detail,
        ));
        #[allow(clippy::cast_possible_truncation)]
        let key = (
            (there.0 * 10.0).round() as i64,
            (there.1 * 10.0).round() as i64,
        );
        drops
            .entry(key)
            .and_modify(|d| {
                if !d.name.split(", ").any(|n| n == name) {
                    d.name.push_str(", ");
                    d.name.push_str(name);
                }
            })
            .or_insert_with(|| {
                place(
                    "drop",
                    name,
                    there,
                    Some(format!("A portal at {} leads here.", coords_text(at(i)))),
                )
            });
    }
    out.extend(drops.into_values());

    for (i, w) in lifestones.iter().filter(|(i, _)| on_land(i.obj_cell_id)) {
        out.push(place("lifestone", name_of(w), at(i), None));
    }
    for (i, w) in vendors.iter().filter(|(i, _)| on_land(i.obj_cell_id)) {
        out.push(place("vendor", name_of(w), at(i), None));
    }
    for (i, w) in creatures.iter().filter(|(i, _)| on_land(i.obj_cell_id)) {
        if attackable(w) || w.weenie_properties_emote.is_empty() {
            continue;
        }
        let takes = w
            .weenie_properties_emote
            .iter()
            .any(|e| i64::from(e.category) == i64::from(EmoteCategory::Refuse.0));
        out.push(place(
            if takes { "collector" } else { "npc" },
            name_of(w),
            at(i),
            None,
        ));
    }
    for (i, w) in houses.iter().filter(|(i, _)| on_land(i.obj_cell_id)) {
        let Some(kind) = house_kind(w).filter(|&k| k != "apartment") else {
            continue;
        };
        out.push(place(kind, name_of(w), at(i), None));
    }
    for (i, w) in bindstones.iter().filter(|(i, _)| on_land(i.obj_cell_id)) {
        out.push(place("bindstone", name_of(w), at(i), None));
    }

    out.extend(spawn_areas(content, &mut spawns, &creatures, &generics));
    out.extend(named_places(era, &vendors, &lifestones, &on_land));

    let order = |k: &str| KINDS.iter().position(|&x| x == k).unwrap_or(KINDS.len());
    out.sort_by(|a, b| {
        order(a.kind)
            .cmp(&order(b.kind))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.ns.total_cmp(&b.ns))
            .then_with(|| a.ew.total_cmp(&b.ew))
    });
    out
}

/// Every kind, in the document's order.
pub const KINDS: [&str; 17] = [
    "town",
    "landmark",
    "cottage",
    "villa",
    "mansion",
    "lifestone",
    "dungeon",
    "portal",
    "drop",
    "meeting_hall",
    "apartments",
    "vendor",
    "npc",
    "collector",
    "bindstone",
    "spawn",
    "camp",
];

/// The towns and landmarks of `era`, each town where its vendors and lifestones are.
fn named_places(
    era: EraId,
    vendors: &[(LandblockInstance, Arc<Weenie>)],
    lifestones: &[(LandblockInstance, Arc<Weenie>)],
    on_land: &dyn Fn(u32) -> bool,
) -> Vec<Place> {
    let anchors: Vec<(f64, f64)> = vendors
        .iter()
        .chain(lifestones)
        .filter(|(i, _)| on_land(i.obj_cell_id))
        .map(|(i, _)| coords(i.obj_cell_id, i.origin_x, i.origin_y))
        .collect();
    let within = |spot: (f64, f64), r: f64| -> Vec<(f64, f64)> {
        anchors
            .iter()
            .copied()
            .filter(|a| (a.0 - spot.0).powi(2) + (a.1 - spot.1).powi(2) <= r * r)
            .collect()
    };
    let mut out = Vec::new();
    for &(name, ns, ew, first) in &TOWNS {
        if era < first {
            continue;
        }
        let mut spot = (ns, ew);
        if within(spot, TOWN_HERE).len() < TOWN_ANCHORS {
            let near = within(spot, TOWN_MOVED);
            if near.len() >= TOWN_ANCHORS {
                #[allow(clippy::cast_precision_loss)]
                let n = near.len() as f64;
                spot = (
                    near.iter().map(|a| a.0).sum::<f64>() / n,
                    near.iter().map(|a| a.1).sum::<f64>() / n,
                );
            }
        }
        out.push(place("town", name, spot, None));
    }
    for &(name, ns, ew, first) in &LANDMARKS {
        if era >= first {
            out.push(place("landmark", name, (ns, ew), None));
        }
    }
    out
}

/// The spawn areas and camps. An area is neighbouring landblocks whose encounters come mostly
/// from the same region generator, named for it; a camp is the generators and creatures placed
/// outdoors in one landblock, named for its commonest creatures. Each is marked at the landblock
/// (or, for a camp, the spot) nearest its middle.
fn spawn_areas(
    content: &dyn WorldDatabase,
    spawns: &mut Spawns<'_>,
    creatures: &[(LandblockInstance, Arc<Weenie>)],
    generics: &[(LandblockInstance, Arc<Weenie>)],
) -> Vec<Place> {
    let mut out = Vec::new();
    // Each landblock's commonest encounter generator.
    let mut region: BTreeMap<u16, u32> = BTreeMap::new();
    for block in 0..=0xFFFF_u16 {
        let encounters = content.get_cached_encounters_by_landblock(block);
        let mut counts: BTreeMap<u32, u32> = BTreeMap::new();
        for e in encounters.iter() {
            *counts.entry(e.weenie_class_id).or_default() += 1;
        }
        if let Some((&wcid, _)) = counts
            .iter()
            .max_by_key(|(&w, &n)| (n, std::cmp::Reverse(w)))
        {
            region.insert(block, wcid);
        }
    }
    let mut taken: HashSet<u16> = HashSet::new();
    for (&seed, &wcid) in &region {
        if !taken.insert(seed) {
            continue;
        }
        let mut members = vec![seed];
        let mut queue = vec![seed];
        while let Some(b) = queue.pop() {
            for n in neighbours(b) {
                if region.get(&n) == Some(&wcid) && taken.insert(n) {
                    members.push(n);
                    queue.push(n);
                }
            }
        }
        if members.len() < SPAWN_BLOCKS {
            continue;
        }
        let mut area = Inside::default();
        for (name, level) in spawns.of(wcid).iter() {
            area.creature(name, *level);
        }
        if area.creatures.is_empty() {
            continue;
        }
        let name = content
            .get_weenie(wcid)
            .map(|w| region_name(name_of(&w)))
            .filter(|n| !n.is_empty() && !n.contains("Unnamed"))
            .unwrap_or_else(|| most_common(&area.creatures, SPAWN_NAMED));
        let centre = |b: u16| coords(u32::from(b) << 16, 96.0, 96.0);
        let spot = nearest_middle(&members.iter().map(|&m| centre(m)).collect::<Vec<_>>());
        out.push(place("spawn", &name, spot, area.text()));
    }

    // Camps: what is placed outdoors, by landblock.
    let mut camps: BTreeMap<u32, (Inside, Vec<(f64, f64)>)> = BTreeMap::new();
    let outdoors = |i: &LandblockInstance| i.obj_cell_id & 0xFFFF < 0x100;
    for (i, w) in creatures
        .iter()
        .filter(|(i, w)| outdoors(i) && attackable(w))
    {
        let camp = camps.entry(i.obj_cell_id >> 16).or_default();
        camp.0.creature(name_of(w), int_of(w, PropertyInt::Level));
        camp.1.push(coords(i.obj_cell_id, i.origin_x, i.origin_y));
    }
    for (i, w) in generics
        .iter()
        .filter(|(i, w)| outdoors(i) && !w.weenie_properties_generator.is_empty())
    {
        let made = spawns.of_weenie(w, 0);
        if made.is_empty() {
            continue;
        }
        let camp = camps.entry(i.obj_cell_id >> 16).or_default();
        for (name, level) in made {
            camp.0.creature(&name, level);
        }
        camp.1.push(coords(i.obj_cell_id, i.origin_x, i.origin_y));
    }
    for (camp, spots) in camps.values() {
        if camp.creatures.is_empty() {
            continue;
        }
        out.push(place(
            "camp",
            &most_common(&camp.creatures, SPAWN_NAMED),
            nearest_middle(spots),
            camp.spawn_text(),
        ));
    }
    out
}

/// The four landblocks beside `b`.
fn neighbours(b: u16) -> impl Iterator<Item = u16> {
    let (x, y) = (b >> 8, b & 0xFF);
    [
        (x > 0).then(|| b - 0x100),
        (x < 0xFF).then(|| b + 0x100),
        (y > 0).then(|| b - 1),
        (y < 0xFF).then(|| b + 1),
    ]
    .into_iter()
    .flatten()
}

/// Of `spots`, the one nearest their mean.
fn nearest_middle(spots: &[(f64, f64)]) -> (f64, f64) {
    #[allow(clippy::cast_precision_loss)]
    let n = spots.len().max(1) as f64;
    let mean = (
        spots.iter().map(|s| s.0).sum::<f64>() / n,
        spots.iter().map(|s| s.1).sum::<f64>() / n,
    );
    let d = |p: &(f64, f64)| (p.0 - mean.0).powi(2) + (p.1 - mean.1).powi(2);
    spots
        .iter()
        .copied()
        .min_by(|a, b| d(a).total_cmp(&d(b)))
        .unwrap_or(mean)
}

/// A region generator's name as a place: `Low North Forest Mix Generator` is `Low North Forest`.
fn region_name(name: &str) -> String {
    let mut name = name.trim();
    for tail in [" Generator", " Gen", " Mix"] {
        name = name.strip_suffix(tail).unwrap_or(name).trim_end();
    }
    name.to_owned()
}
