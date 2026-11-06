//! The water Better Lighting draws with an art pack's ground or its own box: it holds still while
//! the game's clock is held, its ripples lying where the clock puts them; it is drawn only with the
//! pack's ground or the Reflective Water box (never with Retail Look), and its knob turns it off.
//! Retail's water pictures' colors it is drawn with are worked out once for a ground, not again
//! every frame.
//!
//! The door stamps are tested here too, over the Reflective Water box's ground: a door behind a
//! hill or past a road does not show through the ground in front of it.
//!
//! The pack is made in the test: a flat ground of its own color for each dry terrain type and
//! none for the water, so the water's own color is retail's water picture's.
//!
//! The frames are drawn without the anti-aliasing, so a frame that does not change draws the
//! same pixels again: what changes, changes because something drew it differently.
//!
//! Fixture: the retail dats (`DERETH_TEST_DAT_DIR`) and a hardware `wgpu` device off screen.
//!
//! Behaviour: none (experimental hi-fi art packs)

#![cfg(all(gpu, feature = "hifi"))]

use std::collections::HashMap;
use std::sync::Arc;

use dereth_client_runtime::render_prefs::FidelityPreferences;
use dereth_render::device::{AdapterKind, Backend, DeviceConfig, Gpu};
use dereth_scene::art_pack::ArtPack;

use crate::instruments::hifi_stations::{capture, write_png, Clock, Place, Shot, Station};

/// The frame's size.
const SIZE: (u32, u32) = (960, 540);

/// A four-texel square PNG of one color.
fn flat_png(rgba: [u8; 4]) -> Vec<u8> {
    let mut png = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut png, 4, 4);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().expect("header");
        w.write_image_data(&rgba.repeat(16)).expect("data");
    }
    png
}

/// The pack: a flat ground of its own color for each dry terrain type, none for the water.
fn ground_pack() -> Arc<ArtPack> {
    let mut files: HashMap<String, Vec<u8>> = HashMap::new();
    let mut manifest = String::from("[pack]\nname = \"Grounds\"\nformat = 1\n");
    let credit = "author = \"a\"\ntool = \"t\"\nlicense = \"CC0-1.0\"\n";
    for t in (0..16u8).chain(21..32) {
        let name = format!("g{t}.png");
        let h = u32::from(t).wrapping_mul(0x9E37_79B9).rotate_left(7);
        let [a, b, c, _] = h.to_le_bytes();
        files.insert(
            name.clone(),
            flat_png([40 + a / 2, 60 + b / 2, 30 + c / 3, 255]),
        );
        manifest.push_str(&format!(
            "[[terrain]]\nground = {t}\nbase_color = \"{name}\"\nsize = 4\n{credit}"
        ));
    }
    Arc::new(
        ArtPack::open(&manifest, &mut |path| {
            files
                .get(path)
                .cloned()
                .ok_or_else(|| format!("{path}: missing"))
        })
        .expect("the pack opens"),
    )
}

/// The river below Holtburg from its south bank, at a standing eye's height, looking across the
/// water to the far bank: most of the frame below the horizon is water.
fn holtburg_shore() -> Station {
    Station {
        name: "holtburg-shore",
        why: "water near and far",
        place: Place::Free {
            block: 0xAAB5,
            x: 110.0,
            y: 20.0,
            eye: 2.5,
            yaw: 0.6,
            pitch: -0.12,
        },
        clock: Clock::Sunny(0.5),
        weather: false,
        land_radius: 2,
    }
}

/// The boxes every frame is drawn with: the light, the sun's shadows and the sky, without the
/// anti-aliasing.
const BOXES: &str = "Lighting=1,Shadows=1,Sky=1,NoAntiAliasing=1";

/// The same with the anti-aliasing, which draws each frame a fraction of a pixel off the last.
const JITTERED: &str = "Lighting=1,Shadows=1,Sky=1";

