//! AI Players walk as a client does: the motion (`MoveToState`) when they start, stop or sit,
//! and their position (`AutonomousPosition`) as they go, each through the server's own handlers,
//! so the server checks and broadcasts the movement as it does a client's.

use dereth_protocol::movement::{
    AutonomousPosition, MoveTimestamps, MoveToStatePack, MovementAutonomousPosition,
    MovementMoveToState, RawMotionState,
};
use dereth_protocol::types::space::{Frame, PositionWire, Quat, Vec3};
use dereth_protocol::Message;
use empyrean_entity::{LandblockId, ObjectGuid, Position};
use empyrean_net::ClientMessage;

use super::planner::Planner;
use crate::entity::position_extensions;
use crate::network::managers::inbound_message_manager;
use crate::World;
use dereth_primitives::num::math as m;

/// `MotionCommand.WalkForward` (run with the run key held), `Sitting`, and the hold keys.
const WALK_FORWARD: u32 = 0x4500_0005;
const SITTING: u32 = 0x4100_0013;
const HOLD_NONE: u32 = 1;
const HOLD_RUN: u32 = 2;

/// Seconds between position steps.
const STEP: f64 = 0.5;
/// The longest route planned at once; a longer trip goes in hops.
const HOP: f32 = 180.0;
/// Close enough to a corner to turn for the next.
const ARRIVE: f32 = 1.0;
/// Seconds without a metre of progress that count as stuck.
const STUCK_AFTER: f64 = 3.0;
/// Routes planned again before a walk is given up.
const REPLANS: u32 = 3;
/// `MotionInterp.RunAnimSpeed`: metres a second at a run rate of 1.
const RUN_ANIM_SPEED: f32 = 4.0;

/// How a walk ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Arrived,
    Failed,
}

/// A walk under way.
#[derive(Debug, Clone)]
pub struct Walk {
    /// Where to, in world metres, and how close counts.
    pub to: [f32; 2],
    pub reach: f32,
    /// The current hop's corners, once planned.
    route: Option<Vec<[f32; 3]>>,
    next: usize,
    moving: bool,
    next_step: f64,
    check_at: f64,
    check_from: [f32; 2],
    replans: u32,
    pub outcome: Option<Outcome>,
}

/// A position in world metres: x east, y north, z up.
#[must_use]
pub fn global(p: &Position) -> [f32; 3] {
    let raw = p.landblock_id().raw();
    #[allow(clippy::cast_precision_loss)]
    let block = |n: u32| (n & 0xFF) as f32 * 192.0;
    [
        block(raw >> 24) + p.position_x,
        block(raw >> 16) + p.position_y,
        p.position_z,
    ]
}

