//! Behaviour: none (experimental Horizon rendering effects: Screen Brightness)
//!
//! Enhanced Graphics Mode scales its finished picture by the Screen Brightness, once: the sky, the
//! replayed world and what the presentation draws itself all alike, with the anti-aliasing and
//! without it.
//!
//! Fixture: the retail dats (`DERETH_TEST_DAT_DIR`) and a hardware `wgpu` device.

#![cfg(gpu)]

use dereth_client_runtime::render_prefs::FidelityPreferences;
use dereth_render::device::{Backend, DeviceConfig, Gpu};
use dereth_scene::world_scene::SceneWrites;

use crate::instruments::hifi_stations::{self, capture, Shot};

const W: u32 = 480;
const H: u32 = 270;

/// How much brighter the picture at `brightness` is than at none, the middle pixel's share, of
/// those neither black nor near white at none.
fn gain(spec: &str, brightness: f32) -> f32 {
    let store = crate::common::dats();
    let station = hifi_stations::stations(&store)
        .into_iter()
        .find(|s| s.name == "holtburg")
        .expect("the station");
    let cfg = DeviceConfig {
        width: W,
        height: H,
        hifi: true,
        ..DeviceConfig::default()
    };
    let mut gpu = Gpu::new_on(Backend::Wgpu, None, &cfg).expect("a wgpu device");
    let mut shot = Shot::open(&store, &mut gpu, &station);
    shot.scene.draw.cfg.render.fidelity =
        FidelityPreferences::parse_switch(spec).expect("a fidelity spec");
    shot.scene
        .update_from_preferences(&store, &mut gpu)
        .expect("the preferences poll");
    let mut settled = |gpu: &mut Gpu| {
        for _ in 0..8 {
            shot.step(gpu);
            assert!(gpu.hifi_settle(std::time::Duration::from_secs(60)));
        }
        capture(gpu)
    };
    let plain = settled(&mut gpu);
    gpu.set_gamma(brightness);
    let bright = settled(&mut gpu);
    let mut shares: Vec<f32> = plain
        .iter()
        .zip(&bright)
        .filter(|(p, _)| (40..=150).contains(*p))
        .map(|(p, b)| f32::from(*b) / f32::from(*p))
        .collect();
    assert!(shares.len() > 1000, "too few pixels to tell");
    shares.sort_by(f32::total_cmp);
    shares[shares.len() / 2]
}

#[test]
fn the_screen_brightness_scales_the_finished_picture_once() {
    let _gpu = crate::common::gpu_lock();
    for spec in ["Lighting=1", "Lighting=1,NoAntiAliasing=1"] {
        let share = gain(spec, 0.25);
        assert!((share - 1.5).abs() < 0.08, "{spec}: scaled by {share}");
    }
}
