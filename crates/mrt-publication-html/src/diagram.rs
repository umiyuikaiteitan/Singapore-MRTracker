//! Render a [`DiagramDocument`] as a self-contained HTML page.
//!
//! The page holds the SVG, the filter controls, and a call table for
//! every run. The tables are the accessible and print-friendly
//! equivalent of the drawing: with JavaScript switched off the reader
//! still gets every scheduled time, and with a screen reader the
//! tables carry what the polylines show.

use mrt_publication::{DiagramDocument, Labels, PublicationConfig};

use crate::escape;
use crate::page;
use crate::svg::{render_svg, SvgMode};

/// Render a diagram document as one HTML file.
pub fn render_diagram(document: &DiagramDocument, config: &PublicationConfig) -> String {
    let labels = Labels::for_language(config.language);
    let accent = document.runs.first().map(|r| &r.line);
    let (accent_color, accent_text) = page::accent_of(
        config,
        accent.and_then(|l| l.color.as_deref()),
        accent.and_then(|l| l.text_color.as_deref()),
    );
    let theme = page::theme_block(&config.theme, accent_color, accent_text);
    let title = document.title.get(config.language);

    let mut out = String::with_capacity(64 * 1024);
    page::head(
        &mut out,
        title,
        config.language.tag(),
        &[
            include_str!("../assets/common.css"),
            include_str!("../assets/diagram.css"),
        ],
        &theme,
    );

    let subtitle = format!(
        "{} \u{00B7} {} \u{2013} {} \u{00B7} {}",
        labels.diagram,
        crate::common_time(document.time_axis.start),
        crate::common_time(document.time_axis.end),
        document.service_day_label
    );
    page::masthead(&mut out, title, &subtitle, &[]);
    page::warnings(&mut out, &document.metadata, labels);
    controls(&mut out, labels);
    filters(&mut out, document, labels);

    out.push_str("<div class=\"diagram-frame\">\n");
    out.push_str(&render_svg(document, config, SvgMode::Embedded));
    out.push_str("</div>\n");

    out.push_str(
        "<section class=\"run-details needs-script no-print\" \
         aria-live=\"polite\" aria-labelledby=\"run-details-heading\">\n\
         <h2 id=\"run-details-heading\">",
    );
    out.push_str(&escape::text(labels.selected_run));
    out.push_str("</h2>\n<div id=\"run-details-body\"></div>\n</section>\n");

    if !document.frequency_bands.is_empty() {
        out.push_str("<ul class=\"band-list\">\n");
        for band in &document.frequency_bands {
            out.push_str("<li>");
            out.push_str(&escape::text(&band.label));
            out.push_str(" \u{00B7} ");
            out.push_str(&escape::text(&band.line.name));
            out.push_str(" \u{2192} ");
            out.push_str(&escape::text(&band.destination));
            out.push_str("</li>\n");
        }
        out.push_str("</ul>\n");
    }

    call_tables(&mut out, document, config, labels);
    page::legend(&mut out, &document.legend, labels);
    page::colophon(&mut out, &document.metadata, labels);

    out.push_str("<script type=\"application/json\" id=\"diagram-data\">");
    out.push_str(&escape::json(&data_island(document, config, labels)));
    out.push_str("</script>\n<script>\n");
    out.push_str(include_str!("../assets/diagram.js"));
    out.push_str("\n</script>\n");
    page::foot(&mut out);
    out
}

/// Render only the standalone SVG file.
pub fn render_diagram_svg(document: &DiagramDocument, config: &PublicationConfig) -> String {
    render_svg(document, config, SvgMode::Standalone)
}

fn controls(out: &mut String, labels: &Labels) {
    out.push_str("<div class=\"controls needs-script no-print\">\n");
    for (id, label) in [
        ("zoom-in", labels.zoom_in),
        ("zoom-out", labels.zoom_out),
        ("reset-view", labels.reset),
        ("print-page", labels.print),
        ("download-svg", labels.download_svg),
        ("toggle-mono", labels.monochrome),
    ] {
        out.push_str("<button type=\"button\" id=\"");
        out.push_str(id);
        out.push_str("\">");
        out.push_str(&escape::text(label));
        out.push_str("</button>\n");
    }
    out.push_str("</div>\n");
}

