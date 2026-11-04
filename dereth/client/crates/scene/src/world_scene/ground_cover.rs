//! Ground cover, the scene's half: the plants an art pack lays over the land by terrain type,
//! worked out off the frame for the blocks near the viewer and handed to the presentation patch
//! by patch. They are pixels alone: nothing collides with them, nothing can pick them and the
//! server never hears of them.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, PoisonError};

use dereth_assets::region::Region;
use dereth_dat::RetailDatStore;
use dereth_primitives::frame::V3 as _;
use dereth_terrain::consts::CELL_SIZE;
use dereth_terrain::ground_cover::{scatter, CoverKind, RetailLand, CLEARING_REACH};
use dereth_terrain::land::drawn::BLOCK_METERS;
use dereth_terrain::scenery::outside_cell_index;

use super::drawn_ground::SceneryShift;
use super::far_trees::blocks_within;
use super::hifi_bridge::{CoverPatch, CoverPlacement};

/// One kind of plant, and what the patches take of its model.
#[derive(Debug, Clone, Copy)]
struct Kind {
    cover: CoverKind,
    /// How far its largest plant reaches from where it stands, meters.
    radius: f32,
    /// How far from the eye its largest plant is drawn, meters.
    reach: f32,
}

/// The kinds of plant `pack` lays, with the models that read.
fn kinds_of(pack: &crate::art_pack::ArtPack) -> Vec<Kind> {
    pack.ground_cover
        .iter()
        .filter_map(|c| {
            let m = pack.models.iter().find(|m| m.id == c.id)?;
            let last = m.lods.iter().rfind(|l| !l.shadows_only)?.distance;
            let centre = glam::Vec3::from(m.bounds.0).length();
            Some(Kind {
                cover: CoverKind {
                    id: c.id.0,
                    grounds: c.grounds,
                    density: c.density,
                    scale: c.scale,
                    steepest: c.steepest,
                },
                radius: (centre + m.bounds.1) * c.scale.1,
                reach: last * c.scale.1,
            })
        })
        .collect()
}

/// `plants` gathered into a patch for each outside cell they stand in; `measure` says, for a
/// plant's id and scale, how far it reaches from where it stands and how far from the eye it is
/// drawn.
pub(crate) fn patches_of(
    plants: Vec<CoverPlacement>,
    measure: &dyn Fn(u32, f32) -> (f32, f32),
) -> Vec<CoverPatch> {
    let mut cells: BTreeMap<u16, Vec<CoverPlacement>> = BTreeMap::new();
    for p in plants {
        cells
            .entry(outside_cell_index(p.origin.x, p.origin.y))
            .or_default()
            .push(p);
    }
    cells
        .into_values()
        .map(|plants| {
            let mut radius = 0.0f32;
            let mut reach = 0.0f32;
            let mut low = f32::MAX;
            let mut high = f32::MIN;
            for p in &plants {
                low = low.min(p.origin.z);
                high = high.max(p.origin.z);
                let (r, far) = measure(p.id, p.scale);
                radius = radius.max(r);
                reach = reach.max(far);
            }
            let first = plants[0].origin;
            let half = CELL_SIZE * 0.5;
            let middle = |at: f32| {
                // LINT-OK: a cell index, far inside f32's exact range.
                #[allow(clippy::cast_precision_loss)]
                let cell = dereth_primitives::num::floor_to_i32(at / CELL_SIZE) as f32;
                (cell + 0.5) * CELL_SIZE
            };
            CoverPatch {
                centre: glam::Vec3::new(middle(first.x), middle(first.y), (low + high) * 0.5),
                radius: (2.0 * half * half + (high - low).powi(2) * 0.25).sqrt() + radius,
                reach,
                plants,
            }
        })
        .collect()
}

/// What a block's plants are worked out from: the same for every block.
struct Sources {
    store: Arc<RetailDatStore>,
    region: Arc<Region>,
    table: [f32; 256],
    kinds: Vec<Kind>,
    /// Each building model's walls as read, by its id: the low and high corners of the box its
    /// parts fill, seen from above in its own frame.
    walls: std::cell::RefCell<HashMap<u32, Option<(glam::Vec2, glam::Vec2)>>>,
}

