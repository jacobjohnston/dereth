//! The render scale: the share of its screen's own pixels the client draws while it fills the
//! screen, or on a Mac whenever Horizon shows. Fewer pixels draw faster, and the picture, enlarged
//! to fill the screen, looks softer.
//!
//! It is this client's own option, not a retail one, and only a host that can draw the game at
//! fewer pixels than its screen and enlarge the picture offers it: the browser, whose page is the
//! screen in full screen, and a window that goes by points. Such a host registers the option as
//! the client starts ([`register`]), at the share it starts at. A host that does not offer it
//! never registers it, and the options pages leave its row out ([`super::sheet::Row::hosted`]).
//! Every interface that shows the row shows it under Display, after Full Screen; the host reads
//! the share from the store every frame ([`percent`]), so a change applies at once. In a browser
//! the front page keeps the choice, and the host hands a change made on a page back to it, so the
//! profile does not keep it; a desktop window has no front page, and the profile does
//! ([`left_out_of_save`]).
//!
//! The enhanced graphics' own render scale (`Fidelity.RenderScale`, [`super::fidelity`]) offers
//! the same shares and Dynamic ([`DYNAMIC`]), reads the same words ([`is_a_scale`]), and the profile
//! keeps it.

use crate::options::names::fidelity::RENDER_SCALE as FIDELITY_RENDER_SCALE;
use crate::view::PrefValue;

/// `[Display] RenderScale`: the share of the screen's own pixels drawn while the client fills
/// the screen, in percent.
pub const RENDER_SCALE: &str = crate::options::names::DISPLAY_RENDER_SCALE;

/// The option's caption, literal text: no string table has one.
pub const CAPTION: &str = "Render Scale";

/// The shares a page offers, in percent, the whole screen first.
pub const CHOICES: [u32; 5] = [100, 85, 75, 67, 50];

/// All of the screen's pixels.
pub const ALL: u32 = 100;

/// The enhanced graphics' choice that moves the render scale to hold a frame rate (see
/// [`super::dynamic_resolution`]): no share, so no share is mistaken for it.
pub const DYNAMIC: u32 = 1;

/// Its words.
const DYNAMIC_WORD: &str = "Dynamic";

/// Dynamic's words in the menu, marked where it is the default.
fn dynamic_label() -> String {
    if super::fidelity::SCALE_DEFAULT == DYNAMIC {
        format!("{DYNAMIC_WORD} (Default)")
    } else {
        DYNAMIC_WORD.to_owned()
    }
}

/// The least share drawn: half the screen's pixels each way.
pub const LEAST: u32 = 50;

/// A share in percent as a multiple: `0.5x`, `1x`, `1.5x`.
fn times(share: u32) -> String {
    let whole = share / 100;
    let part = share % 100;
    if part == 0 {
        format!("{whole}x")
    } else if part.is_multiple_of(10) {
        format!("{whole}.{}x", part / 10)
    } else {
        format!("{whole}.{part:02}x")
    }
}

/// `share`'s words in the profile, marked where it is the enhanced graphics' default.
fn label(share: u32) -> String {
    if share == super::fidelity::SCALE_DEFAULT {
        format!("{} (Default)", times(share))
    } else {
        times(share)
    }
}

/// The enhanced graphics' words for a share in their menu: percent, the whole frame named and the
/// default marked.
fn percent_label(share: u32) -> String {
    match (share == ALL, share == super::fidelity::SCALE_DEFAULT) {
        (true, true) => "100% (Native, Default)".to_owned(),
        (true, false) => "100% (Native)".to_owned(),
        (false, true) => format!("{share}% (Default)"),
        (false, false) => format!("{share}%"),
    }
}

/// The share in percent a word stands for: a multiple with or without its mark, or a percent with
/// or without its sign.
fn parse_share(raw: &str) -> Option<u32> {
    let word = raw.trim();
    let word = word.split(" (").next().unwrap_or(word).trim_end();
    if let Some(many) = word.strip_suffix(['x', 'X']) {
        let many: f32 = many.trim_end().parse().ok()?;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        // LINT-OK: a menu's multiple, a small positive number, brought to percent and rounded.
        return (many.is_finite() && many > 0.0).then_some((many * 100.0 + 0.5) as u32);
    }
    word.trim_end_matches('%').trim_end().parse().ok()
}

