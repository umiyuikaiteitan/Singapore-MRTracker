//! URL-friendly station aliases.
//!
//! A station carries one or more official codes: `NS1`, `EW24`. An
//! application that puts a station in a URL wants every code to work,
//! in whatever spelling a visitor types. [`normalize`] reduces a
//! spelling to a comparison key, so `NS1`, `ns-1`, and `NS 1` all
//! name the same station.
//!
//! Codes alone make the alias table, because a code identifies one
//! station and a station name does not: the LTA feed carries several
//! names twice, for example `Bukit Panjang` on the Downtown Line and
//! on the Bukit Panjang LRT. A name in a link would name whichever
//! station the resolver happened to reach first, so
//! [`RailNetwork::station_by_alias`] never accepts one. Use
//! [`RailNetwork::station_by_name`] where a person types a full name
//! and an operator can resolve the ambiguity.
//!
//! [`RailNetwork::station_by_alias`]: crate::RailNetwork::station_by_alias
//! [`RailNetwork::station_by_name`]: crate::RailNetwork::station_by_name

/// Reduce any spelling of a station code to a comparison key.
///
/// The key keeps ASCII letters and digits only, in lower case, so
/// spacing and punctuation never decide whether a URL resolves.
///
/// ```
/// use mrt_gtfs::alias;
///
/// assert_eq!(alias::normalize("NS1"), "ns1");
/// assert_eq!(alias::normalize("ns-1"), "ns1");
/// assert_eq!(alias::normalize("NS 1"), "ns1");
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
    fn normalization_ignores_case_and_punctuation() {
        assert_eq!(normalize("NS1"), "ns1");
        assert_eq!(normalize("ns-1"), "ns1");
        assert_eq!(normalize("NS 1"), "ns1");
        assert_eq!(normalize("  ew24  "), "ew24");
        assert_eq!(normalize("n.s.1"), "ns1");
    }

    #[test]
    fn normalization_drops_everything_else() {
        assert_eq!(normalize(""), "");
        assert_eq!(normalize("!!!"), "");
        assert_eq!(normalize("nsl/ewl"), "nslewl");
    }
}