/// The box building model `id`'s parts fill, seen from above in its own frame.
fn walls_of(
    store: &RetailDatStore,
    id: dereth_primitives::DataId,
) -> Option<(glam::Vec2, glam::Vec2)> {
    use dereth_primitives::frame::localtoglobal;
    let mut stats = dereth_world_data::setup::SetupPartStats::default();
    let g = dereth_world_data::env_cells::static_geometry(store, id, &mut stats)?;
    let mut low = glam::Vec2::splat(f32::MAX);
    let mut high = glam::Vec2::splat(f32::MIN);
    for part in &g.parts {
        let Some(b) = part.bound_box else {
            continue;
        };
        let s = part.default_scale;
        for k in 0..8 {
            let corner = dereth_primitives::Vec3::new(
                if k & 1 == 0 { b.min.x } else { b.max.x } * s.x,
                if k & 2 == 0 { b.min.y } else { b.max.y } * s.y,
                if k & 4 == 0 { b.min.z } else { b.max.z } * s.z,
            );
            let at = localtoglobal(&part.placement_frame, corner);
            low = low.min(glam::Vec2::new(at.x, at.y));
            high = high.max(glam::Vec2::new(at.x, at.y));
        }
    }
    (low.x <= high.x).then_some((low, high))
}

/// A building's walls in a block: where it stands, how it is turned, and its box.
struct Footprint {
    origin: dereth_primitives::Vec3,
    turn: dereth_primitives::frame::Mat3,
    low: glam::Vec2,
    high: glam::Vec2,
}

impl Footprint {
    /// How far block-local `(x, y)` lies outside the walls, metres.
    fn gap(&self, x: f32, y: f32) -> f32 {
        let from = dereth_primitives::Vec3::new(x, y, self.origin.z).sub(self.origin);
        let at = dereth_primitives::frame::globaltolocalvec(self.turn, from);
        let p = glam::Vec2::new(at.x, at.y);
        (self.low - p)
            .max(p - self.high)
            .max(glam::Vec2::ZERO)
            .length()
    }
}

/// The walls of the buildings near enough block `(bx, by)` to thin its plants: its own and its
/// neighbours', in its own coordinates.
fn footprints_near(src: &Sources, bx: i32, by: i32) -> Vec<Footprint> {
    use dereth_client_runtime::world_build::read_lbi;
    let mut out = Vec::new();
    for dx in -1..=1 {
        for dy in -1..=1 {
            let Some(info) = read_lbi(&src.store, bx + dx, by + dy) else {
                continue;
            };
            // LINT-OK: block offsets, -1..=1.
            #[allow(clippy::cast_precision_loss)]
            let shift = dereth_primitives::Vec3::new(
                dx as f32 * BLOCK_METERS,
                dy as f32 * BLOCK_METERS,
                0.0,
            );
            for b in &info.buildings {
                let Some((low, high)) = *src
                    .walls
                    .borrow_mut()
                    .entry(b.id.0)
                    .or_insert_with(|| walls_of(&src.store, b.id))
                else {
                    continue;
                };
                let origin = b.frame.origin.add(shift);
                let reach = low.abs().max(high.abs()).length() + CLEARING_REACH;
                let near = |at: f32| (-reach..BLOCK_METERS + reach).contains(&at);
                if near(origin.x) && near(origin.y) {
                    out.push(Footprint {
                        origin,
                        turn: dereth_primitives::frame::l2g(b.frame.rotation),
                        low,
                        high,
                    });
                }
            }
        }
    }
    out
}

/// Block `(bx, by)`'s plants, gathered into patches.
fn block_patches(src: &Sources, bx: i32, by: i32, shift: Option<&SceneryShift>) -> Vec<CoverPatch> {
    use dereth_client_runtime::world_build::read_landblock;
    let Some(lb) = read_landblock(&src.store, bx, by) else {
        return Vec::new();
    };
    let mesh = dereth_terrain::land::mesh::generate_landblock_with_table(
        &lb,
        &src.region,
        &src.table,
        bx,
        by,
        1,
        dereth_terrain::land::mesh::Direction::InViewerBlock,
    );
    if mesh.side_cell_count != 8 {
        return Vec::new();
    }
    let footprints = footprints_near(src, bx, by);
    let clearing = |x: f32, y: f32| {
        footprints
            .iter()
            .map(|f| f.gap(x, y))
            .fold(f32::INFINITY, f32::min)
    };
    let land = RetailLand {
        landblock: &lb,
        mesh: &mesh,
        clearing: &clearing,
    };
    let kinds: Vec<CoverKind> = src.kinds.iter().map(|k| k.cover).collect();
    let plants: Vec<CoverPlacement> = scatter(&land, bx, by, &kinds)
        .iter()
        .map(|p| CoverPlacement {
            id: p.id,
            origin: glam::Vec3::new(
                p.at.x,
                p.at.y,
                p.at.z + shift.map_or(0.0, |s| s.shift(p.at.x, p.at.y, p.at.z)),
            ),
            rotation: glam::Quat::from_rotation_z(p.turn),
            scale: p.scale,
        })
        .collect();
    patches_of(plants, &|id, _| {
        src.kinds
            .iter()
            .find(|k| k.cover.id == id)
            .map_or((0.0, 0.0), |k| (k.radius, k.reach))
    })
}

/// How a thread works out a block's patches, with what moves them onto the drawn ground.
pub(crate) type MakePatches =
    Box<dyn FnMut((i32, i32), Option<&SceneryShift>) -> Vec<CoverPatch> + Send>;

