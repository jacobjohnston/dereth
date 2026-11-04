//! The Display page of the settings opens, in a build with the experimental rendering effects, with
//! a Graphics menu choosing standard or enhanced graphics, and ends with their boxes under a
//! heading of their own: a box another box's choice rules out leaves the page, one the device rules
//! out is greyed and cannot be ticked, and the rest tick as any other.
//!
//! Behaviour: none (experimental Horizon interface)
#![cfg_attr(not(feature = "hifi"), allow(unused_imports, dead_code))]

use dereth_client_contract::options::fidelity::{self, Availability};
use dereth_client_contract::options::names::fidelity as names;
use dereth_client_contract::options::store;
use dereth_client_contract::view::PrefValue;
use dereth_client_contract::UiRequest;
use dereth_horizon::ui::game::GameState;
use dereth_horizon::ui::panels::options::tab;
use dereth_horizon::ui::panels::WindowId;

use crate::Harness;

/// The Display page, scrolled to its end, with the device `hifi` reports and every box off.
fn client_page(#[cfg(feature = "hifi")] hifi: Availability) -> (Harness, GameState) {
    store::init();
    for o in &fidelity::OPTIONS {
        store::set_value(o.name, PrefValue::Bool(false));
    }
    #[allow(unused_mut)]
    let mut state = GameState {
        in_world: true,
        name: "Tester".into(),
        ..GameState::default()
    };
    #[cfg(feature = "hifi")]
    {
        state.hifi = hifi;
    }
    let mut h = Harness::new(Default::default());
    h.frame(&state);
    h.ui.windows.open(WindowId::Options, 0.0);
    h.ui.windows.options_page.tab = tab::DISPLAY;
    h.frame(&state);
    h.ui.windows.options_page.scroll[tab::DISPLAY] = 1.0e6;
    h.frame(&state);
    h.frame(&state);
    (h, state)
}

/// Scroll the Client page so the row captioned `caption` shows whole in its list, from the page's
/// top down a few pixels at a time; whether it was found. The effects' rows are more than the
/// list shows at once.
fn show(h: &mut Harness, state: &GameState, caption: &str) -> bool {
    let whole = |h: &Harness| {
        let page = &h.ui.windows.options_page;
        match (page.control("list"), page.control(caption)) {
            (Some(list), Some(r)) => r.y >= list.y && r.bottom() <= list.bottom(),
            _ => false,
        }
    };
    if whole(h) {
        return true;
    }
    h.ui.windows.options_page.scroll[tab::DISPLAY] = 0.0;
    h.frame(state);
    for _ in 0..2000 {
        if whole(h) {
            return true;
        }
        h.ui.windows.options_page.scroll[tab::DISPLAY] += 6.0;
        h.frame(state);
    }
    false
}

/// Choose Enhanced in the Graphics menu, and what that asked the game for. Where the effects are
/// not offered it shows no list to choose in.
#[cfg(feature = "hifi")]
fn choose_enhanced(h: &mut Harness, state: &GameState) -> Vec<UiRequest> {
    assert!(show(h, state, fidelity::MODE_CAPTION), "no Graphics row");
    let r =
        h.ui.windows
            .options_page
            .control(fidelity::MODE_CAPTION)
            .expect("its box");
    h.move_to(r.x + 10.0, r.y + r.h / 2.0);
    h.press();
    let mut out = h.frame(state).requests;
    h.release();
    out.extend(h.frame(state).requests);
    h.ui.windows.options_page.choose_in_open_list(1);
    out.extend(h.frame(state).requests);
    for r in &out {
        if let UiRequest::SetPreference(name, value) = r {
            store::set_value(name, value.clone());
        }
    }
    out
}

/// Click the box of the row captioned `caption`, and what the click asked the game for.
fn tick(h: &mut Harness, state: &GameState, caption: &str) -> Vec<UiRequest> {
    #[cfg(feature = "hifi")]
    if caption == fidelity::find(names::LIGHTING).expect("its box").caption {
        return choose_enhanced(h, state);
    }
    assert!(show(h, state, caption), "no {caption} row");
    let r =
        h.ui.windows
            .options_page
            .control(caption)
            .unwrap_or_else(|| panic!("no {caption} row"));
    h.move_to(r.x + 10.0, r.y + r.h / 2.0);
    h.press();
    let mut out = h.frame(state).requests;
    h.release();
    out.extend(h.frame(state).requests);
    out
}

