//! Not a claim: an instrument that draws the options window, every tab scrolled from top to bottom
//! and every dropdown list opened, with the interface's own art, into PNG files for a person to
//! look at.
//!
//! - `DERETH_UI_SHOTS`: the folder the pictures go to (the test does nothing without it).
//! - `DERETH_UI_SET`: preferences set first, `name=value` with `;` between (`true`, `false`, an
//!   integer or a number).
//! - `DERETH_UI_TABS`: a comma list of the tab indices to draw (default all of them).
//! - `DERETH_UI_MENUS`: more dropdowns to open, by caption, `;` between.
//! - `DERETH_UI_LIST`: when set, prints every control's caption on each picture's page.

use std::path::Path;
use std::sync::Arc;

use dereth_client_contract::options::fidelity::Availability;
use dereth_client_contract::options::interface::Interface;
use dereth_client_contract::options::sheet::{self, PageId, Value};
use dereth_client_contract::options::store;
use dereth_client_contract::view::PrefValue;
use dereth_horizon::art::Art;
use dereth_horizon::draw::DrawList;
use dereth_horizon::ui::game::GameState;
use dereth_horizon::ui::input::InputFrame;
use dereth_horizon::ui::panels::WindowId;
use dereth_horizon::ui::HorizonUi;

use crate::Harness;

/// The window's size.
const W: usize = 1920;
const H: usize = 1080;

/// The dropdowns of this interface's own that the options sheet doesn't list.
const OWN_MENUS: [&str; 1] = ["Interface Scale"];

#[test]
#[ignore = "an instrument: set DERETH_UI_SHOTS to a folder"]
fn draw_every_options_tab_and_dropdown_for_a_person_to_look_at() {
    let Some(dir) = std::env::var_os("DERETH_UI_SHOTS") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("the pictures' folder");
    store::init();
    for item in std::env::var("DERETH_UI_SET")
        .unwrap_or_default()
        .split(';')
    {
        let Some((name, v)) = item.split_once('=') else {
            continue;
        };
        let v = v.trim();
        let value = match v {
            "true" => PrefValue::Bool(true),
            "false" => PrefValue::Bool(false),
            _ => v.parse::<i32>().map_or_else(
                |_| PrefValue::Float(v.parse::<f32>().expect("a preference's value")),
                PrefValue::Int,
            ),
        };
        let _ = store::set_value(name.trim(), value);
    }
    let state = GameState {
        in_world: true,
        name: "Tester".into(),
        hifi: Availability {
            built: true,
            wgpu: true,
            widened: true,
            supported: true,
            rays: Some(false),
            active: true,
            ..Availability::default()
        },
        ..GameState::default()
    };
    let art = Arc::new(Art::new(Arc::new(
        dereth_horizon::pieces::Pieces::built_in().expect("the pieces built in"),
    )));
    let mut h = Harness {
        ui: HorizonUi::new(Arc::clone(&art), Default::default()),
        list: DrawList::default(),
        input: InputFrame::default(),
        screen: (W as f32, H as f32),
    };
    let extra: Vec<String> = std::env::var("DERETH_UI_MENUS")
        .unwrap_or_default()
        .split(';')
        .map(|m| m.trim().to_owned())
        .filter(|m| !m.is_empty())
        .collect();
    let menus: Vec<&str> = sheet::rows_for(PageId::Client, Interface::Horizon)
        .filter(|r| matches!(r.value, Value::Menu(_)))
        .map(|r| r.caption)
        .chain(OWN_MENUS)
        .chain(extra.iter().map(String::as_str))
        .collect();
    h.frame(&state);
    h.ui.windows.open(WindowId::Options, 0.0);
    let tabs = h.ui.windows.options_page.scroll.len();
    let wanted: Vec<usize> = std::env::var("DERETH_UI_TABS").map_or_else(
        |_| (0..tabs).collect(),
        |s| s.split(',').filter_map(|t| t.trim().parse().ok()).collect(),
    );
    let mut opened = Vec::new();
    for tab in wanted {
        h.ui.windows.options_page.tab = tab;
        h.ui.windows.options_page.scroll[tab] = 0.0;
        settle(&mut h, &state);
        let mut last = f32::NEG_INFINITY;
        for step in 0..16 {
            let scroll = h.ui.windows.options_page.scroll[tab];
            if scroll <= last {
                break;
            }
            last = scroll;
            write(&dir.join(format!("tab{tab}-{step:02}.png")), &h, &art);
            if std::env::var_os("DERETH_UI_LIST").is_some() {
                let names: Vec<&str> =
                    h.ui.windows
                        .options_page
                        .controls
                        .iter()
                        .map(|(c, _)| c.as_str())
                        .collect();
                println!("tab{tab}-{step:02}: {}", names.join(" | "));
            }
            let Some(list) = h.ui.windows.options_page.control("list") else {
                break;
            };
            for caption in &menus {
                if opened.contains(caption) {
                    continue;
                }
                let Some(r) = h.ui.windows.options_page.control(caption) else {
                    continue;
                };
                if r.y < list.y || r.bottom() > list.bottom() {
                    continue;
                }
                h.move_to(r.x + 10.0, r.y + r.h / 2.0);
                h.press();
                h.frame(&state);
                h.release();
                h.frame(&state);
                if h.ui.windows.options_page.menu.is_some() {
                    let name: String = caption
                        .chars()
                        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                        .collect();
                    write(&dir.join(format!("tab{tab}-menu-{name}.png")), &h, &art);
                    opened.push(caption);
                }
                h.ui.windows.options_page.menu = None;
                h.move_to(0.0, 0.0);
                h.frame(&state);
            }
            h.ui.windows.options_page.scroll[tab] = scroll + list.h * 0.85;
            settle(&mut h, &state);
        }
    }
}

