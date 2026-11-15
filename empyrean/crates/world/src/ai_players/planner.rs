//! Routes for AI Players: the shared route planner (`dereth-navigation`, the client's own) on a few
//! threads of their own, so planning never holds up the world. Each area of 4 by 4 landblocks is
//! always planned by the same thread, so no area's map is built twice.
//!
//! The planner reads the land, buildings and setups from its own copies of the dat sources. The
//! outdoor placements (landblock objects and scenery) are the server's, computed on the world
//! thread and sent with the first request that needs each landblock.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use dereth_navigation::{LandNavSource, NavConfig, NavWorld, StaticPlacement};
use dereth_primitives::{CellId, DataId, Frame, LandblockId, Position as NavPos, Quat, Vec3};
use empyrean_entity::Position;

use crate::physics::phys_ext::{self, DatStaticKind};
use crate::World;

/// A route asked for: from, to, and how far short of `to` it may end.
struct Request {
    bot: u32,
    from: NavPos,
    to: NavPos,
    reach: f32,
    statics: Vec<(u16, Vec<StaticPlacement>)>,
}

/// A route found: its corners after the start, in world metres (x east, y north, z up).
#[derive(Debug)]
pub struct Answer {
    pub bot: u32,
    /// The corners, and whether the route was cut short at a building's door.
    pub route: Result<(Vec<[f32; 3]>, bool), String>,
    /// The planner's seconds on it.
    pub took: f64,
}

/// The world's half of the planner.
pub struct Planner {
    /// Each thread's requests, and the landblocks whose placements it has.
    requests: Vec<Sender<Request>>,
    sent: Vec<HashSet<u16>>,
    answers: Receiver<Answer>,
    /// Routes asked for and not yet answered.
    outstanding: usize,
    /// Answers taken by [`Planner::wait`], not yet handed out.
    ready: Vec<Answer>,
    /// Routes answered, and the planner's seconds spent on them.
    pub answered: u64,
    pub busy_seconds: f64,
}

impl std::fmt::Debug for Planner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Planner")
            .field("threads", &self.requests.len())
            .finish_non_exhaustive()
    }
}

type Placements = Arc<Mutex<HashMap<u16, Vec<StaticPlacement>>>>;

/// How many planner threads: half the machine's cores, from one to four.
fn threads() -> usize {
    std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .div_ceil(2)
        .clamp(1, 4)
}

impl Planner {
    /// Starts the planner threads over the world's dats.
    #[must_use]
    pub fn start(w: &World) -> Self {
        let (outbox, answers) = channel::<Answer>();
        let mut requests = Vec::new();
        for i in 0..threads() {
            let (tx, inbox) = channel::<Request>();
            requests.push(tx);
            spawn(w, i, inbox, outbox.clone());
        }
        let n = requests.len();
        Self {
            requests,
            sent: vec![HashSet::new(); n],
            answers,
            outstanding: 0,
            ready: Vec::new(),
            answered: 0,
            busy_seconds: 0.0,
        }
    }
}