fn sets(requests: &[UiRequest], name: &str) -> Option<PrefValue> {
    requests.iter().find_map(|r| match r {
        UiRequest::SetPreference(n, v) if *n == name => Some(v.clone()),
        _ => None,
    })
}

fn caption(name: &str) -> &'static str {
    fidelity::find(name).expect("a box").caption
}

#[test]
#[cfg(not(feature = "hifi"))]
fn without_the_effects_the_client_page_has_none_of_their_boxes() {
    let (h, _) = client_page();
    for o in &fidelity::OPTIONS {
        assert!(h.ui.windows.options_page.control(o.caption).is_none());
    }
}

/// Whether the page has a row captioned `caption`, wherever it is scrolled to.
fn on_the_page(h: &mut Harness, state: &GameState, caption: &str) -> bool {
    show(h, state, caption)
}

#[cfg(feature = "hifi")]
const READY: Availability = Availability {
    built: true,
    wgpu: true,
    widened: true,
    supported: true,
    rays: Some(false),
    active: false,
    failed: None,
    wgpu_next: None,
};

#[test]
#[cfg(feature = "hifi")]
fn a_new_player_starts_with_the_effects_on() {
    store::init();
    for o in &fidelity::OPTIONS {
        assert_eq!(
            store::inq_value(o.name),
            Some(PrefValue::Bool(o.on_by_default)),
            "{}",
            o.name
        );
    }
}

#[test]
#[cfg(feature = "hifi")]
fn with_enhanced_graphics_off_only_the_occlusion_and_the_sky_stay_with_its_box() {
    let (mut h, state) = client_page(READY);
    for o in &fidelity::OPTIONS {
        let stays = [names::AMBIENT_OCCLUSION, names::SKY].contains(&o.name);
        assert_eq!(
            on_the_page(&mut h, &state, o.caption),
            stays,
            "{}",
            o.caption
        );
    }
    // They tick on their own.
    let asked = tick(&mut h, &state, caption(names::SKY));
    assert_eq!(sets(&asked, names::SKY), Some(PrefValue::Bool(true)));
    let asked = tick(&mut h, &state, caption(names::AMBIENT_OCCLUSION));
    assert_eq!(
        sets(&asked, names::AMBIENT_OCCLUSION),
        Some(PrefValue::Bool(true))
    );
    // Ticked, the rest come back and tick as any other.
    let asked = tick(&mut h, &state, caption(names::LIGHTING));
    assert_eq!(sets(&asked, names::LIGHTING), Some(PrefValue::Bool(true)));
    h.frame(&state);
    let asked = tick(&mut h, &state, caption(names::SHADOWS));
    assert_eq!(sets(&asked, names::SHADOWS), Some(PrefValue::Bool(true)));
    let asked = tick(&mut h, &state, caption(names::WEATHER));
    assert_eq!(sets(&asked, names::WEATHER), Some(PrefValue::Bool(true)));
    // On a device without ray tracing the lamps stay, greyed.
    let asked = tick(&mut h, &state, caption(names::LAMPS));
    assert_eq!(sets(&asked, names::LAMPS), None, "{asked:?}");
    // The bounced light ticked leaves the occlusion's box free: indoors it is the only effect.
    let asked = tick(&mut h, &state, caption(names::GLOBAL_ILLUMINATION));
    assert_eq!(
        sets(&asked, names::GLOBAL_ILLUMINATION),
        Some(PrefValue::Bool(true))
    );
    let asked = tick(&mut h, &state, caption(names::AMBIENT_OCCLUSION));
    assert_eq!(
        sets(&asked, names::AMBIENT_OCCLUSION),
        Some(PrefValue::Bool(false))
    );
}

