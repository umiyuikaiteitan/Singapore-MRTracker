//! Escaping.
//!
//! Every string that reaches a generated page may come from a GTFS
//! feed, and a feed is untrusted input. This module holds the only
//! functions that put such a string into markup. Nothing else in the
//! crate writes feed text directly.
//!
//! | Function | Context |
//! |----------|---------|
//! | [`text`] | element content in HTML and SVG |
//! | [`attr`] | an attribute value in double quotes |
//! | [`json`] | a JSON island inside `<script type="application/json">` |
//! | [`css_ident`] | a value that becomes part of a CSS selector |

/// Escape a string for element content in HTML and SVG.
///
/// The function escapes `&`, `<`, and `>`. It also escapes `"` and
/// `'`, which costs nothing and makes the result safe to paste into an
/// attribute by mistake.
pub fn text(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 8);
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Escape a string for an attribute value in double quotes.
///
/// The rules are the same as for [`text`]; the separate name keeps the
/// call sites self-documenting.
pub fn attr(value: &str) -> String {
    text(value)
}

/// Escape a JSON document so that it is safe inside a `<script>`
/// element.
///
/// A JSON string may contain `</script`, which would end the element
/// early. Escaping `<`, `>`, and `&` as `\uXXXX` keeps the document
/// valid JSON and inert as markup.
pub fn json(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            _ => out.push(c),
        }
    }
    out
}

/// Reduce a string to characters that are safe inside a CSS selector,
/// an HTML `id`, and a `class` name.
///
/// The output holds ASCII letters, digits, and hyphens only, so no
/// feed value can close a rule or start a new one.
pub fn css_ident(value: &str) -> String {
    mrt_publication::css_key(value)
}

/// Escape a color for a CSS declaration.
///
/// Only a `#` followed by three, four, six, or eight hexadecimal
/// digits survives. Anything else yields `None`, and the caller falls
/// back to a theme value.
pub fn css_color(value: &str) -> Option<String> {
    let digits = value.strip_prefix('#')?;
    let valid =
        matches!(digits.len(), 3 | 4 | 6 | 8) && digits.bytes().all(|b| b.is_ascii_hexdigit());
    valid.then(|| format!("#{digits}"))
}

/// Escape a font family name for a CSS `font-family` list.
///
/// A name that is a plain identifier or a generic family passes
/// through. Anything else is quoted, with quotes and backslashes
/// removed, so a hostile configuration cannot end the declaration.
pub fn css_font_family(value: &str) -> Option<String> {
    let clean: String = value
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        .collect();
    let clean = clean.trim();
    if clean.is_empty() {
        return None;
    }
    const GENERIC: [&str; 6] = [
        "serif",
        "sans-serif",
        "monospace",
        "cursive",
        "fantasy",
        "system-ui",
    ];
    if GENERIC.contains(&clean) {
        Some(clean.to_string())
    } else {
        Some(format!("\"{clean}\""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_characters_cannot_survive() {
        assert_eq!(
            text("<script>alert('x')&</script>"),
            "&lt;script&gt;alert(&#39;x&#39;)&amp;&lt;/script&gt;"
        );
        assert_eq!(attr("a\"b"), "a&quot;b");
    }

    #[test]
    fn ordinary_text_passes_through() {
        assert_eq!(text("Jurong East"), "Jurong East");
        assert_eq!(text("発車時刻表"), "発車時刻表");
        assert_eq!(text("06:30\u{2013}09:00"), "06:30\u{2013}09:00");
    }

    #[test]
    fn a_json_island_cannot_close_its_script_element() {
        let payload = r#"{"name":"</script><img src=x onerror=alert(1)>"}"#;
        let escaped = json(payload);
        assert!(!escaped.contains("</script"));
        assert!(!escaped.contains('<'));
        // The result is still the same JSON value.
        let parsed: serde_json::Value = serde_json::from_str(&escaped).unwrap();
        assert_eq!(parsed["name"], "</script><img src=x onerror=alert(1)>");
    }

    #[test]
    fn line_separators_are_escaped_for_javascript() {
        let escaped = json("\"a\u{2028}b\"");
        assert!(escaped.contains("\\u2028"));
        let parsed: serde_json::Value = serde_json::from_str(&escaped).unwrap();
        assert_eq!(parsed, "a\u{2028}b");
    }

    #[test]
    fn only_real_colors_reach_the_stylesheet() {
        assert_eq!(css_color("#D42E12").as_deref(), Some("#D42E12"));
        assert_eq!(css_color("#abc").as_deref(), Some("#abc"));
        assert_eq!(css_color("red").as_deref(), None);
        assert_eq!(css_color("#12345").as_deref(), None);
        assert_eq!(css_color("#fff;}body{display:none").as_deref(), None);
    }

    #[test]
    fn font_names_are_quoted_and_stripped() {
        assert_eq!(
            css_font_family("Noto Sans").as_deref(),
            Some("\"Noto Sans\"")
        );
        assert_eq!(css_font_family("sans-serif").as_deref(), Some("sans-serif"));
        assert_eq!(
            css_font_family("Evil\";} body { display: none } .x {").as_deref(),
            Some("\"Evil body  display none  x\"")
        );
        assert_eq!(css_font_family("  ").as_deref(), None);
    }

    #[test]
    fn css_identifiers_hold_no_punctuation() {
        assert_eq!(css_ident("NS_1"), "ns-1");
        assert_eq!(css_ident("a{}b"), "a-b");
    }
}
