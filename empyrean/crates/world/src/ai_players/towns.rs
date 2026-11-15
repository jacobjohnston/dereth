//! The towns AI Players fill: where each stands, its size and the level it is for.
//!
//! The spots are the end of retail's (as the world's place marks have them). The levels are a
//! first guess, until the level map works them out from the land around each town.

use dereth_primitives::era::EraId;

/// How many AI Players a town holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Hub,
    Town,
    Outpost,
}

impl Size {
    #[must_use]
    pub const fn capacity(self) -> usize {
        match self {
            Self::Hub => 12,
            Self::Town => 8,
            Self::Outpost => 4,
        }
    }
}

/// One town: its name, north and east map coordinates, the first era it stands in, its size and
/// the level it is for.
#[derive(Debug, Clone, Copy)]
pub struct Town {
    pub name: &'static str,
    pub ns: f64,
    pub ew: f64,
    pub era: EraId,
    pub size: Size,
    pub level: u32,
}

const fn town(name: &'static str, ns: f64, ew: f64, era: EraId, size: Size, level: u32) -> Town {
    Town {
        name,
        ns,
        ew,
        era,
        size,
        level,
    }
}

use EraId::{Infiltration as I, Launch as L};
use Size::{Hub, Outpost, Town as T};

pub const TOWNS: [Town; 33] = [
    town("Al-Arqas", -31.17, 13.9, L, T, 10),
    town("Al-Jalima", 7.44, 4.83, L, T, 20),
    town("Arwic", 33.49, 56.78, L, Hub, 10),
    town("Ayan Baqur", -59.79, -87.91, I, Outpost, 60),
    town("Baishi", -49.43, 62.5, L, T, 30),
    town("Cragstone", 25.58, 49.19, L, Hub, 10),
    town("Dryreach", -7.9, 72.97, L, Outpost, 25),
    town("Eastham", 17.73, 63.37, L, T, 10),
    town("Fort Tethana", 1.55, -71.77, L, Outpost, 50),
    town("Glenden Wood", 29.74, 27.46, L, T, 5),
    town("Hebian-to", -39.17, 83.13, L, Hub, 15),
    town("Holtburg", 42.28, 33.31, L, Hub, 1),
    town("Kara", -83.29, 47.06, L, Outpost, 40),
    town("Khayyaban", -47.55, 24.71, L, T, 30),
    town("Kryst", -74.31, 84.41, I, Outpost, 60),
    town("Lin", -54.53, 73.21, L, T, 25),
    town("Lytelthorpe", 0.74, 51.43, L, T, 15),
    town("MacNiall's Freehold", -73.96, 92.19, I, Outpost, 70),
    town("Mayoi", -61.8, 82.09, L, T, 30),
    town("Nanto", -52.68, 82.27, L, Hub, 25),
    town("Qalaba'r", -74.34, 19.16, L, Hub, 35),
    town("Rithwic", 10.55, 58.27, L, T, 5),
    town("Samsur", -2.93, 19.44, L, T, 5),
    town("Sawato", -28.73, 59.39, L, T, 15),
    town("Shoushi", -33.57, 72.73, L, Hub, 1),
    town("Tou-Tou", -30.28, 94.77, L, Outpost, 40),
    town("Tufa", -14.0, 5.3, L, T, 15),
    town("Uziz", -25.14, 28.19, L, T, 20),
    town("Wai Jhou", -62.06, -51.3, I, Outpost, 60),
    town("Xarabydun", -41.87, 15.81, I, Outpost, 35),
    town("Yanshi", -12.13, 42.58, L, T, 15),
    town("Yaraq", -21.51, -1.52, L, Hub, 1),
    town("Zaikhal", 13.6, 0.61, L, Hub, 15),
];

/// The towns standing in a world of era `era`.
pub fn of_era(era: EraId) -> impl Iterator<Item = (usize, &'static Town)> {
    TOWNS.iter().enumerate().filter(move |(_, t)| t.era <= era)
}