#[test]
#[cfg(feature = "hifi")]
fn keep_retail_look_takes_what_an_art_pack_draws_off_the_page() {
    let (mut h, state) = client_page(READY);
    tick(&mut h, &state, caption(names::LIGHTING));
    h.frame(&state);
    let pack = [names::NO_SLOPE_MATERIALS];
    for name in pack {
        assert!(on_the_page(&mut h, &state, caption(name)), "{name}");
    }
    let asked = tick(&mut h, &state, caption(names::RETAIL_LOOK));
    assert_eq!(
        sets(&asked, names::RETAIL_LOOK),
        Some(PrefValue::Bool(true))
    );
    h.frame(&state);
    for name in pack {
        assert!(!on_the_page(&mut h, &state, caption(name)), "{name}");
    }
    assert!(on_the_page(&mut h, &state, caption(names::SHADOWS)));
}

/// God Rays starts at Low, and is on the page only with Enhanced Graphics Mode on.
#[test]
#[cfg(feature = "hifi")]
fn god_rays_start_low_and_show_only_with_enhanced_graphics() {
    let rays = dereth_client_contract::options::god_rays::CAPTION;
    let (mut h, state) = client_page(READY);
    assert_eq!(store::inq_value(names::GOD_RAYS), Some(PrefValue::Int(1)));
    assert!(!on_the_page(&mut h, &state, rays));
    tick(&mut h, &state, caption(names::LIGHTING));
    h.frame(&state);
    assert!(on_the_page(&mut h, &state, rays));
}

/// Open the Quality Preset menu's list: its names, and the one the box shows. The list is closed
/// again with that one chosen, which changes nothing.
#[cfg(feature = "hifi")]
fn presets_shown(h: &mut Harness, state: &GameState) -> (Vec<String>, usize) {
    assert!(
        show(h, state, fidelity::PRESET_CAPTION),
        "no Quality Preset row"
    );
    let r =
        h.ui.windows
            .options_page
            .control(fidelity::PRESET_CAPTION)
            .expect("its box");
    h.move_to(r.x + 10.0, r.y + r.h / 2.0);
    h.press();
    h.frame(state);
    h.release();
    h.frame(state);
    let menu = h.ui.windows.options_page.menu.as_ref().expect("its list");
    let shown = (menu.names.clone(), menu.at);
    h.ui.windows.options_page.choose_in_open_list(shown.1);
    h.frame(state);
    shown
}

#[test]
#[cfg(feature = "hifi")]
fn a_quality_preset_sets_its_values_and_shows_until_a_box_changes() {
    let (mut h, state) = client_page(READY);
    tick(&mut h, &state, caption(names::LIGHTING));
    h.frame(&state);
    // Every box off is no preset's.
    let (shown, at) = presets_shown(&mut h, &state);
    assert_eq!(shown, ["Low", "Medium", "High", "Ultra", "Custom"]);
    assert_eq!(at, 4);
    // High chosen: what it sets is set, and it is shown, though Medium sets the same.
    let r =
        h.ui.windows
            .options_page
            .control(fidelity::PRESET_CAPTION)
            .expect("its box");
    h.move_to(r.x + 10.0, r.y + r.h / 2.0);
    h.press();
    h.frame(&state);
    h.release();
    h.frame(&state);
    h.ui.windows.options_page.choose_in_open_list(2);
    let asked = h.frame(&state).requests;
    for (name, value) in fidelity::PRESETS[2].values() {
        assert_eq!(sets(&asked, name), Some(value), "{name}");
    }
    let (shown, at) = presets_shown(&mut h, &state);
    assert_eq!(shown, ["Low", "Medium", "High", "Ultra"]);
    assert_eq!(at, 2);
    // A box of the preset's changed: the player's own settings.
    let asked = tick(&mut h, &state, caption(names::SKY));
    assert_eq!(sets(&asked, names::SKY), Some(PrefValue::Bool(false)));
    assert_eq!(presets_shown(&mut h, &state).1, 4);
}

#[test]
#[cfg(feature = "hifi")]
fn anti_aliasing_and_slope_materials_are_ticked_while_their_preferences_are_off() {
    let (mut h, state) = client_page(READY);
    tick(&mut h, &state, caption(names::LIGHTING));
    h.frame(&state);
    for name in [names::NO_ANTI_ALIASING, names::NO_SLOPE_MATERIALS] {
        store::set_value(name, PrefValue::Bool(false));
        h.frame(&state);
        assert_eq!(store::inq_value(name), Some(PrefValue::Bool(false)));
        let asked = tick(&mut h, &state, caption(name));
        assert_eq!(
            sets(&asked, name),
            Some(PrefValue::Bool(true)),
            "unticked: off"
        );
        h.frame(&state);
        let asked = tick(&mut h, &state, caption(name));
        assert_eq!(
            sets(&asked, name),
            Some(PrefValue::Bool(false)),
            "ticked: on"
        );
    }
}

