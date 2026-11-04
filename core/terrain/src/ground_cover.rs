//! Ground cover: plants scattered over the land by terrain type, which retail never places and
//! nothing but the picture knows of.
//!
//! Each kind of plant is laid on a grid of its own across the whole world, as dense as the kind
//! asks, each point moved within its square by a hash of where it is. So a plant stands in the
//! same place whenever its block is worked out, and a block's edge is no seam. A point keeps its
//! plant by how much of the ground about it is one the kind grows on, off roads, on a slope no
//! steeper than the kind allows, and thinning toward the walls of buildings. That share is the
//! terrain's vertices softened by their neighbours and read through a slow wobble, so a kind thins
//! into the next over tens of metres along no grid line, and its plants grow shorter as it thins.

use dereth_assets::world::CellLandblock;
use dereth_primitives::num::{floor_to_i32, math};
use dereth_primitives::Vec3;

use crate::consts::{BLOCK_LENGTH, CELL_SIZE, SIDE_VERTEX_COUNT};
use crate::land::mesh::{plane_set_height, LandblockMesh};
use crate::road::on_road;
use crate::scenery::{find_terrain_poly, outside_cell_index};

/// One kind of plant and where it grows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoverKind {
    /// The number the presentation knows its model by.
    pub id: u32,
    /// The terrain types it grows on, a bit each.
    pub grounds: u32,
    /// How many plants a square metre.
    pub density: f32,
    /// The smallest and the largest it is scaled by.
    pub scale: (f32, f32),
    /// The steepest slope it grows on, degrees.
    pub steepest: f32,
}

/// One plant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoverPlant {
    /// The kind's number.
    pub id: u32,
    /// Where it stands, block-local, on the ground.
    pub at: Vec3,
    /// How far it is turned about the vertical, radians.
    pub turn: f32,
    /// How it is scaled.
    pub scale: f32,
}

/// What the scatter asks of the land under a block.
pub trait CoverLand {
    /// The terrain word of the block's vertex `(a, b)`, each `0..=8`.
    fn word(&self, a: usize, b: usize) -> u16;
    /// Whether a road runs at block-local `(x, y)`.
    fn road(&self, x: f32, y: f32) -> bool;
    /// How far block-local `(x, y)` lies from the nearest building's walls, metres: nought inside
    /// one, infinite with none near.
    fn clearing(&self, x: f32, y: f32) -> f32;
    /// The ground at block-local `(x, y)`: its height and how nearly level it faces, the height
    /// of its normal.
    fn surface(&self, x: f32, y: f32) -> Option<(f32, f32)>;
}

/// A block's retail records as the land the scatter asks of.
pub struct RetailLand<'a> {
    /// The block's terrain words.
    pub landblock: &'a CellLandblock,
    /// Its full-detail mesh.
    pub mesh: &'a LandblockMesh,
    /// How far a block-local point lies from the nearest building's walls.
    pub clearing: &'a dyn Fn(f32, f32) -> f32,
}

impl std::fmt::Debug for RetailLand<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RetailLand")
            .field("block", &self.landblock.id)
            .finish_non_exhaustive()
    }
}

impl CoverLand for RetailLand<'_> {
    fn word(&self, a: usize, b: usize) -> u16 {
        self.landblock.terrain[a * SIDE_VERTEX_COUNT + b]
    }

    fn road(&self, x: f32, y: f32) -> bool {
        on_road(&self.landblock.terrain, x, y)
    }

    fn clearing(&self, x: f32, y: f32) -> f32 {
        (self.clearing)(x, y)
    }

    fn surface(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        let plane = find_terrain_poly(self.mesh, outside_cell_index(x, y), x, y)?;
        Some((plane_set_height(plane, x, y)?, plane.normal.z))
    }
}

/// A hash of `v`, mixed so nearby inputs give unrelated outputs.
fn mix(mut v: u32) -> u32 {
    v ^= v >> 16;
    v = v.wrapping_mul(0x7FEB_352D);
    v ^= v >> 15;
    v = v.wrapping_mul(0x846C_A68B);
    v ^ (v >> 16)
}

