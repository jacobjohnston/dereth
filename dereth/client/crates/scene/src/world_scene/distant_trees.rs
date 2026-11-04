//! The trees past the blocks whose own objects stand in the world, while the player asks for an
//! art pack's trees farther than those blocks reach. They are retail's scenery, placed as retail
//! places it, of the trees the pack has models for, and they are pixels alone. Where a pack has
//! far pictures, the far trees draw to the horizon whatever this says.

use std::collections::HashMap;
use std::sync::Arc;

use dereth_assets::region::Region;
use dereth_dat::RetailDatStore;
use dereth_primitives::DataId;
use dereth_terrain::scenery::{generate_scenery, outside_cell_index, SceneryEnv, WithinBlockShape};

use super::drawn_ground::SceneryShift;
use super::ground_cover::{patches_of, MakePatches, PatchField};
use super::hifi_bridge::{CoverPatch, CoverPlacement, TREE_REACH};

/// What the thread reads once and keeps: each scene, and each object's shape for the scenery's
/// last test.
#[derive(Default)]
struct Reads {
    scenes: HashMap<DataId, Option<dereth_assets::world::Scene>>,
    shapes: HashMap<u32, Option<WithinBlockShape>>,
}

/// What a block's trees are worked out from: the same for every block.
struct Sources {
    store: Arc<RetailDatStore>,
    region: Arc<Region>,
    table: [f32; 256],
    /// The pack's trees: each retail object and how far its model reaches from where it stands.
    trees: HashMap<u32, f32>,
}

/// The trees of `pack`: its growth that stands in for a retail object, each with how far its
/// model reaches from where it stands.
fn trees_of(pack: &crate::art_pack::ArtPack) -> HashMap<u32, f32> {
    pack.models
        .iter()
        .filter(|m| m.foliage && !pack.ground_cover.iter().any(|c| c.id == m.id))
        .map(|m| (m.id.0, glam::Vec3::from(m.bounds.0).length() + m.bounds.1))
        .collect()
}

/// Block `(bx, by)`'s trees the pack has models for, gathered into patches.
fn block_patches(
    src: &Sources,
    reads: &mut Reads,
    (bx, by): (i32, i32),
    shift: Option<&SceneryShift>,
) -> Vec<CoverPatch> {
    use dereth_client_runtime::world_build::{read_landblock, read_lbi, read_scene};
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
    // A cell that owns a building grows no scenery.
    let mut owned = [false; 65];
    if let Some(info) = read_lbi(&src.store, bx, by) {
        for b in &info.buildings {
            owned[usize::from(outside_cell_index(b.frame.origin.x, b.frame.origin.y))] = true;
        }
    }
    let store = &src.store;
    let reads = std::cell::RefCell::new(reads);
    let env = SceneryEnv {
        scenes: &|id| {
            reads
                .borrow_mut()
                .scenes
                .entry(id)
                .or_insert_with(|| read_scene(store, id))
                .clone()
        },
        has_building: &|cell| owned.get(usize::from(cell)).copied().unwrap_or(false),
        shape: &|id| {
            reads
                .borrow_mut()
                .shapes
                .entry(id.0)
                .or_insert_with(|| dereth_client_runtime::models::within_block_shape(store, id))
                .clone()
        },
    };
    let placed = generate_scenery(&lb, &mesh, &src.region, bx, by, &env);
    let plants: Vec<CoverPlacement> = placed
        .iter()
        .filter(|p| src.trees.contains_key(&p.gfxobj.0))
        .map(|p| {
            let at = p.frame.origin;
            let q = p.frame.rotation;
            CoverPlacement {
                id: p.gfxobj.0,
                origin: glam::Vec3::new(
                    at.x,
                    at.y,
                    at.z + shift.map_or(0.0, |s| s.shift(at.x, at.y, at.z)),
                ),
                rotation: glam::Quat::from_xyzw(q.x, q.y, q.z, q.w),
                scale: p.scale,
            }
        })
        .collect();
    patches_of(plants, &|id, scale| {
        (
            src.trees.get(&id).copied().unwrap_or(0.0) * scale,
            TREE_REACH,
        )
    })
}

/// The distant trees' state in the scene.
#[derive(Debug, Default)]
pub(crate) struct DistantTrees {
    pub(crate) field: PatchField,
    /// The share of the trees' distance the player asked for, percent.
    pub(crate) share: u32,
}

impl DistantTrees {
    /// One frame's upkeep: wanted while the player asks for more than the whole distance.
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
        let pack = pack.filter(|_| wanted && share > 100);
        let key = pack.map_or((0, 0), |p| (p.serial, build));
        let reach = TREE_REACH * share_of(share);
        let mut start = || -> Option<MakePatches> {
            let trees = trees_of(pack?);
            if trees.is_empty() {
                return None;
            }
            let src = Sources {
                store: Arc::clone(store),
                region: region(),
                table: *table,
                trees,
            };
            let mut reads = Reads::default();
            Some(Box::new(move |block, shift| {
                block_patches(&src, &mut reads, block, shift)
            }))
        };
        self.field
            .step(pack.is_some(), key, viewer, reach, &mut start, shift);
    }
}

/// `share` percent as a share.
fn share_of(share: u32) -> f32 {
    // LINT-OK: a percentage, far inside f32's exact range.
    #[allow(clippy::cast_precision_loss)]
    let s = share as f32 / 100.0;
    s
}

#[cfg(test)]
mod tests {
    //! Behaviour: none (experimental hi-fi art packs)
    use super::*;

    #[test]
    fn a_share_is_its_percent_of_the_whole_distance() {
        assert_eq!(share_of(100), 1.0);
        assert_eq!(share_of(500), 5.0);
        assert_eq!(share_of(50), 0.5);
    }
}