/// Holtburg from sixty meters up, north over the river and its marshes: water from below the
/// town out to a kilometer and more, its shores far off and its reflections of the far banks.
fn over_holtburg() -> Station {
    Station {
        name: "over-holtburg",
        why: "far water and its shores",
        place: Place::Free {
            block: 0xA9B3,
            x: 84.0,
            y: 139.0,
            eye: 60.0,
            yaw: 0.0,
            pitch: -0.36,
        },
        clock: Clock::Sunny(0.5),
        weather: false,
        land_radius: 2,
    }
}

/// `station` opened on a device of its own with `pack` and `boxes`, the ground the pack lays
/// drawn and settled.
fn opened(station: &Station, pack: &Arc<ArtPack>, boxes: &str) -> (Gpu, Shot) {
    opened_with(station, Some(pack), boxes)
}

/// `station` opened on a device of its own, with `pack` if any and `boxes`, settled.
fn opened_with(station: &Station, pack: Option<&Arc<ArtPack>>, boxes: &str) -> (Gpu, Shot) {
    let store = crate::common::dats();
    let cfg = DeviceConfig {
        width: SIZE.0,
        height: SIZE.1,
        hifi: true,
        ..DeviceConfig::default()
    };
    let mut device = Gpu::new_on(Backend::Wgpu, None, &cfg)
        .unwrap_or_else(|e| panic!("this test needs a wgpu device and none opened: {e}"));
    assert_eq!(
        device.adapter_kind(),
        AdapterKind::Hardware,
        "{} is a software rasteriser",
        device.adapter_name()
    );
    let gpu = &mut device;
    let mut shot = Shot::open(&store, gpu, station);
    if let Some(pack) = pack {
        shot.scene
            .set_art_pack(&store, gpu, Some(Arc::clone(pack)))
            .expect("the blocks bake for the pack");
    }
    set_boxes(&mut shot, gpu, boxes);
    (device, shot)
}

/// `boxes` ticked on an open `shot`, the ground they ask for drawn and settled.
fn set_boxes(shot: &mut Shot, gpu: &mut Gpu, boxes: &str) {
    shot.scene.draw.cfg.render.fidelity =
        FidelityPreferences::parse_switch(boxes).expect("the boxes parse");
    shot.poll(gpu);
    shot.step(gpu);
    shot.scene.wait_for_drawn_ground();
    shot.poll(gpu);
    for _ in 0..12 {
        shot.step(gpu);
        assert!(
            gpu.hifi_settle(std::time::Duration::from_secs(60)),
            "the presentation's pipelines were not built in a minute"
        );
    }
    shot.step(gpu);
    assert!(gpu.hifi_failed().is_none(), "{:?}", gpu.hifi_failed());
}

/// Where the frames go, in cargo's scratch folder, to look at when a test fails.
fn keep(name: &str, rgba: &[u8]) {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("hifi-water");
    std::fs::create_dir_all(&dir).expect("the scratch folder");
    write_png(&dir.join(format!("{name}.png")), SIZE.0, SIZE.1, rgba);
}

/// The pixels below the frame's top third (the land and the water, under the sky and its drifting
/// clouds) whose color differs by more than `levels` in any channel between `a` and `b`.
fn changed_below_the_sky(a: &[u8], b: &[u8], levels: u8) -> usize {
    let row = SIZE.0 as usize * 4;
    let from = SIZE.1 as usize / 3 * row;
    a[from..]
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b[from..].as_chunks::<4>().0)
        .filter(|(p, q)| (0..3).any(|c| p[c].abs_diff(q[c]) > levels))
        .count()
}

/// Whether the presentation drew the water in the last frame.
fn water_drawn(gpu: &Gpu) -> bool {
    gpu.hifi_report()
        .is_some_and(|r| r.notes.iter().any(|(n, v)| *n == "water drawn" && *v == 1))
}

