//! AI Player names: about a thousand, made by the 1999 creation screens' naming help.

/// One name: its heritage, its gender (1 male, 2 female) and the name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Name {
    pub heritage: u32,
    pub gender: u32,
    pub name: &'static str,
}

/// Every name, in the file's order.
pub fn all() -> &'static [Name] {
    static ALL: std::sync::OnceLock<Vec<Name>> = std::sync::OnceLock::new();
    ALL.get_or_init(|| parse(include_str!("names.txt")))
}

fn parse(text: &'static str) -> Vec<Name> {
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let mut parts = l.splitn(3, ' ');
            Some(Name {
                heritage: parts.next()?.parse().ok()?,
                gender: parts.next()?.parse().ok()?,
                name: parts.next()?.trim(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_names_are_many_unique_and_creatable() {
        let names = all();
        assert!(names.len() >= 1000);
        let mut seen = std::collections::HashSet::new();
        for n in names {
            assert!((1..=3).contains(&n.heritage) && (1..=2).contains(&n.gender));
            assert!(dereth_rules::chargen::name_length_ok(n.name), "{}", n.name);
            assert!(seen.insert(n.name.to_lowercase()), "{} twice", n.name);
            assert!(n
                .name
                .chars()
                .all(|c| c.is_ascii_alphabetic() || matches!(c, ' ' | '\'' | '-')));
        }
    }
}