/// A block to work out and what moves its plants onto the drawn ground.
type Asked = ((i32, i32), Option<SceneryShift>);
/// The blocks still to work out, nearest first, and the signal that more are asked for.
type Queue = Arc<(Mutex<VecDeque<Asked>>, Condvar)>;

/// The thread that works the blocks out, and what it shares with the frame.
struct Worker {
    queue: Queue,
    done: mpsc::Receiver<((i32, i32), Vec<CoverPatch>)>,
    stop: Arc<AtomicBool>,
}

impl Worker {
    fn start(mut make: MakePatches) -> Option<Self> {
        let queue: Queue = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let (send, done) = mpsc::channel();
        let (q, halt) = (Arc::clone(&queue), Arc::clone(&stop));
        crate::workers::spawn("plants and distant trees", move || loop {
            let next = {
                let (lock, wake) = &*q;
                let mut list = lock.lock().unwrap_or_else(PoisonError::into_inner);
                while list.is_empty() && !halt.load(Ordering::Relaxed) {
                    list = wake.wait(list).unwrap_or_else(PoisonError::into_inner);
                }
                list.pop_front()
            };
            if halt.load(Ordering::Relaxed) {
                return;
            }
            let Some((block, shift)) = next else {
                continue;
            };
            let patches = make(block, shift.as_ref());
            if send.send((block, patches)).is_err() {
                return;
            }
        })
        .ok()?;
        Some(Self { queue, done, stop })
    }

    /// Work out `blocks` next, in their order, in place of whatever was still to do.
    fn ask(&self, blocks: Vec<Asked>) {
        let (lock, wake) = &*self.queue;
        let mut list = lock.lock().unwrap_or_else(PoisonError::into_inner);
        list.clear();
        list.extend(blocks);
        wake.notify_one();
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.queue.1.notify_all();
    }
}

/// The patches of the blocks near the viewer, worked out off the frame: the ground cover's, or
/// the distant trees'.
#[derive(Default)]
pub(crate) struct PatchField {
    worker: Option<Worker>,
    /// Each block's patches, once worked out.
    made: HashMap<(i32, i32), Arc<Vec<CoverPatch>>>,
    /// The blocks asked for and not yet done.
    asked: HashSet<(i32, i32)>,
    /// The blocks within reach, nearest first, and the viewer's block and the reach they were
    /// worked out for.
    wanted: Vec<(i32, i32)>,
    wanted_for: Option<((i32, i32), u32)>,
    /// What they were worked out for: a change throws them away.
    key: Option<(u64, u64)>,
    /// Bumped whenever the patches within reach change.
    pub(crate) serial: u64,
    /// Whether they are drawn this frame.
    pub(crate) active: bool,
}

impl std::fmt::Debug for PatchField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PatchField")
            .field("made", &self.made.len())
            .field("asked", &self.asked.len())
            .field("serial", &self.serial)
            .field("active", &self.active)
            .finish_non_exhaustive()
    }
}

impl PatchField {
    /// One frame's upkeep: `wanted`, what they are worked out for (`key`), the viewer's block, how
    /// far from the eye they are drawn, how to start working them out, and what moves a block's
    /// patches onto the drawn ground.
    pub(crate) fn step(
        &mut self,
        wanted: bool,
        key: (u64, u64),
        viewer: (i32, i32),
        reach: f32,
        start: &mut dyn FnMut() -> Option<MakePatches>,
        shift: &mut dyn FnMut((i32, i32)) -> Option<SceneryShift>,
    ) {
        if !wanted {
            if self.active || self.worker.is_some() {
                *self = Self {
                    serial: self.serial + 1,
                    ..Self::default()
                };
            }
            return;
        }
        self.active = true;
        if self.key != Some(key) || self.worker.is_none() {
            self.made.clear();
            self.asked.clear();
            self.wanted_for = None;
            self.serial += 1;
            self.worker = start().and_then(Worker::start);
            self.key = Some(key);
        }
        let Some(worker) = self.worker.as_ref() else {
            return;
        };
        let mut changed = false;
        while let Ok((block, patches)) = worker.done.try_recv() {
            self.asked.remove(&block);
            self.made.insert(block, Arc::new(patches));
            changed = true;
        }
        if self.wanted_for != Some((viewer, reach.to_bits())) {
            // LINT-OK: block coordinates, far inside f32's exact range.
            #[allow(clippy::cast_precision_loss)]
            let centre = (
                (viewer.0 as f32 + 0.5) * BLOCK_METERS,
                (viewer.1 as f32 + 0.5) * BLOCK_METERS,
            );
            // Out to the reach from anywhere in the viewer's block, and kept a ring past it.
            self.wanted = blocks_within(centre, reach + BLOCK_METERS * 0.75);
            self.wanted_for = Some((viewer, reach.to_bits()));
            let margin: HashSet<(i32, i32)> = blocks_within(centre, reach + BLOCK_METERS * 2.0)
                .into_iter()
                .collect();
            self.made.retain(|b, _| margin.contains(b));
            let todo: Vec<Asked> = self
                .wanted
                .iter()
                .copied()
                .filter(|b| !self.made.contains_key(b))
                .map(|b| (b, shift(b)))
                .collect();
            self.asked = todo.iter().map(|(b, _)| *b).collect();
            worker.ask(todo);
            changed = true;
        }
        if changed {
            self.serial += 1;
        }
    }