#[test]
#[cfg(feature = "hifi")]
fn a_device_with_ray_tracing_lets_the_lamps_be_ticked_with_the_lighting() {
    let (mut h, state) = client_page(Availability {
        rays: Some(true),
        ..READY
    });
    assert!(!on_the_page(&mut h, &state, caption(names::LAMPS)));
    let asked = tick(&mut h, &state, caption(names::LIGHTING));
    assert_eq!(sets(&asked, names::LIGHTING), Some(PrefValue::Bool(true)));
    h.frame(&state);
    let asked = tick(&mut h, &state, caption(names::LAMPS));
    assert_eq!(sets(&asked, names::LAMPS), Some(PrefValue::Bool(true)));
}

/// On a renderer the effects do not draw on, every box the page shows is greyed and none can be
/// ticked: the status line says how to get them.
#[test]
#[cfg(feature = "hifi")]
fn on_a_renderer_other_than_wgpu_every_box_is_greyed() {
    let native = Availability {
        built: true,
        wgpu: false,
        widened: false,
        supported: false,
        rays: None,
        active: false,
        failed: None,
        wgpu_next: None,
    };
    assert!(!native.offered());
    let (mut h, state) = client_page(native);
    for name in [names::LIGHTING, names::AMBIENT_OCCLUSION, names::SKY] {
        let asked = tick(&mut h, &state, caption(name));
        assert_eq!(sets(&asked, name), None, "{name}: {asked:?}");
        assert_eq!(store::inq_value(name), Some(PrefValue::Bool(false)));
    }
}

/// Off the wgpu renderer, the status line points at the renderer choice above the boxes, and once
/// wgpu is chosen there it says only the restart is left.
#[test]
#[cfg(feature = "hifi")]
fn off_the_wgpu_renderer_the_status_line_points_at_the_renderer_choice() {
    use dereth_client_contract::options::renderer::{self, RendererStatus};
    use dereth_client_contract::RendererChoice::{Vulkan, Wgpu};
    let native = Availability {
        built: true,
        ..Availability::default()
    };
    let (mut h, mut state) = client_page(native);
    state.renderers = RendererStatus {
        offered: vec![Vulkan, Wgpu],
        running: Some(Vulkan),
        ..RendererStatus::default()
    };
    h.frame(&state);
    let page = &h.ui.windows.options_page;
    let status = "Status: Off on this renderer: choose wgpu as the Renderer above, then restart";
    let (_, line) = page
        .controls
        .iter()
        .find(|(l, _)| l == status)
        .unwrap_or_else(|| panic!("{:?}", page.controls));
    let choice = page
        .control(renderer::CAPTION)
        .expect("the renderer choice");
    assert!(choice.y < line.y, "the choice is above the boxes' status");
    store::set_value(renderer::RENDERER, PrefValue::Int(renderer::value(Wgpu)));
    h.frame(&state);
    assert!(h
        .ui
        .windows
        .options_page
        .control("Status: Off until the restart onto wgpu")
        .is_some());
}

/// The renderers a desktop build offers, with `running` drawing.
#[cfg(feature = "hifi")]
fn desktop_on(
    running: dereth_client_contract::RendererChoice,
) -> dereth_client_contract::options::renderer::RendererStatus {
    use dereth_client_contract::RendererChoice::{Vulkan, Wgpu};
    dereth_client_contract::options::renderer::RendererStatus {
        offered: vec![Vulkan, Wgpu],
        running: Some(running),
        ..Default::default()
    }
}

/// The status lines the page drew in its last frame.
fn status(h: &Harness) -> Vec<String> {
    h.ui.windows
        .options_page
        .controls
        .iter()
        .map(|(l, _)| l.clone())
        .filter(|l| l.starts_with("Status: "))
        .collect()
}