/// The outdoor position at world metres `x`, `y`, on the ground, facing `heading` (radians,
/// clockwise from north).
#[must_use]
pub fn ground_at(w: &World, x: f32, y: f32, heading: f32) -> Position {
    let max = 256.0 * 192.0 - 0.01;
    let (x, y) = (x.clamp(0.0, max), y.clamp(0.0, max));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (bx, by) = ((x / 192.0) as u32, (y / 192.0) as u32);
    #[allow(clippy::cast_precision_loss)]
    let (lx, ly) = (x - bx as f32 * 192.0, y - by as f32 * 192.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let cell = ((lx / 24.0) as u32).min(7) * 8 + ((ly / 24.0) as u32).min(7) + 1;
    let mut p = Position::new();
    p.set_landblock_id(LandblockId::new(bx << 24 | by << 16 | cell));
    p.position_x = lx;
    p.position_y = ly;
    p.position_z = 0.0;
    p.position_z = position_extensions::get_terrain_z(w, &p);
    // A turn of `heading` clockwise about the up axis.
    let half = -heading / 2.0;
    p.rotation_w = m::cosf(half);
    p.rotation_x = 0.0;
    p.rotation_y = 0.0;
    p.rotation_z = m::sinf(half);
    p
}

fn wire(p: &Position) -> PositionWire {
    PositionWire {
        objcell_id: p.cell(),
        frame: Frame {
            origin: Vec3 {
                x: p.position_x,
                y: p.position_y,
                z: p.position_z,
            },
            orientation: Quat {
                w: p.rotation_w,
                x: p.rotation_x,
                y: p.rotation_y,
                z: p.rotation_z,
            },
        },
    }
}

/// Sends `m` as AI Player `id`'s client would, through the server's own handler.
pub fn send<M: Message>(w: &mut World, id: u32, m: &M) {
    let Some(bot) = w.ai_players.bots.get_mut(&id) else {
        return;
    };
    bot.stamp = bot.stamp.wrapping_add(1);
    let (stamp, session) = (bot.stamp, bot.session);
    let Ok(data) = dereth_protocol::actions::pack_action(stamp, m) else {
        return;
    };
    if let Some(message) = ClientMessage::new(data) {
        inbound_message_manager::handle_client_message(w, message, session);
    }
}

fn motion(at: &Position, raw_motion_state: RawMotionState) -> MovementMoveToState {
    MovementMoveToState(MoveToStatePack {
        raw_motion_state,
        position: wire(at),
        timestamps: MoveTimestamps::default(),
        contact: true,
        longjump_mode: false,
    })
}

fn run(w: &mut World, id: u32, at: &Position) {
    let m = motion(
        at,
        RawMotionState {
            current_holdkey: Some(HOLD_RUN),
            forward_command: Some(WALK_FORWARD),
            forward_holdkey: Some(HOLD_RUN),
            ..RawMotionState::default()
        },
    );
    send(w, id, &m);
}

fn halt(w: &mut World, id: u32, at: &Position) {
    let m = motion(
        at,
        RawMotionState {
            current_holdkey: Some(HOLD_NONE),
            ..RawMotionState::default()
        },
    );
    send(w, id, &m);
}

fn moved(w: &mut World, id: u32, at: &Position) {
    let m = MovementAutonomousPosition(AutonomousPosition {
        position: wire(at),
        timestamps: MoveTimestamps::default(),
        contact: 1,
    });
    send(w, id, &m);
}

/// AI Player `id` sits down where it stands.
pub fn sit(w: &mut World, id: u32, p: ObjectGuid) {
    let Some(at) = w.objects.get(p).and_then(|o| o.location()) else {
        return;
    };
    let m = motion(
        &at,
        RawMotionState {
            current_holdkey: Some(HOLD_NONE),
            forward_command: Some(SITTING),
            ..RawMotionState::default()
        },
    );
    send(w, id, &m);
}

/// AI Player `id` stands still (up, if it was sitting).
pub fn stand(w: &mut World, id: u32, p: ObjectGuid) {
    if let Some(at) = w.objects.get(p).and_then(|o| o.location()) {
        halt(w, id, &at);
    }
}

/// AI Player `id` sets off for world metres `to`, to within `reach`.
pub fn go(w: &mut World, id: u32, to: [f32; 2], reach: f32) {
    let Some(bot) = w.ai_players.bots.get_mut(&id) else {
        return;
    };
    bot.walk = Some(Walk {
        to,
        reach,
        route: None,
        next: 0,
        moving: false,
        next_step: 0.0,
        check_at: 0.0,
        check_from: [0.0, 0.0],
        replans: 0,
        outcome: None,
    });
    plan(w, id);
}

/// Plans AI Player `id`'s next hop from where it stands.
fn plan(w: &mut World, id: u32) {
    let Some(p) = w
        .ai_players
        .bots
        .get(&id)
        .and_then(|b| w.sessions.player(b.session))
    else {
        return;
    };
    let Some(walk) = w.ai_players.bots.get(&id).and_then(|b| b.walk.clone()) else {
        return;
    };
    let Some(from) = w.objects.get(p).and_then(|o| o.location()) else {
        return;
    };
    let here = global(&from);
    let (dx, dy) = (walk.to[0] - here[0], walk.to[1] - here[1]);
    let d = m::hypotf(dx, dy);
    let (target, reach) = if d > HOP {
        let k = HOP / d;
        ([here[0] + dx * k, here[1] + dy * k], 20.0)
    } else {
        (walk.to, walk.reach)
    };
    let to = ground_at(w, target[0], target[1], 0.0);
    Planner::ask(w, id, &from, &to, reach);
}

/// Takes the planner's answers.
pub fn take_routes(w: &mut World) {
    for answer in Planner::answers(w) {
        let Some(walk) = w
            .ai_players
            .bots
            .get_mut(&answer.bot)
            .and_then(|b| b.walk.as_mut())
        else {
            continue;
        };
        match answer.route {
            Ok(route) if !route.is_empty() => {
                walk.route = Some(route);
                walk.next = 0;
            }
            Ok(_) => walk.outcome = Some(Outcome::Arrived),
            Err(e) => {
                log::debug!("AI Players: no route for {:#x}: {e}", answer.bot);
                walk.outcome = Some(Outcome::Failed);
            }
        }
    }
}

/// One step of AI Player `id`'s walk, when one is due.
pub fn step(w: &mut World, id: u32, p: ObjectGuid, now: f64) {
    let Some(mut walk) = w.ai_players.bots.get(&id).and_then(|b| b.walk.clone()) else {
        return;
    };
    if walk.outcome.is_some() || now < walk.next_step {
        return;
    }
    let Some(route) = walk.route.clone() else {
        return;
    };
    let Some(at) = w.objects.get(p).and_then(|o| o.location()) else {
        return;
    };
    let here = global(&at);
    walk.next_step = now + STEP;

    // Past the corners reached.
    while walk.next < route.len() {
        let c = route[walk.next];
        if m::hypotf(c[0] - here[0], c[1] - here[1]) > ARRIVE {
            break;
        }
        walk.next += 1;
    }
    if walk.next >= route.len() {
        let done = m::hypotf(walk.to[0] - here[0], walk.to[1] - here[1]) <= walk.reach + ARRIVE;
        walk.route = None;
        if done {
            walk.outcome = Some(Outcome::Arrived);
            halt(w, id, &at);
            set(w, id, walk);
        } else {
            // The next hop.
            set(w, id, walk);
            plan(w, id);
        }
        return;
    }

    // Stuck: no metre gained for a while.
    if walk.check_at == 0.0 {
        walk.check_at = now;
        walk.check_from = [here[0], here[1]];
    } else if now - walk.check_at >= STUCK_AFTER {
        let gained = m::hypotf(here[0] - walk.check_from[0], here[1] - walk.check_from[1]);
        walk.check_at = now;
        walk.check_from = [here[0], here[1]];
        if gained < 1.0 {
            walk.replans += 1;
            walk.route = None;
            walk.moving = false;
            halt(w, id, &at);
            if walk.replans > REPLANS {
                walk.outcome = Some(Outcome::Failed);
                set(w, id, walk);
            } else {
                set(w, id, walk);
                plan(w, id);
            }
            return;
        }
    }

    let c = route[walk.next];
    let (dx, dy) = (c[0] - here[0], c[1] - here[1]);
    let d = m::hypotf(dx, dy);
    let speed = run_speed(w, p);
    #[allow(clippy::cast_possible_truncation)]
    let stride = (speed * STEP as f32).min(d);
    let heading = m::atan2f(dx, dy);
    let next = ground_at(
        w,
        here[0] + dx / d * stride,
        here[1] + dy / d * stride,
        heading,
    );
    if walk.moving {
        moved(w, id, &next);
    } else {
        walk.moving = true;
        run(w, id, &next);
    }
    set(w, id, walk);
}

fn set(w: &mut World, id: u32, walk: Walk) {
    if let Some(b) = w.ai_players.bots.get_mut(&id) {
        b.walk = Some(walk);
    }
}

/// How fast AI Player `p` runs, metres a second.
fn run_speed(w: &mut World, p: ObjectGuid) -> f32 {
    RUN_ANIM_SPEED * crate::world_objects::monster_navigation::get_run_rate(w, p).max(0.5)
}
