//! How far an art pack's trees and ground cover are drawn: two experimental menus after the
//! effects' boxes, each a share of the distance they are drawn to with nothing changed, and the
//! ground cover's also Off. They are registered with the boxes ([`super::fidelity::register`]);
//! the profile keeps a share the player chose, and leaves out one still at its default
//! ([`super::fidelity::left_out_of_save`]).

use crate::options::names::fidelity::{GROUND_COVER_DRAW_DISTANCE, TREE_DRAW_DISTANCE};
use crate::view::PrefValue;

/// The trees' menu's caption.
pub const TREE_CAPTION: &str = "Tree Draw Distance";

/// What the trees' menu does, for the row's tooltip.
pub const TREE_NOTE: &str = "How far an art pack's trees are drawn, against the distance they are \
                             drawn to at 1x. Past 1x they are planted in farther blocks, as \
                             far as the landscape is drawn, in less detail and with no shadows, \
                             and the higher shares cost the most. It changes at once.";

/// The ground cover's menu's caption.
pub const GROUND_COVER_CAPTION: &str = "Ground Cover";

/// What the ground cover's menu does, for the row's tooltip.
pub const GROUND_COVER_NOTE: &str =
    "The grass and low plants an art pack lays over the land, and how far they are drawn, \
     against the distance they are drawn to at 1x; Off leaves them out. Past 1x they stay as \
     thick most of the way out and thin only toward the end, and the higher shares cost the \
     most. They are pictures alone: nothing collides with them. It changes at once.";

/// The shares a menu offers, in percent, nearest first.
pub const CHOICES: [u32; 8] = [50, 75, 100, 150, 200, 300, 400, 500];

/// The distance the pack draws to with nothing changed.
pub const WHOLE: u32 = 100;

/// The trees' share until the player changes it, as the Medium preset has it.
pub const TREE_DEFAULT: i32 = 150;

/// The ground cover's share until the player changes it: the whole distance.
pub const GROUND_COVER_DEFAULT: i32 = 100;

/// The share `name`'s menu holds until the player changes it.
#[must_use]
pub fn default_of(name: &str) -> i32 {
    if name.eq_ignore_ascii_case(TREE_DRAW_DISTANCE) {
        TREE_DEFAULT
    } else {
        GROUND_COVER_DEFAULT
    }
}

/// The ground cover's choice that leaves it out: no share, so no share is mistaken for it.
pub const OFF: i32 = 1;

/// Its words.
const OFF_WORD: &str = "Off";

/// The least share.
pub const LEAST: u32 = 50;

/// The greatest share.
pub const MOST: u32 = 500;

/// Whether `name` is one of the two menus.
#[must_use]
pub fn is_a_distance(name: &str) -> bool {
    name.eq_ignore_ascii_case(TREE_DRAW_DISTANCE)
        || name.eq_ignore_ascii_case(GROUND_COVER_DRAW_DISTANCE)
}

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

/// `share`'s words, marked where it is `name`'s default.
fn label(name: &str, share: u32) -> String {
    if i32::try_from(share).is_ok_and(|s| s == default_of(name)) {
        format!("{} (Default)", times(share))
    } else {
        times(share)
    }
}

/// The share in percent a menu's words stand for: a multiple with or without its mark, or a
/// percent with or without its sign.
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

/// The value a word stands for: a share between [`LEAST`] and [`MOST`], or the ground cover's
/// Off. `None` for another name or another word, which leaves the option as it was.
#[must_use]
pub fn parse_value(name: &str, raw: &str) -> Option<i32> {
    if !is_a_distance(name) {
        return None;
    }
    if name.eq_ignore_ascii_case(GROUND_COVER_DRAW_DISTANCE)
        && raw.trim().eq_ignore_ascii_case(OFF_WORD)
    {
        return Some(OFF);
    }
    let share = parse_share(raw)?;
    i32::try_from(share)
        .ok()
        .filter(|_| (LEAST..=MOST).contains(&share))
}

