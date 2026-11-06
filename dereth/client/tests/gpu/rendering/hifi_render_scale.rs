//! Behaviour: none (experimental Horizon rendering effects: the better lighting's world drawn at a
//! share of the frame's pixels and enlarged to the frame)
//!
//! At a render scale under a hundred percent the presentation draws its world at that share of
//! the frame's pixels each way, so the work of every pass that works per pixel falls with the
//! square of it, and enlarges the picture to fill the frame: the frame shows the same place as at
//! full scale, nothing shifted and nothing cut off, only softer. On macOS MetalFX rebuilds the
//! picture, elsewhere the anti-aliasing's blend into a history of the frame's size, and the frame
//! goes on at its full size after it; a frame neither rebuilds is enlarged along its edges.
//!
//! Fixture: the retail dats (`DERETH_TEST_DAT_DIR`) and a hardware `wgpu` device.

#![cfg(gpu)]

use dereth_client_runtime::render_prefs::FidelityPreferences;
use dereth_render::device::{Backend, DeviceConfig, Gpu};
use dereth_render::wgpu::sidecar::SidecarReport;
use dereth_scene::world_scene::SceneWrites;

use crate::instruments::hifi_stations::{self, capture, Shot};

const W: u32 = 960;
const H: u32 = 540;
/// The effects a frame is drawn with.
const FIDELITY: &str = "Lighting=1,Shadows=1,AmbientOcclusion=1,Sky=1";
/// Frames the station is stepped before its frame is taken.
const STEPS: usize = 24;

fn device() -> Gpu {
    device_of(W, H)
}

fn device_of(width: u32, height: u32) -> Gpu {
    let cfg = DeviceConfig {
        width,
        height,
        hifi: true,
        ..DeviceConfig::default()
    };
    let gpu = Gpu::new_on(Backend::Wgpu, None, &cfg)
        .unwrap_or_else(|e| panic!("this station needs a wgpu device and none opened: {e}"));
    assert_eq!(
        gpu.adapter_kind(),
        dereth_render::device::AdapterKind::Hardware,
        "{} is a software rasteriser",
        gpu.adapter_name()
    );
    gpu
}

/// The town at noon with the better lighting, drawn at `percent` of the frame's pixels each way
/// and stepped [`STEPS`] frames: its last frame and the presentation's report of it.
fn town(percent: u32) -> (Vec<u8>, SidecarReport) {
    let store = crate::common::dats();
    let station = hifi_stations::stations(&store)
        .into_iter()
        .find(|s| s.name == "holtburg")
        .expect("the town's station");
    let mut gpu = device();
    gpu.hifi_set_render_scale(percent);
    let mut shot = Shot::open(&store, &mut gpu, &station);
    shot.scene.draw.cfg.render.fidelity =
        FidelityPreferences::parse_switch(FIDELITY).expect("a fidelity spec");
    shot.scene
        .update_from_preferences(&store, &mut gpu)
        .expect("the preferences poll");
    for _ in 0..STEPS {
        shot.step(&mut gpu);
        assert!(
            gpu.hifi_settle(std::time::Duration::from_secs(60)),
            "the pipelines were not built in a minute"
        );
    }
    let report = gpu.hifi_report().expect("the presentation is installed");
    assert!(
        report.composited,
        "the presentation drew the frame: {report:?}"
    );
    (capture(&mut gpu), report)
}

fn note(report: &SidecarReport, name: &str) -> u64 {
    report
        .notes
        .iter()
        .find(|(n, _)| *n == name)
        .map_or_else(|| panic!("no note {name:?} in {report:?}"), |(_, v)| *v)
}