#[test]
fn the_water_holds_still_while_the_clock_is_held_and_its_ripples_lie_where_the_clock_puts_them() {
    let _gpu = crate::common::gpu_lock();
    let pack = ground_pack();
    let (mut device, mut shot) = opened(&holtburg_shore(), &pack, BOXES);
    let gpu = &mut device;
    // Under retail's own sky, whose broken clouds the ripples show against as they move: the
    // Remastered sky's even cloud low over this water leaves half as much of them to see.
    let (knobs, notes) = dereth_terrain::land::drawn::Knobs::parse("sky = false");
    assert!(notes.is_empty(), "{notes:?}");
    shot.scene.set_terrain_knobs(knobs);
    shot.step_held(gpu);
    shot.step_held(gpu);
    assert!(water_drawn(gpu), "the water is drawn");
    let first = capture(gpu);
    keep("held-first", &first);
    for k in 0..4 {
        shot.step_held(gpu);
        let again = capture(gpu);
        // The air's dither moves a level or two from frame to frame, and the light's own noise
        // moves a pixel here and there a little more; the water itself not at all.
        let differ = changed_below_the_sky(&first, &again, 8);
        if differ != 0 {
            keep("held-again", &again);
        }
        assert!(
            differ <= 20,
            "frame {k} after the first differs in {differ} pixels"
        );
    }
    // At the same held clock, ripples drifting twice as fast lie elsewhere: the ripples are
    // where the clock and their speed put them, and nothing else moves them.
    let (knobs, notes) = dereth_terrain::land::drawn::Knobs::parse("sky = false\nripple_speed = 2");
    assert!(notes.is_empty(), "{notes:?}");
    shot.scene.set_terrain_knobs(knobs);
    shot.step_held(gpu);
    shot.step_held(gpu);
    let faster = capture(gpu);
    keep("held-faster", &faster);
    let moved = changed_below_the_sky(&first, &faster, 8);
    assert!(
        moved > 2_000,
        "only {moved} pixels of the water moved with the ripples' speed doubled"
    );
}

/// The water's own color starts from retail's water pictures' mean colors: worked out for the
/// pack's ground as the water is first drawn, and not again while the ground stays.
#[test]
fn retails_water_colors_are_worked_out_once_for_a_ground() {
    let _gpu = crate::common::gpu_lock();
    let pack = ground_pack();
    let (mut device, mut shot) = opened(&holtburg_shore(), &pack, BOXES);
    let gpu = &mut device;
    let worked_out = |gpu: &Gpu| {
        gpu.hifi_report()
            .and_then(|r| {
                r.notes
                    .iter()
                    .find(|(n, _)| *n == "retail water colors worked out")
                    .map(|(_, v)| *v)
            })
            .expect("the water pass's note")
    };
    shot.step_held(gpu);
    assert!(water_drawn(gpu), "the water is drawn");
    let first = worked_out(gpu);
    for _ in 0..10 {
        shot.step_held(gpu);
    }
    assert!(water_drawn(gpu), "the water is drawn");
    assert!(first >= 1, "worked out for the pack's ground");
    assert_eq!(
        worked_out(gpu),
        first,
        "worked out again for the same ground"
    );
}