fn filters(out: &mut String, document: &DiagramDocument, labels: &Labels) {
    let has_bands_or_approximate = !document.frequency_bands.is_empty()
        || document.runs.iter().any(|r| !r.exactness.is_exact());
    let mut directions: Vec<String> = document
        .runs
        .iter()
        .map(|r| match r.direction {
            Some(value) => value.to_string(),
            None => "\u{2014}".to_string(),
        })
        .collect();
    directions.sort();
    directions.dedup();

    out.push_str("<details class=\"filters needs-script no-print\" open>\n<summary>");
    out.push_str(&escape::text(labels.filters));
    out.push_str("</summary>\n<div class=\"filter-groups\">\n");

    out.push_str("<fieldset><legend>");
    out.push_str(&escape::text(labels.line));
    out.push_str("</legend>\n");
    for line in &document.lines {
        let color = line
            .color
            .as_deref()
            .and_then(escape::css_color)
            .unwrap_or_else(|| "#8b97ad".to_string());
        out.push_str("<label><input type=\"checkbox\" checked data-filter=\"line\" value=\"");
        out.push_str(&escape::attr(&line.route_id));
        out.push_str("\"><span class=\"swatch\" style=\"background:");
        out.push_str(&color);
        out.push_str("\"></span>");
        out.push_str(&escape::text(&line.name));
        out.push_str("</label>\n");
    }
    out.push_str("</fieldset>\n");

    out.push_str("<fieldset><legend>");
    out.push_str(&escape::text(labels.direction));
    out.push_str("</legend>\n");
    for direction in &directions {
        out.push_str("<label><input type=\"checkbox\" checked data-filter=\"direction\" value=\"");
        out.push_str(&escape::attr(direction));
        out.push_str("\">");
        out.push_str(&escape::text(direction));
        out.push_str("</label>\n");
    }
    out.push_str("</fieldset>\n");

    out.push_str("<fieldset><legend>");
    out.push_str(&escape::text(labels.destination));
    out.push_str("</legend>\n");
    for destination in &document.destinations {
        out.push_str(
            "<label><input type=\"checkbox\" checked data-filter=\"destination\" value=\"",
        );
        out.push_str(&escape::attr(destination));
        out.push_str("\">");
        out.push_str(&escape::text(destination));
        out.push_str("</label>\n");
    }
    out.push_str("</fieldset>\n");

    if has_bands_or_approximate {
        out.push_str("<fieldset><legend>");
        out.push_str(&escape::text(labels.show_approximate));
        out.push_str("</legend>\n");
        out.push_str(
            "<label><input type=\"checkbox\" checked data-filter=\"exactness\" value=\"exact\">",
        );
        out.push_str(&escape::text(labels.exact));
        out.push_str("</label>\n");
        out.push_str(
            "<label><input type=\"checkbox\" checked data-filter=\"exactness\" \
             value=\"approximate\">",
        );
        out.push_str(&escape::text(labels.approximate));
        out.push_str("</label>\n");
        out.push_str("</fieldset>\n");
    }
    out.push_str("</div>\n</details>\n");
}