/// Pick wgpu, tick, restart: on another renderer with wgpu chosen for the next start, the boxes
/// tick (the lamps too: whether the card traces rays is the restart's to find) and the status line
/// says the restart onto wgpu applies them; after it they draw, and on a card without ray tracing
/// the lamps' box is greyed with its tick kept.
#[test]
#[cfg(feature = "hifi")]
fn with_wgpu_chosen_on_another_renderer_the_boxes_tick_and_the_restart_onto_it_applies_them() {
    use dereth_client_contract::options::renderer;
    use dereth_client_contract::RendererChoice::{Vulkan, Wgpu};
    let (mut h, mut state) = client_page(Availability {
        built: true,
        ..Availability::default()
    });
    state.renderers = desktop_on(Vulkan);
    store::set_value(renderer::RENDERER, PrefValue::Int(renderer::value(Wgpu)));
    h.frame(&state);
    assert_eq!(status(&h), ["Status: Off until the restart onto wgpu"]);
    let asked = tick(&mut h, &state, caption(names::LIGHTING));
    assert_eq!(sets(&asked, names::LIGHTING), Some(PrefValue::Bool(true)));
    let asked = tick(&mut h, &state, caption(names::LAMPS));
    assert_eq!(sets(&asked, names::LAMPS), Some(PrefValue::Bool(true)));
    h.frame(&state);
    assert_eq!(status(&h), ["Status: Restart onto wgpu to apply"]);
    // Restarted onto wgpu with a box ticked: the device is asked for them, and they draw.
    state.renderers = desktop_on(Wgpu);
    state.hifi = Availability {
        built: true,
        wgpu: true,
        widened: true,
        supported: true,
        rays: Some(false),
        active: true,
        failed: None,
        wgpu_next: None,
    };
    h.frame(&state);
    assert_eq!(status(&h), ["Status: Active"]);
    // This card does not trace rays: the lamps' box keeps its tick and cannot be changed.
    let asked = tick(&mut h, &state, caption(names::LAMPS));
    assert_eq!(sets(&asked, names::LAMPS), None, "{asked:?}");
    assert_eq!(store::inq_value(names::LAMPS), Some(PrefValue::Bool(true)));
}

/// Pick wgpu, restart, tick: on wgpu started without the effects there is nothing to say until a
/// box is ticked, every box ticks (the lamps on a card that traces rays), and the status line says
/// the restart applies them; after it they draw. The page never says wgpu is needed while it is
/// the renderer.
#[test]
#[cfg(feature = "hifi")]
fn on_wgpu_started_without_the_effects_a_tick_says_the_restart_applies_it() {
    use dereth_client_contract::options::renderer;
    use dereth_client_contract::RendererChoice::Wgpu;
    let (mut h, mut state) = client_page(Availability {
        built: true,
        wgpu: true,
        widened: false,
        supported: true,
        rays: Some(true),
        active: false,
        failed: None,
        wgpu_next: None,
    });
    state.renderers = desktop_on(Wgpu);
    store::set_value(renderer::RENDERER, PrefValue::Int(renderer::value(Wgpu)));
    h.frame(&state);
    assert!(status(&h).is_empty(), "{:?}", status(&h));
    let asked = tick(&mut h, &state, caption(names::LIGHTING));
    assert_eq!(sets(&asked, names::LIGHTING), Some(PrefValue::Bool(true)));
    let asked = tick(&mut h, &state, caption(names::LAMPS));
    assert_eq!(
        sets(&asked, names::LAMPS),
        Some(PrefValue::Bool(true)),
        "the lamps on a card that traces rays"
    );
    h.frame(&state);
    assert_eq!(status(&h), ["Status: Restart to apply"]);
    // Restarted with a box ticked: the device is asked for them, and they draw.
    state.hifi = Availability {
        widened: true,
        active: true,
        ..state.hifi.clone()
    };
    h.frame(&state);
    assert_eq!(status(&h), ["Status: Active"]);
}