#[test]
fn the_water_is_drawn_with_the_packs_ground_and_never_with_retail_look_or_its_knob_off() {
    let _gpu = crate::common::gpu_lock();
    let pack = ground_pack();
    let (mut device, mut shot) = opened(&holtburg_shore(), &pack, BOXES);
    let gpu = &mut device;
    shot.step_held(gpu);
    assert!(
        water_drawn(gpu),
        "the water is drawn with the pack's ground"
    );
    let with_water = capture(gpu);
    keep("knob-on", &with_water);
    // The knob off: the ground lays retail's water picture again, and no water is drawn.
    let (knobs, notes) = dereth_terrain::land::drawn::Knobs::parse("water = false");
    assert!(notes.is_empty(), "{notes:?}");
    shot.scene.set_terrain_knobs(knobs);
    shot.step_held(gpu);
    shot.step_held(gpu);
    assert!(!water_drawn(gpu), "no water is drawn with its knob off");
    let without = capture(gpu);
    keep("knob-off", &without);
    let differ = changed_below_the_sky(&with_water, &without, 16);
    assert!(
        differ > 20_000,
        "the water changed only {differ} pixels of the frame"
    );
    // Retail Look: the presentation draws retail's look, no water of its own.
    shot.scene.draw.cfg.render.fidelity =
        FidelityPreferences::parse_switch(&format!("{BOXES},RetailLook=1"))
            .expect("the boxes parse");
    shot.poll(gpu);
    shot.step_held(gpu);
    shot.step_held(gpu);
    assert!(!water_drawn(gpu), "no water is drawn with Retail Look");
}

/// The box alone draws the water, with no pack replacing anything, and changes at once.
#[test]
fn the_reflective_water_box_draws_the_water_with_no_art_pack() {
    let _gpu = crate::common::gpu_lock();
    let (mut device, mut shot) = opened_with(&holtburg_shore(), None, BOXES);
    let gpu = &mut device;
    shot.step_held(gpu);
    assert!(!water_drawn(gpu), "no water without the box");
    assert!(!shot.scene.drawn_ground_active());
    let without = capture(gpu);
    keep("box-off", &without);

    let ticked = format!("{BOXES},Water=1");
    set_boxes(&mut shot, gpu, &ticked);
    shot.step_held(gpu);
    assert!(water_drawn(gpu), "the box draws the water");
    assert!(shot.scene.drawn_ground_active());
    assert!(
        shot.scene.draw.art_pack().is_none(),
        "no pack was set for it"
    );
    let with = capture(gpu);
    keep("box-on", &with);
    let differ = changed_below_the_sky(&with, &without, 16);
    assert!(
        differ > 20_000,
        "the water changed only {differ} pixels of the frame"
    );

    // Retail Look draws the game's own water, whatever the box says.
    set_boxes(&mut shot, gpu, &format!("{ticked},RetailLook=1"));
    shot.step_held(gpu);
    assert!(!water_drawn(gpu), "no water is drawn with Retail Look");
    assert!(!shot.scene.drawn_ground_active());

    // Ticked again it comes back; unticked it goes.
    set_boxes(&mut shot, gpu, &ticked);
    shot.step_held(gpu);
    assert!(water_drawn(gpu), "the box draws the water again");
    set_boxes(&mut shot, gpu, BOXES);
    shot.step_held(gpu);
    assert!(!water_drawn(gpu), "unticked, no water");
    assert!(!shot.scene.drawn_ground_active());
}

/// What `@report` printed behind a hill at Holtburg, a house's door beyond its crest. At this
/// draw distance the frame's first block draws no cell, so its end lies on the sky's last command.
const BEHIND_THE_HILL: &str = "dereth report 1; loc 0xA9B40020 [83.442406 173.377823 62.419792] 0.367503 0.000000 0.000000 -0.930023; camera 0xA9B4 [79.319176 177.73665 63.949776] yaw -2.383948 pitch -0.004999968 cell 0xA9B40020; eye 0xA9B4 [79.319176 177.73665 64.7254]; fov 90 vertical 54.545456 aspect 1; mode orbit; time 0.839 Evensong-and-Half, server's clock; clock 307576472.0926675; weather auto; renderer wgpu; better lighting on; retail look off; effects Lighting=1,Shadows=1,Water=1; render scale 100; art pack none; draw distance 12; window 960x540; viewport 0 0 960 540; build dereth-client 0.4.8 abee60aa2fd707497d8838564480fa036ebf89d5";