/// The mean brightness of each `cell`-pixel square of an RGBA frame, row by row.
fn cells(rgba: &[u8], cell: usize) -> Vec<f32> {
    let (w, h) = (W as usize, H as usize);
    let mut out = Vec::new();
    for cy in 0..h / cell {
        for cx in 0..w / cell {
            let mut sum = 0.0f32;
            for y in cy * cell..(cy + 1) * cell {
                for x in cx * cell..(cx + 1) * cell {
                    let p = &rgba[(y * w + x) * 4..(y * w + x) * 4 + 3];
                    sum += f32::from(p[0]) + f32::from(p[1]) + f32::from(p[2]);
                }
            }
            #[allow(clippy::cast_precision_loss)] // a small cell
            out.push(sum / (3 * cell * cell) as f32);
        }
    }
    out
}

/// At two thirds the world is drawn at two thirds of the frame's pixels each way; at a hundred
/// percent, at all of them.
#[test]
fn at_two_thirds_the_world_is_drawn_at_two_thirds_of_the_frames_pixels_each_way() {
    let _gpu = crate::common::gpu_lock();
    let (_, two_thirds) = town(67);
    assert_eq!(
        (
            note(&two_thirds, "world drawn width"),
            note(&two_thirds, "world drawn height")
        ),
        (643, 362)
    );
    let (_, full) = town(100);
    assert_eq!(
        (
            note(&full, "world drawn width"),
            note(&full, "world drawn height")
        ),
        (u64::from(W), u64::from(H))
    );
}

/// Enlarged to the frame, the picture drawn at two thirds shows the town where the full picture
/// shows it: over twenty-pixel squares, its brightness follows the full picture's far closer than
/// the full picture moved a square over does, and it fills the frame to its edges.
#[test]
fn the_world_drawn_at_two_thirds_fills_the_frame_where_the_full_picture_stands() {
    let _gpu = crate::common::gpu_lock();
    let (full, _) = town(100);
    let (two_thirds, _) = town(67);
    let cell = 20;
    let (a, b) = (cells(&full, cell), cells(&two_thirds, cell));
    let across = W as usize / cell;
    #[allow(clippy::cast_precision_loss)] // a few hundred cells
    let mean = |pairs: &mut dyn Iterator<Item = (f32, f32)>| {
        let (sum, n) = pairs.fold((0.0f32, 0usize), |(s, n), (x, y)| {
            (s + (x - y).abs(), n + 1)
        });
        sum / n.max(1) as f32
    };
    let aligned = mean(&mut a.iter().copied().zip(b.iter().copied()));
    // The full picture against itself a square to the right: what a shift of one square costs.
    let shifted = mean(
        &mut a
            .chunks(across)
            .flat_map(|row| row.iter().copied().zip(row.iter().copied().skip(1))),
    );
    eprintln!("aligned {aligned:.2} levels a cell, shifted by a cell {shifted:.2}");
    assert!(
        aligned * 2.0 < shifted,
        "the picture drawn at two thirds is {aligned:.2} levels a cell from the full one, a shift \
         of one cell {shifted:.2}"
    );
    // The last column and row of cells are drawn, not left black.
    let edge: Vec<f32> = b
        .chunks(across)
        .filter_map(|row| row.last().copied())
        .chain(b[b.len() - across..].iter().copied())
        .collect();
    let full_edge: Vec<f32> = a
        .chunks(across)
        .filter_map(|row| row.last().copied())
        .chain(a[a.len() - across..].iter().copied())
        .collect();
    let mean_of = |v: &[f32]| v.iter().sum::<f32>() / v.len().max(1) as f32;
    assert!(
        mean_of(&edge) > mean_of(&full_edge) * 0.8,
        "the edges of the enlarged picture are dark: {} against {}",
        mean_of(&edge),
        mean_of(&full_edge)
    );
}

/// On macOS the frame is upscaled by MetalFX at a render scale and not at a hundred percent, and
/// the world is still drawn at the share it was asked for.
#[cfg(target_os = "macos")]
#[test]
fn at_a_render_scale_metalfx_rebuilds_the_picture_and_at_a_hundred_percent_it_does_not() {
    let _gpu = crate::common::gpu_lock();
    let (_, scaled) = town(50);
    assert_eq!(note(&scaled, "upscaled by MetalFX"), 1, "{scaled:?}");
    assert_eq!(note(&scaled, "world drawn width"), u64::from(W / 2));
    let (_, full) = town(100);
    assert_eq!(note(&full, "upscaled by MetalFX"), 0, "{full:?}");
}