/// Off the wgpu renderer, where a plain restart would not bring wgpu up, the status line says so
/// rather than promising the restart: this start asked for wgpu and could not create it, or
/// `--renderer` holds the run to another while wgpu is chosen.
#[test]
#[cfg(feature = "hifi")]
fn the_status_line_promises_no_restart_onto_wgpu_that_would_not_come_up_on_it() {
    use dereth_client_contract::options::renderer::{self, RendererStatus};
    use dereth_client_contract::RendererChoice::{Vulkan, Wgpu};
    let desktop = RendererStatus {
        offered: vec![Vulkan, Wgpu],
        running: Some(Vulkan),
        ..RendererStatus::default()
    };
    let cases = [
        (
            "the preferences file named wgpu and the start fell back",
            RendererStatus {
                preference: Some(Wgpu),
                ..desktop.clone()
            },
            "Status: Off: wgpu did not start here",
        ),
        (
            "--renderer vulkan, with wgpu chosen on the page",
            RendererStatus {
                command_line: Some(Vulkan),
                ..desktop.clone()
            },
            "Status: Off until a restart onto wgpu without --renderer",
        ),
    ];
    for (what, renderers, status) in cases {
        let (mut h, mut state) = client_page(Availability {
            built: true,
            ..Availability::default()
        });
        store::set_value(renderer::RENDERER, PrefValue::Int(renderer::value(Wgpu)));
        state.renderers = renderers;
        h.frame(&state);
        let page = &h.ui.windows.options_page;
        assert!(
            page.control(status).is_some(),
            "{what}: {:?}",
            page.controls
        );
        assert!(
            page.control("Status: Off until the restart onto wgpu")
                .is_none(),
            "{what}"
        );
    }
}

/// An effect's note, shown while the pointer is over its caption, is drawn over the rows under
/// it: after every row of the page, and not cut to the page's list.
#[test]
#[cfg(feature = "hifi")]
fn an_effect_s_note_is_drawn_over_every_row_of_the_page() {
    let (mut h, state) = client_page(Availability {
        built: true,
        wgpu: true,
        widened: true,
        supported: true,
        rays: Some(false),
        active: true,
        failed: None,
        wgpu_next: None,
    });
    // The first box, with the others in the rows under it.
    let o = fidelity::find(names::AMBIENT_OCCLUSION).expect("its box");
    assert!(show(&mut h, &state, o.caption), "the first effect's box");
    let page = &h.ui.windows.options_page;
    let list = page.control("list").expect("the page's list");
    let r = page.control(o.caption).expect("the first effect's box");
    let (x, y) = (list.x + 40.0, r.y + r.h / 2.0);
    h.move_to(x, y);
    h.frame(&state);
    assert_eq!(
        h.ui.windows.tip(),
        Some(&(o.caption.to_owned(), vec![o.note.to_owned()]))
    );
    // Its ground stands beside the pointer.
    let at = h
        .list
        .quads
        .iter()
        .position(|q| {
            q.tex.is_none()
                && (q.dst.x - (x + 18.0)).abs() < 0.5
                && (q.dst.y - (y + 18.0)).abs() < 0.5
        })
        .expect("the note's ground");
    h.assert_over_rows(at, list, "the note");
}

/// An effect's row the list shows only in part shows its note only over the part shown: the
/// pointer under the list, over the Defaults button, shows none.
#[test]
#[cfg(feature = "hifi")]
fn an_effect_s_note_shows_only_over_the_part_of_its_row_the_list_shows() {
    let (mut h, state) = client_page(Availability {
        built: true,
        wgpu: true,
        widened: true,
        supported: true,
        rays: Some(false),
        active: true,
        failed: None,
        wgpu_next: None,
    });
    let o = fidelity::find(names::AMBIENT_OCCLUSION).expect("its box");
    assert!(show(&mut h, &state, o.caption), "the first effect's box");
    let page = &h.ui.windows.options_page;
    let list = page.control("list").expect("the page's list");
    let r = page.control(o.caption).expect("the first effect's box");
    // Scrolled back until the row's top is ten pixels above the list's foot.
    let row_top = r.y - 3.0;
    h.ui.windows.options_page.scroll[tab::DISPLAY] -= list.bottom() - 10.0 - row_top;
    h.frame(&state);
    let r =
        h.ui.windows
            .options_page
            .control(o.caption)
            .expect("the row, in part");
    assert!(
        r.y < list.bottom() && r.bottom() > list.bottom(),
        "{r:?} {list:?}"
    );
    h.move_to(list.x + 40.0, list.bottom() + 8.0);
    h.frame(&state);
    assert_eq!(h.ui.windows.tip(), None);
    h.move_to(list.x + 40.0, list.bottom() - 4.0);
    h.frame(&state);
    assert_eq!(h.ui.windows.tip().map(|(t, _)| t.as_str()), Some(o.caption));
}