/// The ground is drawn again over the buildings' stamps after the viewer's own block has stamped
/// them, so a door behind a hill stays behind it.
#[test]
fn a_door_behind_a_hill_stays_hidden_whatever_block_the_frame_opens_with() {
    let _gpu = crate::common::gpu_lock();
    let report = dereth_client_runtime::report::VisualReport::parse(BEHIND_THE_HILL)
        .expect("the report reads");
    let station = crate::instruments::hifi_stations::report_station(&report, None);
    let (mut device, mut shot) = opened_with(&station, None, &format!("{BOXES},Water=1"));
    let gpu = &mut device;
    shot.apply_report(&report);
    for _ in 0..12 {
        shot.step_held(gpu);
    }
    let without = capture(gpu);
    keep("hill-without-door", &without);
    shot.create_object(
        0x8300_0100,
        (0x0200_19FF, None),
        0xA9B4,
        (104.2, 141.2),
        40.0,
    );
    for _ in 0..12 {
        shot.step_held(gpu);
    }
    let with = capture(gpu);
    keep("hill-with-door", &with);
    let shown = changed_below_the_sky(&with, &without, 24);
    assert!(
        shown < 40,
        "{shown} pixels of the door show through the hill"
    );
}

/// What `@report` printed on the hill above the river at Holtburg: a house's door by the water
/// lies past the road below it.
const OVER_THE_KEPT_GROUND: &str = "dereth report 1; loc 0xA9B40033 [150.126495 66.247101 83.329735] -0.998657 0.000000 -0.000000 -0.051816; camera 0xA9B4 [150.62952 61.637173 86.39229] yaw -6.174508 pitch -0.32499987 cell 0xA9B40033; eye 0xA9B4 [150.4786 63.02015 86.18462]; fov 90 vertical 79.07219 aspect 1; mode orbit; time 0.281 Dawnsong, set here; clock 307678878.91440904; weather clear; renderer wgpu; better lighting on; retail look off; effects Lighting=1,Shadows=1,GlobalIllumination=1,AmbientOcclusion=1,Lamps=1,Sky=1,Weather=1,Water=1; draw distance 8; window 3358x2712; viewport 0 0 3358 2712; build dereth-client 0.4.8 86d148d6e9d65f3a54835a7197659488ecebfe3d";

/// A door stamp looks before it cuts: a door past the road, behind the ground the road lies on,
/// does not show through it.
#[test]
fn a_door_past_the_road_does_not_show_through_the_ground() {
    let _gpu = crate::common::gpu_lock();
    let report = dereth_client_runtime::report::VisualReport::parse(OVER_THE_KEPT_GROUND)
        .expect("the report reads");
    let station = crate::instruments::hifi_stations::report_station(&report, None);
    let (mut device, mut shot) = opened_with(&station, None, &format!("{BOXES},Water=1"));
    let gpu = &mut device;
    shot.apply_report(&report);
    for _ in 0..12 {
        shot.step_held(gpu);
    }
    let rgba = capture(gpu);
    keep("kept-ground", &rgba);
    // Where the road runs, past the lamp beside it.
    let w = SIZE.0 as usize;
    let dark = (274..286)
        .flat_map(|y| (670..680).map(move |x| (x, y)))
        .filter(|(x, y)| {
            let p = &rgba[(y * w + x) * 4..][..3];
            p.iter().all(|c| *c < 60)
        })
        .count();
    assert!(
        dark < 3,
        "{dark} black pixels of a door show through the road"
    );
}

/// The water's pixels in `with`, drawn beside `without` (the same frame without the water): those
/// that differ by more than 24 levels in a channel.
fn water_pixels(with: &[u8], without: &[u8]) -> Vec<bool> {
    with.as_chunks::<4>()
        .0
        .iter()
        .zip(without.as_chunks::<4>().0)
        .map(|(p, q)| (0..3).any(|c| p[c].abs_diff(q[c]) > 24))
        .collect()
}

