//! Render a [`DiagramDocument`] as SVG.
//!
//! The same function produces the drawing that the HTML page embeds
//! and the standalone `.svg` file that the command line writes. The
//! standalone form only adds an XML declaration and a `width` and
//! `height`, so what a reader downloads is what a reader saw.
//!
//! The SVG carries its own `<style>` element and no external
//! reference of any kind, so it opens correctly from a file manager,
//! an email attachment, or a drawing program.

use mrt_publication::{
    AxisDirection, DiagramDocument, DiagramPoint, DiagramRun, Labels, PublicationConfig, TickLevel,
};

use crate::escape;

/// The stylesheet that travels inside the SVG.
const SVG_STYLE: &str = r#"
.plot-bg { fill: var(--svg-bg, #ffffff); }
.grid-minor { stroke: #d9dee8; stroke-width: 0.5; }
.grid-medium { stroke: #b8c1d2; stroke-width: 0.8; }
.grid-major { stroke: #8b97ad; stroke-width: 1.2; }
.grid-day { stroke: #1b2a5e; stroke-width: 2; stroke-dasharray: 6 3; }
.station-line { stroke: #c9d0dd; stroke-width: 0.6; }
.station-line.panel-edge { stroke: #7c879c; stroke-width: 1.2; }
.axis-label { font-size: 10px; fill: #3a4358; }
.axis-label.major { font-weight: 700; fill: #1b2a5e; }
.station-name { font-size: 10.5px; fill: #14171f; }
.station-code { font-size: 9px; font-weight: 700; fill: #4a5468; }
.panel-title { font-size: 10px; font-weight: 700; fill: #1b2a5e; letter-spacing: 0.06em; }
.run path { fill: none; stroke-width: 1.6; stroke-linejoin: round; stroke-linecap: round; }
.run.approximate path { stroke-dasharray: 5 3; stroke-width: 1.4; }
.run .hit { stroke: transparent; stroke-width: 9; fill: none; }
.run .stop-dot { r: 1.9; }
.run .pass-dot { fill: none; stroke-width: 1; r: 2.2; }
.run .run-label { font-size: 8.5px; font-weight: 700; paint-order: stroke; stroke: #ffffff; stroke-width: 2.4; }
.band path { fill: none; stroke-dasharray: 5 3; stroke-width: 1.4; }
.band .band-fill { fill-opacity: 0.12; stroke: none; }
.band .band-label { font-size: 9px; font-weight: 700; paint-order: stroke; stroke: #ffffff; stroke-width: 2.4; }
.frame { fill: none; stroke: #8b97ad; stroke-width: 1; }
svg[data-highlight] .run { opacity: 0.16; }
svg[data-highlight] .run:hover,
svg[data-highlight] .run:focus-within { opacity: 1; }
.run:focus { outline: none; }
.run:focus-within path.trace { stroke-width: 3.2; }
@media print {
  .run path { stroke-width: 1.1; }
  .grid-minor { stroke: #e2e6ee; }
}
"#;

/// How the SVG is packaged.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SvgMode {
    /// A fragment for embedding in an HTML page.
    Embedded,
    /// A standalone document with an XML declaration.
    Standalone,
}

/// Render the diagram as SVG.
pub fn render_svg(document: &DiagramDocument, config: &PublicationConfig, mode: SvgMode) -> String {
    let labels = Labels::for_language(config.language);
    let layout = &document.layout;
    let mut out = String::with_capacity(64 * 1024);

    if mode == SvgMode::Standalone {
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    }
    out.push_str("<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\" id=\"diagram-svg\" ");
    out.push_str("role=\"img\" tabindex=\"0\" ");
    if mode == SvgMode::Standalone {
        out.push_str(&format!(
            "width=\"{}\" height=\"{}\" ",
            layout.width, layout.height
        ));
    }
    out.push_str(&format!(
        "viewBox=\"0 0 {} {}\" aria-labelledby=\"svg-title svg-desc\">\n",
        layout.width, layout.height
    ));

    let title = document.title.get(config.language);
    out.push_str("<title id=\"svg-title\">");
    out.push_str(&escape::text(title));
    out.push_str("</title>\n<desc id=\"svg-desc\">");
    out.push_str(&escape::text(&describe(document, labels)));
    out.push_str("</desc>\n<style>");
    out.push_str(SVG_STYLE);
    out.push_str("</style>\n");

    out.push_str(&format!(
        "<rect class=\"plot-bg\" x=\"0\" y=\"0\" width=\"{}\" height=\"{}\"/>\n",
        layout.width, layout.height
    ));

    grid(&mut out, document);
    station_axis(&mut out, document);
    bands(&mut out, document);
    runs(&mut out, document, config);

    out.push_str(&format!(
        "<rect class=\"frame\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>\n",
        layout.margin_left, layout.margin_top, layout.plot_width, layout.plot_height
    ));
    out.push_str("</svg>\n");
    out
}

/// Build the text alternative of the drawing.
fn describe(document: &DiagramDocument, labels: &Labels) -> String {
    format!(
        "{}. {}: {} \u{2013} {}. {}: {}. {}: {}. {}: {}.",
        document.corridor.label,
        labels.time,
        crate::common_time(document.time_axis.start),
        crate::common_time(document.time_axis.end),
        labels.stations,
        document.corridor.nodes.len(),
        labels.runs,
        document.runs.len(),
        labels.service_date,
        document.service_day_label
    )
}

fn grid(out: &mut String, document: &DiagramDocument) {
    let layout = &document.layout;
    let top = layout.margin_top;
    let bottom = layout.margin_top + layout.plot_height;
    out.push_str("<g class=\"grid\" aria-hidden=\"true\">\n");
    for tick in &document.time_axis.ticks {
        let class = match tick.level {
            TickLevel::Minor => "grid-minor",
            TickLevel::Medium => "grid-medium",
            TickLevel::Major => "grid-major",
            TickLevel::DayBoundary => "grid-day",
        };
        out.push_str(&format!(
            "<line class=\"{class}\" x1=\"{x}\" y1=\"{top}\" x2=\"{x}\" y2=\"{bottom}\"/>\n",
            x = tick.x
        ));
    }
    out.push_str("</g>\n<g class=\"time-labels\">\n");
    for tick in &document.time_axis.ticks {
        let Some(label) = &tick.label else {
            continue;
        };
        let class = if tick.level == TickLevel::Minor {
            "axis-label"
        } else {
            "axis-label major"
        };
        for y in [top - 6.0, bottom + 14.0] {
            out.push_str(&format!(
                "<text class=\"{class}\" x=\"{x}\" y=\"{y}\" text-anchor=\"middle\">{label}</text>\n",
                x = tick.x,
                label = escape::text(label)
            ));
        }
    }
    out.push_str("</g>\n");
}

fn station_axis(out: &mut String, document: &DiagramDocument) {
    let layout = &document.layout;
    let left = layout.margin_left;
    let right = layout.margin_left + layout.plot_width;
    out.push_str("<g class=\"stations\">\n");
    for (index, node) in document.corridor.nodes.iter().enumerate() {
        let y = layout.margin_top + node.y;
        let edge = document
            .corridor
            .panels
            .iter()
            .any(|p| p.first_node == index || p.last_node == index);
        out.push_str(&format!(
            "<line class=\"station-line{}\" x1=\"{left}\" y1=\"{y}\" x2=\"{right}\" y2=\"{y}\"/>\n",
            if edge { " panel-edge" } else { "" }
        ));
        let mut label_x = left - 10.0;
        if let Some(code) = node.station.codes.first() {
            out.push_str(&format!(
                "<text class=\"station-code\" x=\"{x}\" y=\"{ty}\" text-anchor=\"end\">{code}</text>\n",
                x = label_x,
                ty = y + 3.5,
                code = escape::text(code)
            ));
            label_x -= 32.0;
        }
        out.push_str(&format!(
            "<text class=\"station-name\" x=\"{x}\" y=\"{ty}\" text-anchor=\"end\">{name}</text>\n",
            x = label_x,
            ty = y + 3.5,
            name = escape::text(&node.station.name)
        ));
    }
    for panel in &document.corridor.panels {
        if document.corridor.panels.len() < 2 {
            break;
        }
        let y = layout.margin_top + document.corridor.nodes[panel.first_node].y - 14.0;
        out.push_str(&format!(
            "<text class=\"panel-title\" x=\"{x}\" y=\"{y}\">{label}</text>\n",
            x = layout.margin_left + 4.0,
            label = escape::text(&panel.label)
        ));
    }
    out.push_str("</g>\n");
}

fn bands(out: &mut String, document: &DiagramDocument) {
    if document.frequency_bands.is_empty() {
        return;
    }
    let top = document.layout.margin_top;
    out.push_str("<g class=\"bands\">\n");
    for band in &document.frequency_bands {
        let color = band.line.color.as_deref().unwrap_or("#7a7f8c");
        let color = escape::css_color(color).unwrap_or_else(|| "#7a7f8c".to_string());
        out.push_str("<g class=\"band\" data-band=\"");
        out.push_str(&escape::attr(&band.band_id));
        out.push_str("\">\n");
        if !band.first_path.is_empty() && !band.last_path.is_empty() {
            // The filled envelope shows that a train runs somewhere in
            // this region, without claiming a departure time.
            let mut fill = points_to_path(&band.first_path, top);
            let reversed: Vec<DiagramPoint> = band.last_path.iter().rev().cloned().collect();
            fill.push(' ');
            fill.push_str(&points_to_path(&reversed, top).replacen('M', "L", 1));
            fill.push_str(" Z");
            out.push_str(&format!(
                "<path class=\"band-fill\" fill=\"{color}\" d=\"{fill}\"/>\n"
            ));
        }
        for path in [&band.first_path, &band.last_path] {
            if path.is_empty() {
                continue;
            }
            out.push_str(&format!(
                "<path stroke=\"{color}\" d=\"{}\"/>\n",
                points_to_path(path, top)
            ));
        }
        if let Some(point) = band.first_path.first() {
            out.push_str(&format!(
                "<text class=\"band-label\" fill=\"{color}\" x=\"{x}\" y=\"{y}\">{label}</text>\n",
                x = point.x + 4.0,
                y = point.y + top - 5.0,
                label = escape::text(&band.label)
            ));
        }
        out.push_str("</g>\n");
    }
    out.push_str("</g>\n");
}

fn runs(out: &mut String, document: &DiagramDocument, config: &PublicationConfig) {
    let top = document.layout.margin_top;
    out.push_str("<g class=\"runs\">\n");
    for run in &document.runs {
        if run.points.len() < 2 {
            continue;
        }
        let color = run
            .line
            .color
            .as_deref()
            .and_then(escape::css_color)
            .unwrap_or_else(|| "#1b2a5e".to_string());
        let path = points_to_path(&run.points, top);

        out.push_str("<g class=\"run");
        if !run.exactness.is_exact() {
            out.push_str(" approximate");
        }
        out.push_str("\" tabindex=\"0\" role=\"listitem\" data-run=\"");
        out.push_str(&escape::attr(&run.instance_id));
        out.push_str("\" data-line=\"");
        out.push_str(&escape::attr(&run.line.route_id));
        out.push_str("\" data-direction=\"");
        out.push_str(&escape::attr(&direction_key(run)));
        out.push_str("\" data-destination=\"");
        out.push_str(&escape::attr(&run.destination));
        out.push_str("\" data-exactness=\"");
        out.push_str(if run.exactness.is_exact() {
            "exact"
        } else {
            "approximate"
        });
        out.push_str("\" data-panel=\"");
        out.push_str(&run.panel.to_string());
        out.push_str("\">\n<title>");
        out.push_str(&escape::text(&run_title(run, config)));
        out.push_str("</title>\n");
        // A wide transparent stroke makes the thin path easy to hit
        // with a pointer and easy to reach with a finger.
        out.push_str(&format!("<path class=\"hit\" d=\"{path}\"/>\n"));
        out.push_str(&format!(
            "<path class=\"trace\" stroke=\"{color}\" d=\"{path}\"/>\n"
        ));

        for call in &run.calls {
            if !call.in_window {
                continue;
            }
            let Some(x) = call.x_arrival.or(call.x_departure) else {
                continue;
            };
            let y = call.y + top;
            if call.stops {
                out.push_str(&format!(
                    "<circle class=\"stop-dot\" fill=\"{color}\" cx=\"{x}\" cy=\"{y}\"/>\n"
                ));
            } else {
                out.push_str(&format!(
                    "<circle class=\"pass-dot\" stroke=\"{color}\" cx=\"{x}\" cy=\"{y}\"/>\n"
                ));
            }
        }

        if let (Some(label), Some(placement)) = (&run.label, &run.label_placement) {
            out.push_str(&format!(
                "<text class=\"run-label\" fill=\"{color}\" x=\"{x}\" y=\"{y}\" \
                 text-anchor=\"middle\" transform=\"rotate({angle} {x} {y})\">{label}</text>\n",
                x = placement.x,
                y = placement.y + top - 4.0,
                angle = placement.angle,
                label = escape::text(label)
            ));
        }
        out.push_str("</g>\n");
    }
    out.push_str("</g>\n");
}

/// Build the accessible title of one run.
fn run_title(run: &DiagramRun, config: &PublicationConfig) -> String {
    let mut text = String::new();
    if let Some(label) = &run.label {
        text.push_str(label);
        text.push_str(" \u{00B7} ");
    }
    text.push_str(&run.line.name);
    text.push_str(" \u{2192} ");
    text.push_str(&run.destination);
    if let (Some(first), Some(last)) = (run.points.first(), run.points.last()) {
        text.push_str(&format!(
            " \u{00B7} {} \u{2013} {}",
            crate::common_time(first.time),
            crate::common_time(last.time)
        ));
    }
    if config.diagram.show_internal_trip_ids {
        text.push_str(&format!(" \u{00B7} {}", run.source_trip_id));
    }
    text
}

fn direction_key(run: &DiagramRun) -> String {
    match run.direction {
        Some(value) => value.to_string(),
        None => match run.axis_direction {
            AxisDirection::Down => "down".to_string(),
            AxisDirection::Up => "up".to_string(),
        },
    }
}

/// Turn a polyline into an SVG path.
fn points_to_path(points: &[DiagramPoint], y_offset: f64) -> String {
    let mut out = String::with_capacity(points.len() * 16);
    for (index, point) in points.iter().enumerate() {
        out.push(if index == 0 { 'M' } else { 'L' });
        out.push_str(&format!("{} {}", point.x, point.y + y_offset));
        if index + 1 < points.len() {
            out.push(' ');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use mrt_gtfs::GtfsTime;

    fn point(x: f64, y: f64) -> DiagramPoint {
        DiagramPoint {
            time: GtfsTime::from_seconds(0),
            node: None,
            x,
            y,
        }
    }

    #[test]
    fn a_polyline_becomes_a_path() {
        let path = points_to_path(&[point(1.0, 2.0), point(3.0, 4.0)], 10.0);
        assert_eq!(path, "M1 12 L3 14");
    }

    #[test]
    fn an_empty_polyline_makes_an_empty_path() {
        assert_eq!(points_to_path(&[], 0.0), "");
    }
}