/// The Sound tab, every sound on.
fn sound_tab() -> (Harness, GameState) {
    store::init();
    let state = GameState {
        in_world: true,
        name: "Tester".into(),
        ..GameState::default()
    };
    let mut h = Harness::new(Default::default());
    h.frame(&state);
    h.ui.windows.open(WindowId::Options, 0.0);
    h.ui.windows.options_page.tab = tab::SOUND;
    h.frame(&state);
    h.frame(&state);
    (h, state)
}

/// The Sound tab has the Client page's sounds and nothing of the Display tab's, and the Display tab
/// none of its sounds.
#[test]
fn the_client_page_is_a_display_tab_and_a_sound_tab() {
    let (mut h, state) = sound_tab();
    let page = &h.ui.windows.options_page;
    for row in ["Ambient Sounds", "Interface Sounds"] {
        assert!(page.control(row).is_some(), "{row}");
    }
    for row in ["Full Screen", "Interface Scale"] {
        assert!(page.control(row).is_none(), "{row}");
    }
    h.ui.windows.options_page.tab = tab::DISPLAY;
    h.frame(&state);
    h.frame(&state);
    let page = &h.ui.windows.options_page;
    assert!(page.control("Full Screen").is_some());
    assert!(page.control("Ambient Sounds").is_none());
}

/// Defaults on the Sound tab puts back its sounds alone, and on the Display tab everything else.
#[test]
fn each_tab_s_defaults_put_back_its_own_rows() {
    let (mut h, state) = sound_tab();
    let r =
        h.ui.windows
            .options_page
            .control("Defaults")
            .expect("Defaults");
    h.move_to(r.x + 10.0, r.y + r.h / 2.0);
    h.press();
    let mut out = h.frame(&state).requests;
    h.release();
    out.extend(h.frame(&state).requests);
    let named: Vec<&str> = out
        .iter()
        .filter_map(|r| match r {
            UiRequest::SetPreference(n, _) => Some(*n),
            _ => None,
        })
        .collect();
    assert!(named.iter().all(|n| n.starts_with("Sound.")), "{named:?}");
    assert!(!named.is_empty());
}

