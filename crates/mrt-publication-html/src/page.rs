//! The shared page frame.
//!
//! Both documents share a head, a theme block, a masthead, a legend,
//! and a colophon. This module writes them and nothing else.

use mrt_publication::{Labels, LegendItem, PublicationConfig, PublicationMetadata, ThemeConfig};

use crate::escape;

/// The Content-Security-Policy of every generated page.
///
/// `default-src 'none'` blocks every network request, so the page
/// cannot phone home, load a tracker, or leak the station a reader
/// looked up. Inline styles and scripts are allowed because the page
/// must be one self-contained file; nothing else is.
pub const CSP: &str = "default-src 'none'; \
     style-src 'unsafe-inline'; \
     script-src 'unsafe-inline'; \
     img-src data:; \
     base-uri 'none'; \
     form-action 'none'; \
     frame-ancestors 'none'";

/// Write the document head, up to and including the opening
/// `<body>` tag.
pub fn head(out: &mut String, title: &str, language: &str, styles: &[&str], theme: &str) {
    out.push_str("<!doctype html>\n<html lang=\"");
    out.push_str(&escape::attr(language));
    out.push_str("\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str("<meta http-equiv=\"Content-Security-Policy\" content=\"");
    out.push_str(&escape::attr(CSP));
    out.push_str("\">\n<meta name=\"referrer\" content=\"no-referrer\">\n<title>");
    out.push_str(&escape::text(title));
    out.push_str("</title>\n<style>\n");
    out.push_str(theme);
    for sheet in styles {
        out.push_str(sheet);
    }
    out.push_str("\n</style>\n</head>\n<body>\n<div class=\"page\">\n");
}

/// Close the page.
pub fn foot(out: &mut String) {
    out.push_str("</div>\n</body>\n</html>\n");
}

/// Build the `:root` block from the theme configuration.
///
/// Every value passes an escaping filter, so a hostile configuration
/// or a hostile `route_color` cannot inject a declaration.
pub fn theme_block(theme: &ThemeConfig, accent: Option<&str>, accent_text: Option<&str>) -> String {
    let fonts: Vec<String> = theme
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
    let accent_color = accent
        .and_then(escape::css_color)
        .unwrap_or_else(|| color(&theme.accent, "#1b2a5e"));
    let accent_text_color = accent_text
        .and_then(escape::css_color)
        .unwrap_or_else(|| "#ffffff".to_string());

    format!(
        ":root {{\n\
         --font-stack: {font_stack};\n\
         --bg: {bg};\n\
         --fg: {fg};\n\
         --muted: color-mix(in srgb, {fg} 62%, {bg});\n\
         --rule: color-mix(in srgb, {fg} 22%, {bg});\n\
         --panel-bg: {row_alt};\n\
         --row-alt: {row_alt};\n\
         --hour-bg: {hour};\n\
         --hour-fg: {hour_text};\n\
         --accent: {accent_color};\n\
         --accent-text: {accent_text_color};\n\
         --focus: #0b62d6;\n\
         --warn: #b26a00;\n\
         --warn-fg: #8a5200;\n\
         --warn-bg: #fff6e5;\n\
         }}\n",
        bg = color(&theme.background, "#ffffff"),
        fg = color(&theme.text, "#14171f"),
        row_alt = color(&theme.row_alternate, "#eef1f8"),
        hour = color(&theme.hour_cell, "#1b2a5e"),
        hour_text = color(&theme.hour_cell_text, "#ffffff"),
    )
}

/// Write the masthead.
pub fn masthead(out: &mut String, title: &str, subtitle: &str, codes: &[String]) {
    out.push_str("<header class=\"masthead\">\n<h1>");
    out.push_str(&escape::text(title));
    if !codes.is_empty() {
        out.push_str("<span class=\"station-codes\">");
        for code in codes {
            out.push_str("<span class=\"code-chip\">");
            out.push_str(&escape::text(code));
            out.push_str("</span>");
        }
        out.push_str("</span>");
    }
    out.push_str("</h1>\n<p class=\"subtitle\">");
    out.push_str(&escape::text(subtitle));
    out.push_str("</p>\n</header>\n");
}

