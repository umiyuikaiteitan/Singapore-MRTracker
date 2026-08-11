//! URL-friendly station aliases.
//!
//! A station carries one or more official codes (`NS1`, `EW24`) and a
//! public name (`Jurong East`). An application that puts a station in
//! a URL wants both forms to work, in whatever spelling a visitor
//! types. Two operations make that possible:
//!
//! - [`slug`] builds the canonical, readable alias of a name:
//!   `Jurong East` becomes `jurong-east`.
//! - [`normalize`] reduces any spelling to a comparison key: `NS1`,
//!   `ns-1`, and `NS 1` all become `ns1`, and `Jurong East`,
//!   `jurong-east`, and `JurongEast` all become `jurongeast`.
//!
//! [`RailNetwork::station_by_alias`] resolves a normalized alias to a
//! station, and [`RailNetwork::station_aliases`] lists every alias the
//! network accepts, so a browser can resolve the same spellings
//! offline.
//!
//! [`RailNetwork::station_by_alias`]: crate::RailNetwork::station_by_alias
//! [`RailNetwork::station_aliases`]: crate::RailNetwork::station_aliases

/// Build the canonical URL alias of a station name.
///
/// The alias keeps ASCII letters and digits, lowercases them, and
/// joins the remaining groups with single hyphens.
///
/// ```
/// use mrt_gtfs::alias;
///
/// assert_eq!(alias::slug("Jurong East"), "jurong-east");
/// assert_eq!(alias::slug("HarbourFront"), "harbourfront");
/// assert_eq!(alias::slug("one-north"), "one-north");
/// ```
pub fn slug(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Reduce any spelling of a code or a name to a comparison key.
///
/// The key keeps ASCII letters and digits only, in lower case, so
/// spacing and punctuation never decide whether a URL resolves.
///
/// ```
/// use mrt_gtfs::alias;
///
/// assert_eq!(alias::normalize("NS 1"), "ns1");
/// assert_eq!(alias::normalize("jurong-east"), "jurongeast");
/// assert_eq!(alias::normalize("Jurong East"), "jurongeast");
/// ```
pub fn normalize(text: &str) -> String {
    text.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_lowercase_and_hyphenated() {
        assert_eq!(slug("Jurong East"), "jurong-east");
        assert_eq!(slug("Choa Chu Kang"), "choa-chu-kang");
        assert_eq!(slug("HarbourFront"), "harbourfront");
        assert_eq!(slug("one-north"), "one-north");
    }

    #[test]
    fn slugs_collapse_and_trim_separators() {
        assert_eq!(slug("  Marina   Bay  "), "marina-bay");
        assert_eq!(slug("Bayfront (CE1)"), "bayfront-ce1");
        assert_eq!(slug("!!!"), "");
    }

    #[test]
    fn normalization_ignores_case_and_punctuation() {
        assert_eq!(normalize("NS1"), "ns1");
        assert_eq!(normalize("ns-1"), "ns1");
        assert_eq!(normalize("NS 1"), "ns1");
        assert_eq!(normalize("Jurong East"), normalize("jurong-east"));
        assert_eq!(normalize("Jurong East"), normalize("JurongEast"));
    }

    #[test]
    fn a_slug_normalizes_to_the_key_of_its_name() {
        for name in ["Jurong East", "Choa Chu Kang", "one-north", "Marina Bay"] {
            assert_eq!(normalize(&slug(name)), normalize(name));
        }
    }
}