/// One planner thread, answering `inbox` into `outbox`.
fn spawn(w: &World, i: usize, inbox: Receiver<Request>, outbox: Sender<Answer>) {
    {
        let dats = Arc::clone(&w.dats);
        let land = phys_ext::dat_land_source(w);
        std::thread::Builder::new()
            .name(format!("ai-players-planner-{i}"))
            .spawn(move || {
                let land = land.unwrap_or_else(|| {
                    Arc::new(
                        empyrean_dat::physics::DatLandSource::new(Arc::clone(&dats))
                            .unwrap_or_else(|_| {
                                empyrean_dat::physics::DatLandSource::with_height_table(
                                    Arc::clone(&dats),
                                    empyrean_dat::physics::linear_height_table(),
                                )
                            }),
                    )
                });
                let setups = empyrean_dat::physics::SetupGeometryCache::new(Arc::clone(&dats));
                let placed: Placements = Arc::default();
                let statics = Arc::clone(&placed);
                let mut nav = NavWorld::new(
                    LandNavSource {
                        land,
                        setups: move |id: DataId| setups.get(id.0),
                        land_statics: move |b: LandblockId| {
                            statics
                                .lock()
                                .ok()
                                .and_then(|m| m.get(&b.0).cloned())
                                .unwrap_or_default()
                        },
                    },
                    NavConfig::default(),
                );
                while let Ok(r) = inbox.recv() {
                    if let Ok(mut m) = placed.lock() {
                        m.extend(r.statics);
                    }
                    let started = std::time::Instant::now();
                    let route = nav
                        .plan_near(&r.from, &r.to, r.reach)
                        .map_err(|e| format!("{e:?}"))
                        .and_then(|path| {
                            // AI Players stay outdoors: a route into a building ends at its door.
                            let mut out = Vec::new();
                            let mut door = false;
                            for p in path.waypoints.iter().skip(1) {
                                if p.position.cell.0 & 0xFFFF >= 0x100 {
                                    door = true;
                                    break;
                                }
                                let g = dereth_navigation::global(&p.position);
                                out.push([g.x, g.y, g.z]);
                            }
                            if door && out.is_empty() {
                                return Err("the route goes indoors".to_owned());
                            }
                            Ok((out, door))
                        });
                    log::debug!(
                        "AI Players: route for {:#x} in {:?}: {:?}",
                        r.bot,
                        started.elapsed(),
                        route.as_ref().map(|r| r.0.len())
                    );
                    let took = started.elapsed().as_secs_f64();
                    if outbox
                        .send(Answer {
                            bot: r.bot,
                            route,
                            took,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .expect("the planner thread starts");
    }
}

impl Planner {
    /// Asks for a route for AI Player `bot` from `from` to within `reach` of `to`.
    pub fn ask(w: &mut World, bot: u32, from: &Position, to: &Position, reach: f32) {
        if w.ai_players.planner.is_none() {
            w.ai_players.planner = Some(Self::start(w));
        }
        let (from, to) = (nav_pos(from), nav_pos(to));
        let n = w
            .ai_players
            .planner
            .as_ref()
            .map_or(1, |p| p.requests.len());
        let lb = from.cell.0 >> 16;
        let area = ((lb >> 8) / 4) * 64 + (lb & 0xFF) / 4;
        let thread = usize::try_from(area).unwrap_or(0) % n;
        let mut statics = Vec::new();
        for b in blocks_between(&from, &to) {
            let known = w
                .ai_players
                .planner
                .as_ref()
                .is_some_and(|p| p.sent[thread].contains(&b));
            if !known {
                statics.push((b, outdoor_placements(w, b)));
            }
        }
        let Some(p) = w.ai_players.planner.as_mut() else {
            return;
        };
        for (b, _) in &statics {
            p.sent[thread].insert(*b);
        }
        p.outstanding += 1;
        let _ = p.requests[thread].send(Request {
            bot,
            from,
            to,
            reach,
            statics,
        });
    }

    /// The routes found since the last call.
    pub fn answers(w: &mut World) -> Vec<Answer> {
        let Some(p) = w.ai_players.planner.as_mut() else {
            return Vec::new();
        };
        let mut out = std::mem::take(&mut p.ready);
        out.extend(p.answers.try_iter());
        p.outstanding = p.outstanding.saturating_sub(out.len());
        p.answered += out.len() as u64;
        p.busy_seconds += out.iter().map(|a| a.took).sum::<f64>();
        out
    }

    /// Routes asked for and not yet answered, routes answered, and the planner's seconds on them.
    #[must_use]
    pub fn stats(w: &World) -> (usize, u64, f64) {
        w.ai_players
            .planner
            .as_ref()
            .map_or((0, 0, 0.0), |p| (p.outstanding, p.answered, p.busy_seconds))
    }

    /// Waits up to `limit` for every route asked for: for a test running on virtual time, whose
    /// seconds pass faster than the planner's.
    pub fn wait(w: &mut World, limit: std::time::Duration) {
        let Some(p) = w.ai_players.planner.as_mut() else {
            return;
        };
        let until = std::time::Instant::now() + limit;
        while p.ready.len() < p.outstanding {
            let left = until.saturating_duration_since(std::time::Instant::now());
            match p.answers.recv_timeout(left) {
                Ok(a) => p.ready.push(a),
                Err(_) => break,
            }
        }
    }
}

/// A server position as the planner's.
#[must_use]
pub fn nav_pos(p: &Position) -> NavPos {
    NavPos::new(
        CellId(p.landblock_id().raw()),
        Frame {
            origin: Vec3::new(p.position_x, p.position_y, p.position_z),
            rotation: Quat {
                w: p.rotation_w,
                x: p.rotation_x,
                y: p.rotation_y,
                z: p.rotation_z,
            },
        },
    )
}

/// The landblocks a route between `a` and `b` may cross: their box, one landblock wider.
fn blocks_between(a: &NavPos, b: &NavPos) -> Vec<u16> {
    let xy = |p: &NavPos| {
        let lb = p.cell.0 >> 16;
        (
            i32::try_from(lb >> 8).unwrap_or(0),
            i32::try_from(lb & 0xFF).unwrap_or(0),
        )
    };
    let ((ax, ay), (bx, by)) = (xy(a), xy(b));
    let mut out = Vec::new();
    for x in (ax.min(bx) - 1).max(0)..=(ax.max(bx) + 1).min(0xFF) {
        for y in (ay.min(by) - 1).max(0)..=(ay.max(by) + 1).min(0xFF) {
            out.push(u16::try_from((x << 8) | y).unwrap_or(0));
        }
    }
    out
}

/// What stands outdoors in landblock `b`, as the server places it: its objects and scenery.
fn outdoor_placements(w: &World, b: u16) -> Vec<StaticPlacement> {
    phys_ext::dat_static_placements(w, b)
        .into_iter()
        .filter(|(kind, _, _)| *kind != DatStaticKind::Interior)
        .map(|(_, s, scale)| StaticPlacement {
            id: s.id,
            frame: s.frame,
            scale,
        })
        .collect()
}