/// Frames enough for the window to fade in and a scroll to land.
fn settle(h: &mut Harness, state: &GameState) {
    for _ in 0..30 {
        h.frame(state);
    }
}

/// The frame's quads composited in software, nearest texels and straight alpha, as a PNG.
fn write(path: &Path, h: &Harness, art: &Art) {
    let mut px = vec![[24.0f32, 26.0, 30.0]; W * H];
    for q in &h.list.quads {
        let image = q.tex.and_then(|t| art.image(t));
        let mut r = q.dst;
        if let Some(c) = q.clip {
            match r.intersect(&c) {
                Some(v) => r = v,
                None => continue,
            }
        }
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        for y in (r.y.max(0.0) as usize)..(r.bottom().min(H as f32).max(0.0) as usize) {
            for x in (r.x.max(0.0) as usize)..(r.right().min(W as f32).max(0.0) as usize) {
                let ca = [
                    ((q.colour >> 16) & 0xFF) as f32 / 255.0,
                    ((q.colour >> 8) & 0xFF) as f32 / 255.0,
                    (q.colour & 0xFF) as f32 / 255.0,
                    ((q.colour >> 24) & 0xFF) as f32 / 255.0,
                ];
                let texel = match &image {
                    Some(img) => {
                        let u = q.src.x + (x as f32 + 0.5 - q.dst.x) / q.dst.w * q.src.w;
                        let v = q.src.y + (y as f32 + 0.5 - q.dst.y) / q.dst.h * q.src.h;
                        let tx = (u.max(0.0) as usize).min(img.width as usize - 1);
                        let ty = (v.max(0.0) as usize).min(img.height as usize - 1);
                        let i = (ty * img.width as usize + tx) * 4;
                        let b = &img.bgra[i..i + 4];
                        [
                            f32::from(b[2]) / 255.0,
                            f32::from(b[1]) / 255.0,
                            f32::from(b[0]) / 255.0,
                            f32::from(b[3]) / 255.0,
                        ]
                    }
                    None if q.tex.is_some() => continue,
                    None => [1.0, 1.0, 1.0, 1.0],
                };
                let a = texel[3] * ca[3];
                let p = &mut px[y * W + x];
                for k in 0..3 {
                    p[k] = p[k] * (1.0 - a) + 255.0 * texel[k] * ca[k] * a;
                }
            }
        }
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let rgb: Vec<u8> = px
        .iter()
        .flat_map(|p| p.map(|c| c.clamp(0.0, 255.0) as u8))
        .collect();
    let file = std::fs::File::create(path).expect("the picture's file");
    #[allow(clippy::cast_possible_truncation)]
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), W as u32, H as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .and_then(|mut w| w.write_image_data(&rgb))
        .expect("the picture written");
}