/// Asked for the temporal upscaler, on any platform, a frame at a render scale is rebuilt by it once
/// its history holds frames, and not enlarged in the composite.
#[test]
fn at_a_render_scale_the_temporal_upscaler_rebuilds_the_picture() {
    let _gpu = crate::common::gpu_lock();
    std::env::set_var("DERETH_HIFI_UPSCALER", "temporal");
    let (frame, scaled) = town(50);
    std::env::remove_var("DERETH_HIFI_UPSCALER");
    assert_eq!(
        note(&scaled, "upscaled by the temporal upscaler"),
        1,
        "{scaled:?}"
    );
    assert_eq!(note(&scaled, "upscaled by MetalFX"), 0);
    assert_eq!(note(&scaled, "enlarged by the spatial upscaler"), 0);
    assert_eq!(note(&scaled, "world drawn width"), u64::from(W / 2));
    let bright = frame.as_chunks::<4>().0.iter().filter(|p| p[1] > 8).count();
    assert!(bright * 10 > frame.len() / 4 * 9, "the frame is dark");
}

/// The scale can change between any two frames, as the options page changes it: the presentation
/// goes on drawing, and at each share its picture fills the frame.
#[test]
fn the_render_scale_changes_between_frames_without_the_presentation_failing() {
    let _gpu = crate::common::gpu_lock();
    let store = crate::common::dats();
    let station = hifi_stations::stations(&store)
        .into_iter()
        .find(|s| s.name == "holtburg")
        .expect("the town's station");
    let mut gpu = device();
    let mut shot = Shot::open(&store, &mut gpu, &station);
    shot.scene.draw.cfg.render.fidelity =
        FidelityPreferences::parse_switch(FIDELITY).expect("a fidelity spec");
    shot.scene
        .update_from_preferences(&store, &mut gpu)
        .expect("the preferences poll");
    for percent in [100, 67, 50, 100, 75, 50] {
        gpu.hifi_set_render_scale(percent);
        for _ in 0..6 {
            shot.step(&mut gpu);
            assert!(gpu.hifi_settle(std::time::Duration::from_secs(60)));
        }
        let report = gpu.hifi_report().expect("the presentation is installed");
        assert!(report.composited, "at {percent}%: {report:?}");
        assert_eq!(gpu.hifi_failed(), None, "at {percent}%");
        let frame = capture(&mut gpu);
        let bright = frame.chunks_exact(4).filter(|p| p[1] > 8).count();
        assert!(
            bright * 10 > frame.len() / 4 * 9,
            "at {percent}% the frame is dark"
        );
    }
}

/// How `a` differs from `b`, RGBA frames of one size: their peak signal to noise ratio, decibels.
fn psnr(a: &[u8], b: &[u8]) -> f64 {
    let sum: u64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| u64::from(x.abs_diff(*y)).pow(2))
        .sum();
    #[allow(clippy::cast_precision_loss)] // a mean over a frame
    let mean = sum as f64 / a.len() as f64;
    10.0 * (255.0f64 * 255.0 / mean.max(1e-9)).log10()
}

/// How much detail an RGBA frame `width` pixels across holds: the mean square of the green
/// channel's difference between neighbours.
fn detail(rgba: &[u8], width: usize) -> f64 {
    let green: Vec<i32> = rgba.chunks_exact(4).map(|p| i32::from(p[1])).collect();
    let sum: i64 = green
        .windows(2)
        .step_by(1)
        .enumerate()
        .filter(|(i, _)| (i + 1) % width != 0)
        .map(|(_, w)| i64::from((w[0] - w[1]).pow(2)))
        .sum();
    #[allow(clippy::cast_precision_loss)] // a mean over a frame
    let mean = sum as f64 / green.len() as f64;
    mean
}