/// A number in `0..1` for grid point `(gx, gy)` of kind `kind`'s use `what`.
fn noise(kind: u32, gx: i32, gy: i32, what: u32) -> f32 {
    // LINT-OK: grid indices hashed by their bits. Not a numeric conversion.
    #[allow(clippy::cast_sign_loss)]
    let h = mix(mix(gx as u32 ^ what.rotate_left(24)) ^ mix(gy as u32 ^ kind.rotate_left(12)));
    // LINT-OK: a hash scaled into 0..1; its low bits are not needed.
    #[allow(clippy::cast_precision_loss)]
    let unit = (h >> 8) as f32 / (1u32 << 24) as f32;
    unit
}

/// How far the edge of a kind's ground wanders from the terrain's grid, metres, and over how many.
const WANDER: f32 = 6.0;
const WANDER_WAVE: f32 = 18.0;
/// How far apart, metres, plants grow to heights of their own rather than their neighbours'.
const SWATHE: f32 = 7.0;
/// How large a plant grows where its kind has all but thinned out, of its full size.
const THIN_SIZE: f32 = 0.55;
/// How near a building's walls nothing grows, and how much farther the plants take to grow thick.
const BARE: f32 = 0.5;
const THICKENS: f32 = 8.0;
/// How far from a building's walls its plants still thin, metres: no farther need be measured.
pub const CLEARING_REACH: f32 = BARE + THICKENS;

/// `0` at or below `lo`, `1` at or above `hi`, and an S between.
fn smoothstep(lo: f32, hi: f32, v: f32) -> f32 {
    let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A number in `0..1` that drifts smoothly across world point `(wx, wy)` over about `wave`
/// metres, for kind `kind`'s use `what`.
fn drift(kind: u32, what: u32, wx: f32, wy: f32, wave: f32) -> f32 {
    let (u, v) = (wx / wave, wy / wave);
    let (i, j) = (floor_to_i32(u), floor_to_i32(v));
    // LINT-OK: lattice indices, far inside f32's exact range.
    #[allow(clippy::cast_precision_loss)]
    let (fu, fv) = (u - i as f32, v - j as f32);
    let (su, sv) = (fu * fu * (3.0 - 2.0 * fu), fv * fv * (3.0 - 2.0 * fv));
    let at = |di: i32, dj: i32| noise(kind, i + di, j + dj, what);
    let low = at(0, 0) + (at(1, 0) - at(0, 0)) * su;
    let high = at(0, 1) + (at(1, 1) - at(0, 1)) * su;
    low + (high - low) * sv
}

/// How much of the ground about each of the block's vertices `kind` grows on, `0..=1`: a vertex's
/// own type softened by its neighbours'. A vertex on the block's edge is softened only along the
/// edge, by the vertices its neighbour block shares, so both blocks agree on it.
fn ground_share(
    land: &dyn CoverLand,
    kind: &CoverKind,
) -> [[f32; SIDE_VERTEX_COUNT]; SIDE_VERTEX_COUNT] {
    let n = SIDE_VERTEX_COUNT;
    let mut own = [[0.0f32; SIDE_VERTEX_COUNT]; SIDE_VERTEX_COUNT];
    for (a, row) in own.iter_mut().enumerate() {
        for (b, v) in row.iter_mut().enumerate() {
            let word = land.word(a, b);
            *v = if kind.grounds & (1 << ((word >> 2) & 0x1F)) != 0 {
                1.0
            } else {
                0.0
            };
        }
    }
    // Across the edge a vertex has no neighbour this block can read.
    let soft = |c: usize| -> &'static [(isize, f32)] {
        if c == 0 || c == n - 1 {
            &[(0, 1.0)]
        } else {
            &[(-1, 0.25), (0, 0.5), (1, 0.25)]
        }
    };
    let mut out = own;
    for (a, row) in out.iter_mut().enumerate() {
        for (b, v) in row.iter_mut().enumerate() {
            let mut sum = 0.0;
            for &(da, wa) in soft(a) {
                for &(db, wb) in soft(b) {
                    sum += wa * wb * own[a.wrapping_add_signed(da)][b.wrapping_add_signed(db)];
                }
            }
            *v = sum;
        }
    }
    out
}