/// Write the warning banner, when the document carries warnings or
/// came from a cached feed.
pub fn warnings(out: &mut String, metadata: &PublicationMetadata, labels: &Labels) {
    if metadata.warnings.is_empty() && !metadata.generated_from_cache {
        return;
    }
    out.push_str("<div class=\"notice\" role=\"note\">\n<strong>");
    out.push_str(&escape::text(labels.warnings));
    out.push_str("</strong>\n");
    if metadata.generated_from_cache {
        out.push_str("<p class=\"stale\">");
        out.push_str(&escape::text(labels.stale_feed));
        out.push_str("</p>\n");
    }
    if !metadata.warnings.is_empty() {
        out.push_str("<ul>\n");
        for warning in &metadata.warnings {
            out.push_str("<li>");
            out.push_str(&escape::text(warning));
            out.push_str("</li>\n");
        }
        out.push_str("</ul>\n");
    }
    out.push_str("</div>\n");
}

/// Write the legend.
pub fn legend(out: &mut String, items: &[LegendItem], labels: &Labels) {
    if items.is_empty() {
        return;
    }
    out.push_str(
        "<section class=\"legend\" aria-labelledby=\"legend-heading\">\n<h2 id=\"legend-heading\">",
    );
    out.push_str(&escape::text(labels.legend));
    out.push_str("</h2>\n<ul>\n");
    for item in items {
        out.push_str("<li>");
        if let Some(symbol) = &item.symbol {
            out.push_str("<span class=\"symbol\" aria-hidden=\"true\">");
            out.push_str(&escape::text(symbol));
            out.push_str("</span> ");
        }
        out.push_str(&escape::text(&item.label));
        out.push_str("</li>\n");
    }
    out.push_str("</ul>\n</section>\n");
}

/// Write the colophon: where the data came from and how to reproduce
/// the page.
pub fn colophon(out: &mut String, metadata: &PublicationMetadata, labels: &Labels) {
    let service_date = labels.service_date_text(metadata.service_date);
    out.push_str("<footer class=\"colophon\">\n<dl>\n");
    let mut row = |term: &str, value: &str| {
        out.push_str("<dt>");
        out.push_str(&escape::text(term));
        out.push_str("</dt><dd>");
        out.push_str(&escape::text(value));
        out.push_str("</dd>\n");
    };
    row(labels.service_date, &service_date);
    row("Time zone", &metadata.timezone);
    if let Some(timestamp) = &metadata.feed_timestamp {
        row(labels.source, timestamp);
    }
    row(labels.feed_fingerprint, metadata.short_feed_sha());
    row("Configuration", &short(&metadata.configuration_sha256));
    row("Generator", &metadata.generator_version);
    row("Schema", &metadata.schema_version);
    out.push_str("</dl>\n<p>");
    out.push_str(&escape::text(labels.offline_note));
    out.push_str("</p>\n</footer>\n");
}

fn short(value: &str) -> String {
    value.chars().take(12).collect()
}

/// Get the accent colors that a document should use.
pub fn accent_of<'a>(
    config: &'a PublicationConfig,
    line_color: Option<&'a str>,
    line_text: Option<&'a str>,
) -> (Option<&'a str>, Option<&'a str>) {
    let _ = config;
    (
        line_color.filter(|c| !c.is_empty()),
        line_text.filter(|c| !c.is_empty()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_theme_block_only_holds_safe_values() {
        let theme = ThemeConfig {
            font_stack: vec!["Evil\";} html { display: none } .x {".into()],
            hour_cell: "red; background: url(https://evil.example)".into(),
            hour_cell_text: "#ffffff".into(),
            row_alternate: "#eef1f8".into(),
            background: "#ffffff".into(),
            text: "#14171f".into(),
            accent: "javascript:alert(1)".into(),
        };
        let block = theme_block(&theme, None, None);
        assert!(!block.contains("url("));
        assert!(!block.contains("javascript"));
        assert!(!block.contains("display: none"));
        assert!(block.contains("--hour-bg: #1b2a5e"));
        assert!(block.contains("--accent: #1b2a5e"));
    }

    #[test]
    fn the_head_declares_a_policy_that_blocks_the_network() {
        let mut out = String::new();
        head(&mut out, "Title", "en", &[], ":root{}");
        assert!(out.contains("default-src &#39;none&#39;"));
        assert!(!out.contains("connect-src http"));
        assert!(out.contains("<html lang=\"en\">"));
    }

    #[test]
    fn a_hostile_title_cannot_escape_the_title_element() {
        let mut out = String::new();
        head(&mut out, "</title><script>x()</script>", "en", &[], "");
        assert!(!out.contains("<script>"));
        assert!(out.contains("&lt;/title&gt;"));
    }
}
