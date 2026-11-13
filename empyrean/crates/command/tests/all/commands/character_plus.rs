//! Not ACE: set-characterplus puts the + before a character's name or takes it off, and god mode
//! takes a character of any era, whose skill list is shorter than the end of retail's.

use empyrean_command::empyrean::character_plus::parse;
use empyrean_command::handlers::admin_commands::god_state_is_well_formed;

fn words(s: &str) -> Vec<String> {
    s.split(' ').map(str::to_owned).collect()
}

#[test]
fn set_characterplus_reads_the_name_and_an_optional_on_or_off() {
    assert_eq!(parse(&words("Ethangar")), ("Ethangar".to_owned(), true));
    assert_eq!(
        parse(&words("Ethangar off")),
        ("Ethangar".to_owned(), false)
    );
    assert_eq!(
        parse(&words("Old Man Ross ON")),
        ("Old Man Ross".to_owned(), true)
    );
    assert_eq!(
        parse(&words("+Ethangar off")),
        ("Ethangar".to_owned(), false)
    );
    // a one-word name is a name, even one spelled like the flag
    assert_eq!(parse(&words("Off")), ("Off".to_owned(), true));
}

#[test]
fn god_mode_takes_any_eras_skill_list() {
    // 50 entries before the skills, 5 for each skill
    assert!(god_state_is_well_formed(240), "end of retail, 38 skills");
    assert!(god_state_is_well_formed(195), "February 2005, 29 skills");
    assert!(god_state_is_well_formed(50), "no skills");
    assert!(!god_state_is_well_formed(49));
    assert!(!god_state_is_well_formed(196));
}