/// MetalFX, the temporal and the spatial upscalers against the plain enlarging and the full
/// picture, the camera still and moving: each frame's likeness to the full picture and the detail
/// it holds, what rebuilt it, and the frames as files in `DERETH_HIFI_CAPTURE_DIR`.
/// `DERETH_HIFI_SIZE` is `WxH` (default 1920x1080). It asserts nothing.
#[test]
#[ignore = "instrument: compares; look at its frames"]
fn compare_the_upscalers() {
    let _gpu = crate::common::gpu_lock();
    let store = crate::common::dats();
    let station = hifi_stations::stations(&store)
        .into_iter()
        .find(|s| s.name == "holtburg")
        .expect("the town's station");
    let (width, height) = std::env::var("DERETH_HIFI_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((1920, 1080));
    let out = std::path::PathBuf::from(
        std::env::var("DERETH_HIFI_CAPTURE_DIR").unwrap_or_else(|_| "upscalers".to_owned()),
    );
    std::fs::create_dir_all(&out).expect("the frames' folder");
    let shoot = |percent: u32, upscaler: &str, jitter: &str, metres: f32| {
        std::env::set_var("DERETH_HIFI_UPSCALER", upscaler);
        std::env::set_var("DERETH_HIFI_METALFX_JITTER", jitter);
        let mut gpu = device_of(width, height);
        gpu.hifi_set_render_scale(percent);
        let mut shot = Shot::open(&store, &mut gpu, &station);
        shot.scene.draw.cfg.render.fidelity =
            FidelityPreferences::parse_switch(FIDELITY).expect("a fidelity spec");
        shot.scene
            .update_from_preferences(&store, &mut gpu)
            .expect("the preferences poll");
        for _ in 0..48 {
            shot.scene.camera.position.x += metres;
            shot.step(&mut gpu);
            assert!(gpu.hifi_settle(std::time::Duration::from_secs(60)));
        }
        let report = gpu.hifi_report().expect("the presentation is installed");
        assert_eq!(gpu.hifi_failed(), None);
        let by = [
            ("upscaled by MetalFX", "metalfx"),
            ("upscaled by the temporal upscaler", "temporal"),
            ("enlarged by the spatial upscaler", "spatial"),
        ]
        .into_iter()
        .find(|(n, _)| note(&report, n) == 1)
        .map_or("plain", |(_, by)| by);
        (capture(&mut gpu), by)
    };
    for (motion, metres) in [("still", 0.0), ("moving", 0.1)] {
        let (full, _) = shoot(100, "", "1", metres);
        hifi_stations::write_png(
            &out.join(format!("{motion}-full.png")),
            width,
            height,
            &full,
        );
        #[allow(clippy::cast_possible_truncation)] // a row's width
        let across = width as usize;
        eprintln!("{motion} full: detail {:.1}", detail(&full, across));
        for (name, percent, upscaler, jitter) in [
            ("metalfx-67", 67, "", "1"),
            ("temporal-67", 67, "temporal", "1"),
            ("spatial-67", 67, "spatial", "1"),
            ("plain-67", 67, "plain", "1"),
            ("metalfx-50", 50, "", "1"),
            ("metalfx-50-jitter-flipped", 50, "", "-1"),
            ("temporal-50", 50, "temporal", "1"),
            ("spatial-50", 50, "spatial", "1"),
            ("plain-50", 50, "plain", "1"),
        ] {
            let (frame, by) = shoot(percent, upscaler, jitter, metres);
            hifi_stations::write_png(
                &out.join(format!("{motion}-{name}.png")),
                width,
                height,
                &frame,
            );
            eprintln!(
                "{motion} {name}: psnr {:.2} dB, detail {:.1}, by {by}",
                psnr(&frame, &full),
                detail(&frame, across),
            );
        }
    }
}
