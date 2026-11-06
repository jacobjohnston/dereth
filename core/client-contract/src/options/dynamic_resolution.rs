//! Dynamic Resolution's frame rate: the frame rate the render scale moves to hold while Render
//! Scale is Dynamic ([`super::render_scale::DYNAMIC`]), a menu the enhanced graphics show only then.

use crate::options::names::fidelity::DYNAMIC_RESOLUTION;
use crate::view::PrefValue;

/// The menu's caption.
pub const CAPTION: &str = "Dynamic Resolution Frame Rate";

/// What the menu does, for the row's tooltip.
pub const NOTE: &str = "The frame rate a Dynamic render scale holds: it lowers the render scale \
                        when the frame rate falls under this, and raises it again, up to 100%, \
                        when there is room.";

/// The frame rates offered.
pub const CHOICES: [u32; 4] = [30, 60, 90, 120];

/// The frame rate until the player chooses one.
pub const DEFAULT: i32 = 60;

/// Whether `name` is the menu.
#[must_use]
pub fn is_dynamic_resolution(name: &str) -> bool {
    name.eq_ignore_ascii_case(DYNAMIC_RESOLUTION)
}

fn label(rate: u32) -> String {
    format!("{rate} FPS")
}

/// The value a word stands for. `None` for another name or another word.
#[must_use]
pub fn parse_value(name: &str, raw: &str) -> Option<i32> {
    if !is_dynamic_resolution(name) {
        return None;
    }
    let raw = raw.trim();
    let rate: u32 = raw
        .trim_end_matches(|c: char| !c.is_ascii_digit())
        .parse()
        .ok()?;
    CHOICES
        .contains(&rate)
        .then(|| i32::try_from(rate).ok())
        .flatten()
}

/// The words for a value. `None` for any other name.
#[must_use]
pub fn convert_to_string(name: &str, v: &PrefValue) -> Option<String> {
    if !is_dynamic_resolution(name) {
        return None;
    }
    match v {
        PrefValue::Int(i) => u32::try_from(*i).ok().map(label),
        _ => None,
    }
}

/// The menu's choices; `None` for any other name.
#[must_use]
pub fn choice_rows(name: &str) -> Option<Vec<super::store::Choice>> {
    if !is_dynamic_resolution(name) {
        return None;
    }
    Some(
        CHOICES
            .iter()
            .map(|&rate| super::store::Choice {
                label: label(rate),
                value: i32::try_from(rate).unwrap_or(0),
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    //! Behaviour: none (experimental hi-fi render scale)
    use super::*;

    #[test]
    fn the_menu_reads_back_what_it_writes() {
        for rate in CHOICES {
            let v = PrefValue::Int(i32::try_from(rate).unwrap());
            let words = convert_to_string(DYNAMIC_RESOLUTION, &v).unwrap();
            assert_eq!(
                parse_value(DYNAMIC_RESOLUTION, &words),
                Some(i32::try_from(rate).unwrap())
            );
        }
        assert_eq!(parse_value(DYNAMIC_RESOLUTION, "45 FPS"), None);
        assert_eq!(parse_value("Fidelity.Other", "30"), None);
    }
}