/// Not a claim: draws the Client page scrolled to the renderer choice and the experimental
/// effects, with the interface's own art, as the wgpu renderer, another renderer, another with
/// wgpu chosen for the next start, another held to itself by `--renderer` with wgpu chosen, and
/// another `--renderer wgpu` fell back to would show it, and as the wgpu renderer with the pointer
/// over the first effect's caption and with the renderer choice's list open, into the PNG files
/// `DERETH_HORIZON_SHOT` names (`<path>-wgpu.png`, `<path>-native.png`,
/// `<path>-native-chosen.png`, `<path>-native-held.png`, `<path>-native-fell-back.png`,
/// `<path>-wgpu-note.png` and `<path>-wgpu-list.png`), for a person to look at. The quads are
/// composited in software: nearest texels, straight alpha.
#[test]
#[cfg(feature = "hifi")]
#[ignore = "an instrument: set DERETH_HORIZON_SHOT to an output path without its extension"]
fn draw_the_effects_section_for_a_person_to_look_at() {
    use dereth_horizon::art::Art;
    use dereth_horizon::draw::DrawList;
    use dereth_horizon::ui::input::InputFrame;
    use dereth_horizon::ui::HorizonUi;
    use std::sync::Arc;
    /// Where the pointer is when the picture is taken.
    #[derive(Clone, Copy)]
    enum Pointer {
        Away,
        /// Over the first effect's caption, its note shown.
        OverNote,
        /// The renderer choice's list opened.
        ListOpen,
    }
    let Some(path) = std::env::var_os("DERETH_HORIZON_SHOT") else {
        return;
    };
    let path = std::path::PathBuf::from(path);
    let wgpu = Availability {
        built: true,
        wgpu: true,
        widened: true,
        supported: true,
        rays: Some(false),
        active: true,
        failed: None,
        wgpu_next: None,
    };
    let native = Availability {
        wgpu: false,
        widened: false,
        supported: false,
        rays: None,
        active: false,
        ..wgpu.clone()
    };
    use dereth_client_contract::options::renderer::{self, RendererStatus};
    use dereth_client_contract::RendererChoice::{Vulkan, Wgpu};
    let on = |running| RendererStatus {
        offered: vec![Vulkan, Wgpu],
        running: Some(running),
        ..RendererStatus::default()
    };
    let held = RendererStatus {
        command_line: Some(Vulkan),
        ..on(Vulkan)
    };
    let fell_back = RendererStatus {
        command_line: Some(Wgpu),
        ..on(Vulkan)
    };
    for (suffix, hifi, renderers, chosen, pointer) in [
        ("wgpu", wgpu.clone(), on(Wgpu), Some(Wgpu), Pointer::Away),
        ("native", native.clone(), on(Vulkan), None, Pointer::Away),
        (
            "native-chosen",
            native.clone(),
            on(Vulkan),
            Some(Wgpu),
            Pointer::Away,
        ),
        (
            "native-held",
            native.clone(),
            held,
            Some(Wgpu),
            Pointer::Away,
        ),
        (
            "native-fell-back",
            native,
            fell_back,
            Some(Wgpu),
            Pointer::Away,
        ),
        (
            "wgpu-note",
            wgpu.clone(),
            on(Wgpu),
            Some(Wgpu),
            Pointer::OverNote,
        ),
        ("wgpu-list", wgpu, on(Wgpu), Some(Wgpu), Pointer::ListOpen),
    ] {
        store::init();
        store::set_value(names::LIGHTING, PrefValue::Bool(true));
        store::set_value(names::SHADOWS, PrefValue::Bool(true));
        store::set_value(names::AMBIENT_OCCLUSION, PrefValue::Bool(true));
        if let Some(c) = chosen {
            store::set_value(renderer::RENDERER, PrefValue::Int(renderer::value(c)));
        }
        let state = GameState {
            in_world: true,
            name: "Tester".into(),
            hifi,
            renderers,
            ..GameState::default()
        };
        let art = Arc::new(Art::new(Arc::new(
            dereth_horizon::pieces::Pieces::built_in().expect("the pieces built in"),
        )));
        let mut h = Harness {
            ui: HorizonUi::new(Arc::clone(&art), Default::default()),
            list: DrawList::default(),
            input: InputFrame::default(),
            screen: (1920.0, 1080.0),
        };
        h.frame(&state);
        h.ui.windows.open(WindowId::Options, 0.0);
        h.ui.windows.options_page.tab = tab::DISPLAY;
        for _ in 0..3 {
            h.frame(&state);
        }
        h.ui.windows.options_page.scroll[tab::DISPLAY] = 1.0e6;
        for _ in 0..30 {
            h.frame(&state);
        }
        let page = &h.ui.windows.options_page;
        let list = page.control("list").expect("the page's list");
        match pointer {
            Pointer::Away => {}
            Pointer::OverNote => {
                let r = page
                    .control(fidelity::OPTIONS[fidelity::OPTIONS.len() - 1].caption)
                    .expect("the last effect's box");
                h.move_to(list.x + 120.0, r.y + r.h / 2.0);
            }
            Pointer::ListOpen => {
                let r = page
                    .control(renderer::CAPTION)
                    .expect("the renderer choice");
                h.move_to(r.x + 10.0, r.y + r.h / 2.0);
                h.press();
                h.frame(&state);
                h.release();
            }
        }
        h.frame(&state);
        let (w, hgt) = (1920usize, 1080usize);
        let mut px = vec![[24.0f32, 26.0, 30.0]; w * hgt];
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
            for y in (r.y.max(0.0) as usize)..(r.bottom().min(hgt as f32).max(0.0) as usize) {
                for x in (r.x.max(0.0) as usize)..(r.right().min(w as f32).max(0.0) as usize) {
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
                    let p = &mut px[y * w + x];
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
        let out = path.with_file_name(format!(
            "{}-{suffix}.png",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("horizon")
        ));
        let file = std::fs::File::create(&out).expect("the picture's file");
        #[allow(clippy::cast_possible_truncation)]
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, hgt as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()
            .and_then(|mut wr| wr.write_image_data(&rgb))
            .expect("the picture writes");
        eprintln!("wrote {}", out.display());
    }
}