/// Whether `name` is the enhanced graphics' render scale.
fn is_fidelity(name: &str) -> bool {
    name.eq_ignore_ascii_case(FIDELITY_RENDER_SCALE)
}

/// Whether `name` is one of the two preferences that hold a share.
#[must_use]
pub fn is_a_scale(name: &str) -> bool {
    name.eq_ignore_ascii_case(RENDER_SCALE) || is_fidelity(name)
}

/// `share`, kept between [`LEAST`] and [`ALL`].
const fn kept(share: u32) -> u32 {
    if share < LEAST {
        LEAST
    } else if share > ALL {
        ALL
    } else {
        share
    }
}

std::thread_local! {
    /// Whether the profile keeps the option, for a host with nowhere else to keep it.
    static IN_PROFILE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// [`register`], for a host that leaves the choice to the profile: it registers the option before
/// the profile is read.
pub fn register_in_profile(start: u32) -> usize {
    let registered = register(start);
    IN_PROFILE.set(true);
    registered
}

/// Register the option in the option value store at `start` percent (kept between [`LEAST`] and
/// [`ALL`]): what a host that offers it does as the client starts. A store that already has it
/// keeps what it holds.
pub fn register(start: u32) -> usize {
    IN_PROFILE.set(false);
    usize::from(super::store::register_preference(
        RENDER_SCALE,
        PrefValue::Int(i32::try_from(kept(start)).unwrap_or(100)),
        super::store::DataType::UInt,
    ))
}

/// Whether the host offers the option: whether it registered it.
#[must_use]
pub fn offered() -> bool {
    super::store::is_registered(RENDER_SCALE)
}

/// The share of the screen's pixels to draw while the client fills the screen, in percent: the
/// store's, kept between [`LEAST`] and [`ALL`], or all of them where the host does not offer the
/// option.
#[must_use]
pub fn percent() -> u32 {
    match super::store::inq_value(RENDER_SCALE) {
        Some(PrefValue::Int(v)) => u32::try_from(v).map_or(LEAST, kept),
        _ => ALL,
    }
}

/// The value a word stands for: a share in percent, with or without its sign, or as a multiple,
/// between [`LEAST`] and [`ALL`], or the enhanced graphics' Dynamic. `None` for another name or
/// another word, which leaves the option as it was.
#[must_use]
pub fn parse_value(name: &str, raw: &str) -> Option<i32> {
    if !is_a_scale(name) {
        return None;
    }
    if is_fidelity(name)
        && raw
            .trim()
            .get(..DYNAMIC_WORD.len())
            .is_some_and(|w| w.eq_ignore_ascii_case(DYNAMIC_WORD))
    {
        return i32::try_from(DYNAMIC).ok();
    }
    let share = parse_share(raw)?;
    (LEAST..=ALL)
        .contains(&share)
        .then(|| i32::try_from(share).ok())
        .flatten()
}

/// The words for a value: the share in percent, or for the enhanced graphics as a multiple, or
/// Dynamic. `None` for any other name.
#[must_use]
pub fn convert_to_string(name: &str, v: &PrefValue) -> Option<String> {
    if !is_a_scale(name) {
        return None;
    }
    match v {
        PrefValue::Int(i) if is_fidelity(name) => u32::try_from(*i).ok().map(|s| {
            if s == DYNAMIC {
                dynamic_label()
            } else {
                label(s)
            }
        }),
        PrefValue::Int(i) => Some(format!("{i}%")),
        _ => None,
    }
}

/// The shares a page offers ([`CHOICES`]), each as its words, after the enhanced graphics'
/// Dynamic; `None` for any other name.
#[must_use]
pub fn choice_rows(name: &str) -> Option<Vec<super::store::Choice>> {
    if !is_a_scale(name) {
        return None;
    }
    let fidelity = is_fidelity(name);
    let dynamic = fidelity.then(|| super::store::Choice {
        label: dynamic_label(),
        value: i32::try_from(DYNAMIC).unwrap_or(1),
    });
    Some(
        dynamic
            .into_iter()
            .chain(CHOICES.iter().map(|&share| super::store::Choice {
                label: if fidelity {
                    percent_label(share)
                } else {
                    format!("{share}%")
                },
                value: i32::try_from(share).unwrap_or(100),
            }))
            .collect(),
    )
}

/// Whether the store's save leaves `name` out of the profile: the render scale, where the
/// browser's front page keeps it.
#[must_use]
pub fn left_out_of_save(name: &str) -> bool {
    name.eq_ignore_ascii_case(RENDER_SCALE) && !IN_PROFILE.get()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::interface::Interface;
    use crate::options::{sheet, store};

    /// The Client Options page of `face`, as the preferences of its rows in order.
    fn client_page(face: Interface) -> Vec<&'static str> {
        sheet::rows_for(sheet::PageId::Client, face)
            .filter_map(sheet::Row::preference)
            .collect()
    }

    /// A host that offers the render scale registers it at the share it starts at, and from then
    /// every interface's Client Options page has the row, Render Scale, under Display after Full
    /// Screen, with the five shares read as their percent alone and no note, all of the screen
    /// restored by Defaults. A host that does not offer it has
    /// no such row on any page, and draws all of the screen.
    ///
    /// Behaviour: window.full-screen.in-a-browser-the-render-scale-is-a-client-option
    #[test]
    fn a_host_that_offers_the_render_scale_has_its_row_on_every_interface_s_page() {
        store::init();
        assert!(!offered());
        assert_eq!(percent(), ALL);
        for face in [Interface::Modern, Interface::Classic, Interface::Horizon] {
            assert!(!client_page(face).contains(&RENDER_SCALE), "{face:?}");
            assert!(sheet::defaults(sheet::PageId::Client, face)
                .iter()
                .all(|(name, _)| *name != RENDER_SCALE));
        }

        assert_eq!(register(75), 1);
        assert!(offered());
        assert_eq!(percent(), 75);
        for face in [Interface::Modern, Interface::Classic, Interface::Horizon] {
            let page = client_page(face);
            let at = page.iter().position(|p| *p == RENDER_SCALE);
            let full_screen = page
                .iter()
                .position(|p| *p == crate::options::names::DISPLAY_FULL_SCREEN);
            assert_eq!(at, full_screen.map(|i| i + 1), "{face:?}: {page:?}");
            assert!(sheet::defaults(sheet::PageId::Client, face).contains(&(
                RENDER_SCALE,
                crate::options::config::PrefValueConst::Int(100)
            )));
        }
        let row = sheet::row_of_preference(sheet::PageId::Client, RENDER_SCALE).unwrap();
        assert_eq!(row.caption, "Render Scale");
        assert_eq!(row.note, None, "the choices say it all");
        assert_eq!(row.value, sheet::Value::Menu(RENDER_SCALE));
        let shares: Vec<(String, i32)> = store::choice_rows(RENDER_SCALE)
            .unwrap()
            .into_iter()
            .map(|c| (c.label, c.value))
            .collect();
        assert_eq!(
            shares,
            [
                ("100%".to_owned(), 100),
                ("85%".to_owned(), 85),
                ("75%".to_owned(), 75),
                ("67%".to_owned(), 67),
                ("50%".to_owned(), 50)
            ]
        );
        store::clear();
    }

    /// The share is the store's from the next read, kept between half and all of the screen, and
    /// a second registration keeps what the store holds.
    ///
    /// Behaviour: window.full-screen.in-a-browser-the-render-scale-is-a-client-option
    #[test]
    fn the_share_is_the_store_s_and_never_below_half_or_above_all() {
        store::init();
        assert_eq!(register(30), 1);
        assert_eq!(percent(), LEAST, "a start below half starts at half");
        assert!(store::set_value(RENDER_SCALE, PrefValue::Int(85)));
        assert_eq!(percent(), 85);
        assert_eq!(register(100), 0);
        assert_eq!(percent(), 85, "a second registration keeps the choice");
        assert!(store::set_value(RENDER_SCALE, PrefValue::Int(250)));
        assert_eq!(percent(), ALL);
        store::clear();
        assert_eq!(percent(), ALL);
    }

    /// The front page keeps the choice, so the profile does not: the save leaves the option out,
    /// and a profile that names it does not change it.
    ///
    /// Behaviour: window.full-screen.in-a-browser-the-render-scale-is-a-client-option
    #[test]
    fn the_profile_neither_keeps_nor_changes_the_render_scale() {
        store::init();
        register(67);
        let text = store::save().to_text();
        assert!(!text.contains("RenderScale"), "{text}");
        let ini =
            crate::persist::preferences::UserPreferences::parse("[Display]\r\nRenderScale=50\r\n")
                .unwrap();
        store::init();
        store::load(&ini);
        register(67);
        assert_eq!(percent(), 67);
        store::clear();
    }

    /// A desktop window has no front page, so its profile keeps the choice.
    ///
    /// Behaviour: presentation.window.a-mac-window-counts-its-size-in-points
    #[test]
    fn a_host_with_no_front_page_keeps_the_render_scale_in_the_profile() {
        store::init();
        register_in_profile(ALL);
        let ini =
            crate::persist::preferences::UserPreferences::parse("[Display]\r\nRenderScale=50\r\n")
                .unwrap();
        store::load(&ini);
        assert_eq!(percent(), 50);
        let text = store::save().to_text();
        assert!(text.contains("RenderScale=50"), "{text}");
        store::clear();
    }

    /// The words the option reads and writes are its share in percent.
    ///
    /// Behaviour: none (the option's own spelling).
    #[test]
    fn a_share_reads_and_writes_as_its_percent() {
        assert_eq!(parse_value(RENDER_SCALE, "75"), Some(75));
        assert_eq!(parse_value(RENDER_SCALE, " 67 % "), Some(67));
        assert_eq!(parse_value(RENDER_SCALE, "40"), None);
        assert_eq!(parse_value(RENDER_SCALE, "big"), None);
        assert_eq!(parse_value("Display.FullScreen", "75"), None);
        assert_eq!(
            convert_to_string(RENDER_SCALE, &PrefValue::Int(85)).as_deref(),
            Some("85%")
        );
        assert_eq!(convert_to_string("Other.Name", &PrefValue::Int(85)), None);
    }

    /// The enhanced graphics' render scale offers Dynamic before the shares, reads a multiple or
    /// a percent, and writes a multiple, or Dynamic, which is its default and reads back.
    ///
    /// Behaviour: none (the experimental render scale's spelling).
    #[test]
    fn the_enhanced_render_scale_reads_a_multiple_or_a_percent_and_offers_dynamic() {
        let fidelity = FIDELITY_RENDER_SCALE;
        assert!(is_a_scale(fidelity) && is_a_scale(RENDER_SCALE));
        assert_eq!(parse_value(fidelity, "0.5x"), Some(50));
        assert_eq!(parse_value(fidelity, "75%"), Some(75));
        assert_eq!(parse_value(RENDER_SCALE, "0.75x"), Some(75));
        assert_eq!(
            parse_value(RENDER_SCALE, "Dynamic"),
            None,
            "the browser's has none"
        );
        assert_eq!(
            convert_to_string(fidelity, &PrefValue::Int(50)).as_deref(),
            Some("0.5x")
        );
        assert_eq!(
            convert_to_string(fidelity, &PrefValue::Int(100)).as_deref(),
            Some("1x")
        );
        let dynamic = convert_to_string(fidelity, &PrefValue::Int(1)).unwrap();
        assert_eq!(dynamic, "Dynamic (Default)");
        assert_eq!(parse_value(fidelity, &dynamic), Some(1));
        let rows: Vec<(String, i32)> = choice_rows(fidelity)
            .unwrap()
            .into_iter()
            .map(|c| (c.label, c.value))
            .collect();
        assert_eq!(
            rows,
            [
                ("Dynamic (Default)".to_owned(), 1),
                ("100% (Native)".to_owned(), 100),
                ("85%".to_owned(), 85),
                ("75%".to_owned(), 75),
                ("67%".to_owned(), 67),
                ("50%".to_owned(), 50)
            ]
        );
    }
}