/// The words for a value: the share as a multiple, or Off. `None` for any other name.
#[must_use]
pub fn convert_to_string(name: &str, v: &PrefValue) -> Option<String> {
    if !is_a_distance(name) {
        return None;
    }
    match v {
        PrefValue::Int(OFF) if name.eq_ignore_ascii_case(GROUND_COVER_DRAW_DISTANCE) => {
            Some(OFF_WORD.to_owned())
        }
        PrefValue::Int(i) => u32::try_from(*i).ok().map(|s| label(name, s)),
        _ => None,
    }
}

/// The shares a menu offers ([`CHOICES`]), each as its words, after the ground cover's Off;
/// `None` for any other name.
#[must_use]
pub fn choice_rows(name: &str) -> Option<Vec<super::store::Choice>> {
    if !is_a_distance(name) {
        return None;
    }
    let off = name
        .eq_ignore_ascii_case(GROUND_COVER_DRAW_DISTANCE)
        .then(|| super::store::Choice {
            label: OFF_WORD.to_owned(),
            value: OFF,
        });
    Some(
        off.into_iter()
            .chain(CHOICES.iter().map(|&share| super::store::Choice {
                label: label(name, share),
                value: i32::try_from(share).unwrap_or(GROUND_COVER_DEFAULT),
            }))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    //! Behaviour: none (experimental hi-fi art packs)
    use super::*;

    #[test]
    fn every_menu_offers_half_to_five_times_the_distance_with_the_whole_among_them() {
        for name in [TREE_DRAW_DISTANCE, GROUND_COVER_DRAW_DISTANCE] {
            let rows = choice_rows(name).expect("a menu");
            let shares: Vec<i32> = rows.iter().map(|c| c.value).collect();
            // The ground cover's menu leaves it out first.
            let off = usize::from(name == GROUND_COVER_DRAW_DISTANCE);
            if off == 1 {
                assert_eq!((rows[0].label.as_str(), rows[0].value), ("Off", OFF));
            }
            assert_eq!(shares[off..], [50, 75, 100, 150, 200, 300, 400, 500]);
            // The trees hold Medium's 1.5x until changed, the ground cover 1x.
            let default = if off == 1 { 2 } else { 3 };
            assert!(rows[default + off].label.ends_with("(Default)"), "{name}");
        }
        assert!(choice_rows("Fidelity.GodRays").is_none());
    }

    #[test]
    fn a_share_is_read_with_or_without_its_sign_and_refused_outside_the_menu_range() {
        assert_eq!(parse_value(TREE_DRAW_DISTANCE, " 300 % "), Some(300));
        assert_eq!(parse_value(TREE_DRAW_DISTANCE, "1.5x"), Some(150));
        assert_eq!(parse_value(TREE_DRAW_DISTANCE, "1x (Default)"), Some(100));
        assert_eq!(parse_value(TREE_DRAW_DISTANCE, "6x"), None);
        assert_eq!(parse_value(GROUND_COVER_DRAW_DISTANCE, "50"), Some(50));
        assert_eq!(parse_value(GROUND_COVER_DRAW_DISTANCE, "off"), Some(OFF));
        assert_eq!(parse_value(TREE_DRAW_DISTANCE, "off"), None);
        assert_eq!(parse_value(TREE_DRAW_DISTANCE, "49"), None);
        assert_eq!(parse_value(TREE_DRAW_DISTANCE, "501"), None);
        assert_eq!(parse_value(TREE_DRAW_DISTANCE, "far"), None);
        assert_eq!(parse_value("Fidelity.GodRays", "100"), None);
        assert_eq!(
            convert_to_string(TREE_DRAW_DISTANCE, &PrefValue::Int(150)).as_deref(),
            Some("1.5x (Default)")
        );
        assert_eq!(
            convert_to_string(GROUND_COVER_DRAW_DISTANCE, &PrefValue::Int(OFF)).as_deref(),
            Some("Off")
        );
    }
}
