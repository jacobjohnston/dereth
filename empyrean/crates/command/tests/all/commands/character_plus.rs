//! Not ACE: set-characterplus puts the + before a character's name or takes it off.

use empyrean_command::empyrean::character_plus::parse;

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
