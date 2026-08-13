//! The page frame of the hub.
//!
//! The generated timetable and diagram pages bring their own frame
//! from `mrt-publication-html`. The hub is a page of this crate, so it
//! writes its own — with the same policy, the same theme tokens, and
//! the same colophon, so the two look like one site.

use mrt_publication::{Labels, PublicationConfig, PublicationMetadata};
use mrt_publication_html::escape;

use crate::plan::SitePlan;

/// Write the head and open the page.
pub fn head(out: &mut String, title: &str, language: &str, theme: &str, styles: &str) {
    out.push_str("<!doctype html>\n<html lang=\"");
    out.push_str(&escape::attr(language));
    out.push_str("\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str("<meta http-equiv=\"Content-Security-Policy\" content=\"");
    out.push_str(&escape::attr(mrt_publication_html::CSP));
    out.push_str("\">\n<meta name=\"referrer\" content=\"no-referrer\">\n<title>");
    out.push_str(&escape::text(title));
    out.push_str("</title>\n<style>\n");
    out.push_str(theme);
    out.push_str(HUB_BASE_CSS);
    out.push_str(styles);
    out.push_str("\n</style>\n</head>\n<body>\n<div class=\"page\">\n");
}

/// Close the page.
pub fn foot(out: &mut String) {
    out.push_str("</div>\n</body>\n</html>\n");
}

/// Write the colophon of the hub.
pub fn colophon(
    out: &mut String,
    metadata: &PublicationMetadata,
    labels: &Labels,
    plan: &SitePlan,
) {
    out.push_str("<footer class=\"colophon\">\n<dl>\n");
    let mut row = |term: &str, value: &str| {
        out.push_str("<dt>");
        out.push_str(&escape::text(term));
        out.push_str("</dt><dd>");
        out.push_str(&escape::text(value));
        out.push_str("</dd>\n");
    };
    row("Time zone", &metadata.timezone);
    if let Some(timestamp) = &metadata.feed_timestamp {
        row(labels.source, timestamp);
    }
    row(labels.feed_fingerprint, metadata.short_feed_sha());
    row("Generator", &metadata.generator_version);
    row("Schema", &metadata.schema_version);
    row(
        "Pages",
        &format!(
            "{} stations \u{00D7} {} days, {} lines \u{00D7} {} windows",
            plan.stations.len(),
            plan.dates.len(),
            plan.lines.len(),
            plan.windows.len()
        ),
    );
    out.push_str("</dl>\n<p>");
    out.push_str(&escape::text(labels.offline_note));
    out.push_str(" Schedule data from LTA DataMall, under the Singapore Open Data Licence.");
    out.push_str("</p>\n</footer>\n");
}

/// Build the `:root` block from the theme configuration.
///
/// This mirrors the block that the document renderer writes, so the
/// hub and the pages it links to share one palette.
pub fn theme_block(config: &PublicationConfig) -> String {
    let fonts: Vec<String> = config
        .theme
        .font_stack
        .iter()
        .filter_map(|name| escape::css_font_family(name))
        .collect();
    let font_stack = if fonts.is_empty() {
        "system-ui, sans-serif".to_string()
    } else {
        fonts.join(", ")
    };
    let color = |value: &str, fallback: &str| {
        escape::css_color(value).unwrap_or_else(|| fallback.to_string())
    };
    format!(
        ":root {{\n\
         --font-stack: {font_stack};\n\
         --bg: {bg};\n\
         --fg: {fg};\n\
         --muted: color-mix(in srgb, {fg} 62%, {bg});\n\
         --rule: color-mix(in srgb, {fg} 22%, {bg});\n\
         --panel-bg: {row_alt};\n\
         --row-alt: {row_alt};\n\
         --accent: {accent};\n\
         --accent-text: #ffffff;\n\
         --focus: #0b62d6;\n\
         }}\n",
        bg = color(&config.theme.background, "#ffffff"),
        fg = color(&config.theme.text, "#14171f"),
        row_alt = color(&config.theme.row_alternate, "#eef1f8"),
        accent = color(&config.theme.accent, "#1b2a5e"),
    )
}

/// The frame styles that the hub shares with the document pages.
const HUB_BASE_CSS: &str = r#"
*, *::before, *::after { box-sizing: border-box; }
html { -webkit-text-size-adjust: 100%; }
body {
  margin: 0;
  padding: 0 0 3rem;
  background: var(--bg);
  color: var(--fg);
  font-family: var(--font-stack);
  font-size: 16px;
  line-height: 1.45;
}
.page { max-width: 78rem; margin: 0 auto; padding: 1.5rem 1.25rem 0; }
.masthead {
  border-top: 6px solid var(--accent);
  padding-top: 0.9rem;
  margin-bottom: 1.25rem;
}
.masthead h1 {
  margin: 0;
  font-size: clamp(1.6rem, 1.2rem + 1.6vw, 2.4rem);
  font-weight: 800;
}
.masthead .subtitle { margin: 0.35rem 0 0; font-size: 1rem; color: var(--muted); }
.colophon {
  margin: 2rem 0 0;
  padding-top: 0.75rem;
  border-top: 1px solid var(--rule);
  font-size: 0.78rem;
  color: var(--muted);
}
.colophon dl { display: grid; grid-template-columns: auto 1fr; gap: 0.15rem 0.75rem; margin: 0; }
.colophon dt { font-weight: 700; }
.colophon dd { margin: 0; word-break: break-all; }
a:focus-visible { outline: 3px solid var(--focus); outline-offset: 2px; }
@page { size: A4 portrait; margin: 12mm; }
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hub_frame_blocks_the_network_like_the_document_pages() {
        let mut out = String::new();
        head(&mut out, "Title", "en", ":root{}", "");
        assert!(out.contains("default-src &#39;none&#39;"));
        assert!(!out.contains("<link "));
        assert!(out.contains("<html lang=\"en\">"));
    }

    #[test]
    fn a_hostile_theme_cannot_reach_the_stylesheet() {
        let mut config = PublicationConfig::default();
        config.theme.accent = "red; background: url(https://evil.example)".into();
        config.theme.font_stack = vec!["A\"; } html { display: none } .x {".into()];
        let block = theme_block(&config);
        assert!(!block.contains("url("));
        assert!(!block.contains("display: none"));
        assert!(block.contains("--accent: #1b2a5e"));
    }
}