/// `share` read at block-local `(x, y)`, between the vertices about it.
fn share_at(share: &[[f32; SIDE_VERTEX_COUNT]; SIDE_VERTEX_COUNT], x: f32, y: f32) -> f32 {
    let split = |at: f32| -> (usize, f32) {
        let cell = floor_to_i32(at / CELL_SIZE).clamp(0, 7);
        // LINT-OK: a cell index, 0..=7.
        #[allow(clippy::cast_precision_loss)]
        let into = (at / CELL_SIZE - cell as f32).clamp(0.0, 1.0);
        // LINT-OK: clamped to 0..=7 above.
        #[allow(clippy::cast_sign_loss)]
        let index = cell as usize;
        (index, into)
    };
    let ((a, fa), (b, fb)) = (split(x), split(y));
    let low = share[a][b] + (share[a + 1][b] - share[a][b]) * fa;
    let high = share[a][b + 1] + (share[a + 1][b + 1] - share[a][b + 1]) * fa;
    low + (high - low) * fb
}

/// The plants of block `(block_x, block_y)` on `land`, of every kind in `kinds`.
#[must_use]
pub fn scatter(
    land: &dyn CoverLand,
    block_x: i32,
    block_y: i32,
    kinds: &[CoverKind],
) -> Vec<CoverPlant> {
    let mut out = Vec::new();
    for kind in kinds {
        if kind.density.is_nan() || kind.density <= 0.0 {
            continue;
        }
        // LINT-OK: a grid's squares a side of a block, a small positive number.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let side = (BLOCK_LENGTH * kind.density.sqrt()).floor().max(1.0) as i32;
        // LINT-OK: a grid's squares a side of a block, far inside f32's exact range.
        #[allow(clippy::cast_precision_loss)]
        let step = BLOCK_LENGTH / side as f32;
        let least_normal = math::cosf(kind.steepest.to_radians());
        let grounds = ground_share(land, kind);
        // LINT-OK: block coordinates, far inside f32's exact range.
        #[allow(clippy::cast_precision_loss)]
        let corner = (block_x as f32 * BLOCK_LENGTH, block_y as f32 * BLOCK_LENGTH);
        for ix in 0..side {
            for iy in 0..side {
                let (gx, gy) = (block_x * side + ix, block_y * side + iy);
                // LINT-OK: grid indices, far inside f32's exact range.
                #[allow(clippy::cast_precision_loss)]
                let x = (ix as f32 + noise(kind.id, gx, gy, 0)) * step;
                // LINT-OK: as above.
                #[allow(clippy::cast_precision_loss)]
                let y = (iy as f32 + noise(kind.id, gx, gy, 1)) * step;
                let (wx, wy) = (corner.0 + x, corner.1 + y);
                // The wobble dies away at the block's edge, where both blocks read the same line.
                let reach =
                    WANDER * smoothstep(0.0, CELL_SIZE, x.min(y).min(BLOCK_LENGTH - x.max(y)));
                let read =
                    |what: u32| (drift(kind.id, what, wx, wy, WANDER_WAVE) * 2.0 - 1.0) * reach;
                let (sx, sy) = (
                    (x + read(6)).clamp(0.0, BLOCK_LENGTH),
                    (y + read(7)).clamp(0.0, BLOCK_LENGTH),
                );
                let keep = noise(kind.id, gx, gy, 4);
                let mut share = share_at(&grounds, sx, sy);
                if keep >= share || land.road(x, y) {
                    continue;
                }
                share *= smoothstep(BARE, BARE + THICKENS, land.clearing(x, y));
                if keep >= share {
                    continue;
                }
                let Some((z, normal)) = land.surface(x, y) else {
                    continue;
                };
                if normal < least_normal {
                    continue;
                }
                let grow = 0.5 * (noise(kind.id, gx, gy, 2) + drift(kind.id, 8, wx, wy, SWATHE));
                let full = kind.scale.0 + (kind.scale.1 - kind.scale.0) * grow;
                out.push(CoverPlant {
                    id: kind.id,
                    at: Vec3::new(x, y, z),
                    turn: noise(kind.id, gx, gy, 3) * std::f32::consts::TAU,
                    scale: full * (THIN_SIZE + (1.0 - THIN_SIZE) * share),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    //! Behaviour: none (experimental hi-fi art packs)
    use super::*;

    /// Level ground at height 10: `west` terrain west of the middle and `east` east of it.
    struct Land {
        west: u16,
        east: u16,
        normal: f32,
        road: bool,
        clearing: f32,
    }

    impl Land {
        fn of(ground: u16) -> Self {
            Self {
                west: ground,
                east: ground,
                normal: 1.0,
                road: false,
                clearing: f32::INFINITY,
            }
        }
    }

    impl CoverLand for Land {
        fn word(&self, a: usize, _b: usize) -> u16 {
            (if a < 4 { self.west } else { self.east }) << 2
        }
        fn road(&self, _x: f32, _y: f32) -> bool {
            self.road
        }
        fn clearing(&self, _x: f32, _y: f32) -> f32 {
            self.clearing
        }
        fn surface(&self, _x: f32, _y: f32) -> Option<(f32, f32)> {
            Some((10.0, self.normal))
        }
    }

    const GRASS: CoverKind = CoverKind {
        id: 7,
        grounds: 1 << 3,
        density: 0.25,
        scale: (0.5, 2.0),
        steepest: 30.0,
    };

    #[test]
    fn a_kind_grows_as_densely_as_it_asks_on_its_own_ground_and_stands_on_the_ground() {
        let plants = scatter(&Land::of(3), 100, 100, &[GRASS]);
        // 96 squares a side at one plant in four square metres.
        assert_eq!(plants.len(), 96 * 96);
        assert!(plants.iter().all(|p| p.id == 7
            && p.at.z == 10.0
            && (0.0..BLOCK_LENGTH).contains(&p.at.x)
            && (0.0..BLOCK_LENGTH).contains(&p.at.y)
            && (0.5..=2.0).contains(&p.scale)));
    }

    #[test]
    fn nothing_grows_on_other_ground_on_roads_against_walls_or_on_steep_slopes() {
        assert!(scatter(&Land::of(4), 100, 100, &[GRASS]).is_empty());
        let road = Land {
            road: true,
            ..Land::of(3)
        };
        assert!(scatter(&road, 100, 100, &[GRASS]).is_empty());
        let walls = Land {
            clearing: BARE,
            ..Land::of(3)
        };
        assert!(scatter(&walls, 100, 100, &[GRASS]).is_empty());
        let steep = Land {
            normal: 0.8,
            ..Land::of(3)
        };
        assert!(scatter(&steep, 100, 100, &[GRASS]).is_empty());
    }

    #[test]
    fn a_block_grows_the_same_plants_whenever_it_is_worked_out_and_its_neighbour_others() {
        let land = Land::of(3);
        let a = scatter(&land, 100, 100, &[GRASS]);
        assert_eq!(a, scatter(&land, 100, 100, &[GRASS]));
        assert_ne!(a[0].at, scatter(&land, 101, 100, &[GRASS])[0].at);
    }

    #[test]
    fn the_plants_are_not_laid_in_rows() {
        let plants = scatter(&Land::of(3), 100, 100, &[GRASS]);
        // Within its square, each plant lies anywhere: the mean offset is near the middle and
        // its spread near a uniform number's.
        let offsets: Vec<f32> = plants.iter().map(|p| (p.at.x / 2.0).fract()).collect();
        // LINT-OK: a count of plants, far inside f32's exact range.
        #[allow(clippy::cast_precision_loss)]
        let n = offsets.len() as f32;
        let mean = offsets.iter().sum::<f32>() / n;
        let spread = offsets.iter().map(|o| (o - mean).powi(2)).sum::<f32>() / n;
        assert!((mean - 0.5).abs() < 0.03, "{mean}");
        assert!((spread - 1.0 / 12.0).abs() < 0.01, "{spread}");
    }

    #[test]
    fn where_two_grounds_meet_a_kind_thins_out_over_two_cells_and_shrinks_as_it_goes() {
        let land = Land {
            west: 3,
            east: 4,
            ..Land::of(3)
        };
        let plants = scatter(&land, 100, 100, &[GRASS]);
        // The word changes between vertices 3 and 4. Softened by their neighbours, vertex 3 is
        // three quarters grass and vertex 4 a quarter, so the plants thin from 48 m to 120 m, give
        // or take the wander.
        assert!(plants.iter().all(|p| p.at.x < CELL_SIZE * 5.0 + WANDER));
        let band = |lo: f32, hi: f32| plants.iter().filter(move |p| (lo..hi).contains(&p.at.x));
        let counts: Vec<usize> = (0..6)
            .map(|k| {
                // LINT-OK: a band index, 0..6.
                #[allow(clippy::cast_precision_loss)]
                let lo = 24.0 + 16.0 * k as f32;
                band(lo, lo + 16.0).count()
            })
            .collect();
        assert!(counts.windows(2).all(|w| w[1] <= w[0]), "{counts:?}");
        assert!(counts[5] > 0 && counts[5] < counts[0] / 4, "{counts:?}");
        let mean = |lo: f32, hi: f32| {
            let (n, sum) = band(lo, hi).fold((0.0f32, 0.0), |(n, s), p| (n + 1.0, s + p.scale));
            sum / n
        };
        assert!(mean(96.0, 120.0) < mean(24.0, 48.0) * 0.85);
    }

    #[test]
    fn a_ground_softened_along_a_blocks_edge_agrees_with_its_neighbours() {
        // A vertex on the west edge sees only the edge's own vertices, as the block west of it
        // does on its east edge.
        struct Spot(u16);
        impl CoverLand for Spot {
            fn word(&self, a: usize, b: usize) -> u16 {
                (if (a, b) == (0, 4) || (a, b) == (1, 4) {
                    4
                } else {
                    self.0
                }) << 2
            }
            fn road(&self, _x: f32, _y: f32) -> bool {
                false
            }
            fn clearing(&self, _x: f32, _y: f32) -> f32 {
                f32::INFINITY
            }
            fn surface(&self, _x: f32, _y: f32) -> Option<(f32, f32)> {
                Some((0.0, 1.0))
            }
        }
        let share = ground_share(&Spot(3), &GRASS);
        assert_eq!(share[0][4], 0.5);
        assert_eq!(share[0][3], 0.75);
        assert_eq!(share[0][0], 1.0);
        // Inside, the bare vertex is softened by all eight about it.
        assert_eq!(share[1][4], 0.625);
    }

    #[test]
    fn plants_thin_toward_a_buildings_walls() {
        let at = |clearing: f32| {
            let land = Land {
                clearing,
                ..Land::of(3)
            };
            scatter(&land, 100, 100, &[GRASS]).len()
        };
        let (bare, near, mid, far) = (
            at(BARE),
            at(BARE + 2.0),
            at(BARE + 5.0),
            at(BARE + THICKENS),
        );
        assert_eq!(bare, 0);
        assert!(0 < near && near < mid && mid < far, "{near} {mid} {far}");
        assert_eq!(far, 96 * 96);
    }

    #[test]
    fn a_kind_with_no_density_grows_nothing() {
        let none = CoverKind {
            density: 0.0,
            ..GRASS
        };
        assert!(scatter(&Land::of(3), 100, 100, &[none]).is_empty());
    }
}