/// Write one call table per run.
///
/// This is the part of the page that needs neither JavaScript nor
/// colour vision, and it is what a printer produces when the drawing
/// is too dense to read.
fn call_tables(
    out: &mut String,
    document: &DiagramDocument,
    config: &PublicationConfig,
    labels: &Labels,
) {
    if document.runs.is_empty() {
        return;
    }
    out.push_str("<section class=\"call-tables\" aria-labelledby=\"calls-heading\">\n");
    out.push_str("<h2 id=\"calls-heading\">");
    out.push_str(&escape::text(labels.calls));
    out.push_str("</h2>\n");
    for run in &document.runs {
        out.push_str("<details>\n<summary>");
        let mut heading = String::new();
        if let Some(label) = &run.label {
            heading.push_str(label);
            heading.push_str(" \u{00B7} ");
        }
        heading.push_str(&run.line.name);
        heading.push_str(" \u{2192} ");
        heading.push_str(&run.destination);
        if let Some(first) = run.points.first() {
            heading.push_str(&format!(" \u{00B7} {}", crate::common_time(first.time)));
        }
        if !run.exactness.is_exact() {
            heading.push_str(" \u{00B7} ~");
        }
        out.push_str(&escape::text(&heading));
        out.push_str("</summary>\n<table>\n<thead><tr>");
        for column in [
            labels.stations,
            labels.arrival,
            labels.departure,
            labels.platform,
        ] {
            out.push_str("<th scope=\"col\">");
            out.push_str(&escape::text(column));
            out.push_str("</th>");
        }
        out.push_str("</tr></thead>\n<tbody>\n");
        for call in &run.calls {
            out.push_str("<tr><th scope=\"row\">");
            out.push_str(&escape::text(&call.station));
            if !call.stops {
                out.push_str(" <span class=\"flag\">(");
                out.push_str(&escape::text(labels.legend_pass_through));
                out.push_str(")</span>");
            }
            out.push_str("</th><td>");
            out.push_str(&escape::text(&time_cell(call.arrival)));
            out.push_str("</td><td>");
            out.push_str(&escape::text(&time_cell(call.departure)));
            out.push_str("</td><td>");
            out.push_str(&escape::text(
                call.platform.as_deref().unwrap_or("\u{2014}"),
            ));
            out.push_str("</td></tr>\n");
        }
        out.push_str("</tbody>\n</table>\n");
        if config.diagram.show_internal_trip_ids {
            out.push_str("<p class=\"muted\">");
            out.push_str(&escape::text(&run.source_trip_id));
            out.push_str("</p>\n");
        }
        out.push_str("</details>\n");
    }
    out.push_str("</section>\n");
}

fn time_cell(time: Option<mrt_gtfs::GtfsTime>) -> String {
    match time {
        Some(value) => value.to_string(),
        None => "\u{2014}".to_string(),
    }
}

/// Build the JSON island that the script reads.
fn data_island(document: &DiagramDocument, config: &PublicationConfig, labels: &Labels) -> String {
    let runs: Vec<serde_json::Value> = document
        .runs
        .iter()
        .map(|run| {
            let calls: Vec<serde_json::Value> = run
                .calls
                .iter()
                .map(|call| {
                    serde_json::json!({
                        "station": call.station,
                        "arrival": call.arrival.map(|t| t.to_string()),
                        "departure": call.departure.map(|t| t.to_string()),
                        "platform": call.platform,
                    })
                })
                .collect();
            let mut value = serde_json::json!({
                "id": run.instance_id,
                "line": run.line.name,
                "destination": run.destination,
                "direction": run.direction.map(|d| d.to_string()),
                "label": run.label,
                "exactness": if run.exactness.is_exact() { labels.exact } else { labels.approximate },
                "calls": calls,
            });
            if config.diagram.show_internal_trip_ids {
                value["tripId"] = serde_json::json!(run.source_trip_id);
            }
            value
        })
        .collect();

    serde_json::json!({
        "viewBox": {
            "x": 0.0,
            "y": 0.0,
            "w": document.layout.width,
            "h": document.layout.height,
        },
        "zoomVertical": false,
        "fileName": format!("{}-{}.svg", document.corridor.id, document.metadata.service_date),
        "labels": {
            "line": labels.line,
            "destination": labels.destination,
            "direction": labels.direction,
            "exactness": labels.show_approximate,
            "train": labels.train,
            "tripId": "GTFS trip_id",
            "station": labels.stations,
            "arrival": labels.arrival,
            "departure": labels.departure,
            "platform": labels.platform,
        },
        "runs": runs,
    })
    .to_string()
}