#[test]
fn the_far_shore_and_its_reflections_hold_still_under_the_anti_aliasings_jitter() {
    let _gpu = crate::common::gpu_lock();
    let pack = ground_pack();
    let (mut device, mut shot) = opened(&over_holtburg(), &pack, JITTERED);
    let gpu = &mut device;
    // Settled: the anti-aliasing's history holds its blend of the jitter.
    for _ in 0..24 {
        shot.step_held(gpu);
    }
    assert!(water_drawn(gpu), "the water is drawn");
    let mut frames = Vec::new();
    for _ in 0..8 {
        shot.step_held(gpu);
        frames.push(capture(gpu));
    }
    // Where the water is: the same frame without it.
    let (knobs, notes) = dereth_terrain::land::drawn::Knobs::parse("water = false");
    assert!(notes.is_empty(), "{notes:?}");
    shot.scene.set_terrain_knobs(knobs);
    for _ in 0..24 {
        shot.step_held(gpu);
    }
    let dry = capture(gpu);
    let water = water_pixels(&frames[0], &dry);
    let wet = water.iter().filter(|w| **w).count();
    assert!(wet > 50_000, "only {wet} pixels of water in the frame");
    // From one frame to the next, the pixels of the water (its far shores among them) that
    // change by more than 32 levels: a shore or a reflection flickering between the bank and
    // the water, or between the land and the sky, as the frame is drawn a fraction of a pixel
    // off the last.
    let mut most = 0;
    for (k, pair) in frames.windows(2).enumerate() {
        let flicker = pair[0]
            .as_chunks::<4>()
            .0
            .iter()
            .zip(pair[1].as_chunks::<4>().0)
            .zip(&water)
            .filter(|((p, q), w)| **w && (0..3).any(|c| p[c].abs_diff(q[c]) > 32))
            .count();
        if flicker > most {
            keep(&format!("jittered-{k}"), &pair[1]);
        }
        most = most.max(flicker);
    }
    keep("jittered-first", &frames[0]);
    keep("jittered-dry", &dry);
    eprintln!("{wet} pixels of water; at most {most} change by over 32 levels a frame");
    assert!(
        most <= 30,
        "{most} pixels of the water flicker by over 32 levels from one frame to the next"
    );
}

/// The crater rim west of Neydisa, the highest ground in Dereth, looking south over the land: a
/// river winds through the valley kilometers off, a pixel or a few across, and the land runs on
/// to the haze at the horizon.
fn neydisa_rim() -> Station {
    Station {
        name: "neydisa-rim",
        why: "water kilometers off, seen from high over it",
        place: Place::Free {
            block: 0x92D1,
            x: 83.0,
            y: 12.0,
            eye: 6.0,
            yaw: -2.79,
            pitch: -0.122,
        },
        clock: Clock::Sunny(0.5),
        weather: false,
        land_radius: 2,
    }
}

/// From one of `frames` to the next, the pixels that change by more than 8 levels in a channel:
/// the mean over the frames.
fn mean_changes(frames: &[Vec<u8>]) -> usize {
    let total: usize = frames
        .windows(2)
        .map(|pair| {
            pair[0]
                .as_chunks::<4>()
                .0
                .iter()
                .zip(pair[1].as_chunks::<4>().0)
                .filter(|(p, q)| (0..3).any(|c| p[c].abs_diff(q[c]) > 8))
                .count()
        })
        .sum();
    total / frames.len().saturating_sub(1).max(1)
}