    /// The patches of the blocks within reach that are worked out, block by block.
    pub(crate) fn within(&self) -> impl Iterator<Item = ((i32, i32), &Arc<Vec<CoverPatch>>)> {
        self.wanted
            .iter()
            .filter_map(|b| self.made.get(b).map(|t| (*b, t)))
    }

    /// Wait until every block within reach is worked out: for a capture that draws them from its
    /// first frames.
    pub(crate) fn wait(&mut self) {
        let Some(worker) = self.worker.as_ref() else {
            return;
        };
        while !self.asked.is_empty() {
            match worker.done.recv_timeout(std::time::Duration::from_secs(30)) {
                Ok((block, patches)) => {
                    self.asked.remove(&block);
                    self.made.insert(block, Arc::new(patches));
                }
                Err(_) => break,
            }
        }
        self.serial += 1;
    }
}

/// The ground cover's state in the scene.
#[derive(Debug, Default)]
pub(crate) struct GroundCover {
    pub(crate) field: PatchField,
    /// The share of the plants' distance the player asked for, percent.
    pub(crate) share: u32,
}

impl GroundCover {
    /// One frame's upkeep. `wanted` when the ground cover is drawn at all; the art pack `pack`,
    /// the drawn ground's build `build`, and the viewer's block; `shift` says what moves a
    /// block's plants onto the drawn ground.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn step(
        &mut self,
        share: u32,
        wanted: bool,
        pack: Option<&Arc<crate::art_pack::ArtPack>>,
        build: u64,
        store: &Arc<RetailDatStore>,
        region: &dyn Fn() -> Arc<Region>,
        table: &[f32; 256],
        viewer: (i32, i32),
        shift: &mut dyn FnMut((i32, i32)) -> Option<SceneryShift>,
    ) {
        self.share = share;
        let pack = pack.filter(|p| wanted && !p.ground_cover.is_empty());
        let key = pack.map_or((0, 0), |p| (p.serial, build));
        let kinds = pack.map_or_else(Vec::new, |p| kinds_of(p));
        let whole = kinds.iter().map(|k| k.reach).fold(0.0, f32::max);
        // LINT-OK: a percentage, far inside f32's exact range.
        #[allow(clippy::cast_precision_loss)]
        let reach = whole * share as f32 / 100.0;
        let mut start = || -> Option<MakePatches> {
            if kinds.is_empty() {
                return None;
            }
            let src = Sources {
                store: Arc::clone(store),
                region: region(),
                table: *table,
                kinds: kinds.clone(),
                walls: std::cell::RefCell::default(),
            };
            Some(Box::new(move |(bx, by), shift| {
                block_patches(&src, bx, by, shift)
            }))
        };
        self.field
            .step(pack.is_some(), key, viewer, reach, &mut start, shift);
    }
}

#[cfg(test)]
mod tests {
    //! Behaviour: none (experimental hi-fi art packs)
    use super::*;

    fn plant(x: f32, y: f32, z: f32) -> CoverPlacement {
        CoverPlacement {
            id: 7,
            origin: glam::Vec3::new(x, y, z),
            rotation: glam::Quat::IDENTITY,
            scale: 1.0,
        }
    }

    #[test]
    fn plants_are_gathered_by_the_cell_they_stand_in_and_a_patch_holds_them_all() {
        let patches = patches_of(
            vec![
                plant(1.0, 1.0, 10.0),
                plant(23.0, 23.0, 14.0),
                plant(25.0, 1.0, 10.0),
            ],
            &|_, _| (2.0, 50.0),
        );
        assert_eq!(patches.len(), 2);
        assert_eq!(patches[0].plants.len() + patches[1].plants.len(), 3);
        let both = patches
            .iter()
            .find(|p| p.plants.len() == 2)
            .expect("a patch of two");
        assert_eq!(both.centre, glam::Vec3::new(12.0, 12.0, 12.0));
        assert_eq!(both.reach, 50.0);
        // The cell's corner lies inside the patch, with the plant's own reach beside it.
        assert!(
            both.radius >= (2.0f32 * 144.0 + 4.0).sqrt() + 2.0 - 1e-3,
            "{}",
            both.radius
        );
    }
}