#[test]
fn water_kilometers_off_holds_as_still_as_the_land_under_the_anti_aliasings_jitter() {
    let _gpu = crate::common::gpu_lock();
    let pack = ground_pack();
    let (mut device, mut shot) = opened(&neydisa_rim(), &pack, JITTERED);
    let gpu = &mut device;
    let still = |shot: &mut Shot, gpu: &mut Gpu| {
        for _ in 0..24 {
            shot.step_held(gpu);
        }
        let mut frames = Vec::new();
        for _ in 0..12 {
            shot.step_held(gpu);
            frames.push(capture(gpu));
        }
        frames
    };
    let wet = still(&mut shot, gpu);
    assert!(water_drawn(gpu), "the water is drawn");
    // The same frames without the water: the land's own changes under the jitter.
    let (knobs, notes) = dereth_terrain::land::drawn::Knobs::parse("water = false");
    assert!(notes.is_empty(), "{notes:?}");
    shot.scene.set_terrain_knobs(knobs);
    let dry = still(&mut shot, gpu);
    let (with, without) = (mean_changes(&wet), mean_changes(&dry));
    keep("neydisa-wet", &wet[0]);
    keep("neydisa-dry", &dry[0]);
    eprintln!("pixels changing over 8 levels a frame: {with} with the water, {without} without");
    // The water may change a quarter as many pixels again as the land does, no more.
    assert!(
        with <= without + without / 4,
        "the water adds {} pixels changing a frame to the land's {without}",
        with.saturating_sub(without)
    );
}

/// Wading in the river below Holtburg: a body standing in the water up to its knees, seen from its
/// chase camera, the water all round it.
fn wading() -> Station {
    Station {
        name: "wading",
        why: "a body standing in the water",
        place: Place::Ground {
            block: 0xA9B5,
            x: 59.952,
            y: 140.634,
            yaw: -117.2,
        },
        clock: Clock::Sunny(0.5),
        weather: false,
        land_radius: 2,
    }
}

/// The pixels in rows `rows` whose color differs by more than 8 levels in any channel between
/// `a` and `b`.
fn changed_in(a: &[u8], b: &[u8], rows: std::ops::Range<usize>) -> usize {
    let row = SIZE.0 as usize * 4;
    a[rows.start * row..rows.end * row]
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b[rows.start * row..rows.end * row].as_chunks::<4>().0)
        .filter(|(p, q)| (0..3).any(|c| p[c].abs_diff(q[c]) > 8))
        .count()
}

/// The frame drawn with the water ringing round its bodies as strongly as `rings` asks, the wind's
/// ripples held still, the clock held while it settles.
fn ringing(shot: &mut Shot, gpu: &mut Gpu, rings: f32) -> Vec<u8> {
    let text = format!(
        "ripple_speed = 0
water_rings = {rings}"
    );
    let (knobs, notes) = dereth_terrain::land::drawn::Knobs::parse(&text);
    assert!(notes.is_empty(), "{notes:?}");
    shot.scene.set_terrain_knobs(knobs);
    for _ in 0..8 {
        shot.step_held(gpu);
    }
    assert!(water_drawn(gpu), "the water is drawn");
    capture(gpu)
}

#[test]
fn the_water_rings_round_a_body_standing_in_it_and_nowhere_far_off() {
    let _gpu = crate::common::gpu_lock();
    let pack = ground_pack();
    let (mut device, mut shot) = opened(&wading(), &pack, BOXES);
    let gpu = &mut device;
    let h = SIZE.1 as usize;
    // At one moment the frame without the rings and with them, and the latter held a frame
    // longer.
    let calm = ringing(&mut shot, gpu, 0.0);
    let ringed = ringing(&mut shot, gpu, 1.0);
    shot.step_held(gpu);
    let held = changed_in(&ringed, &capture(gpu), 0..h);
    keep("rings-calm", &calm);
    keep("rings", &ringed);
    // The rings lie round the body, in the water in the frame's lower half, and not on the far
    // water under the horizon (above it the sky's clouds settle on their own as the frames go
    // by); a held clock holds them still.
    let round = changed_in(&calm, &ringed, h / 2..h);
    let far = changed_in(&calm, &ringed, h / 4..h / 3);
    eprintln!("{round} pixels ringed round the body, {far} far off, {held} with the clock held");
    assert!(held <= 20, "{held} pixels changed with the clock held");
    assert!(round > 2_000, "only {round} pixels ringed round the body");
    assert!(far <= 20, "{far} pixels ringed far from the body");
}
