//! Render a [`TimetableDocument`] as self-contained HTML.
//!
//! The markup is semantic first: a panel is a `<section>`, an hour
//! row is a `<tr>` with the hour in a `<th>`, and the departures of
//! that hour are an `<ol>`. With the stylesheet removed the page still
//! reads as "05 — 12 Springleaf, 24 Woodlands South", which is what a
//! screen reader announces and what a text browser shows.

use mrt_publication::{
    DepartureFlag, HourGroup, Labels, PublicationConfig, TimetableDeparture, TimetableDocument,
    TimetablePanel,
};

use crate::escape;
use crate::page;

/// Render a timetable document as one HTML file.
pub fn render_timetable(document: &TimetableDocument, config: &PublicationConfig) -> String {
    let labels = Labels::for_language(config.language);
    let accent = document.panels.first().map(|p| &p.line);
    let (accent_color, accent_text) = page::accent_of(
        config,
        accent.and_then(|l| l.color.as_deref()),
        accent.and_then(|l| l.text_color.as_deref()),
    );
    let theme = page::theme_block(&config.theme, accent_color, accent_text);
    let title = document.title.get(config.language);

    let mut out = String::with_capacity(16 * 1024);
    page::head(
        &mut out,
        title,
        config.language.tag(),
        &[
            include_str!("../assets/common.css"),
            include_str!("../assets/timetable.css"),
        ],
        &theme,
    );

    let subtitle = format!(
        "{} \u{00B7} {}",
        labels.departures, document.service_day_label
    );
    page::masthead(&mut out, title, &subtitle, &document.station.codes);
    page::warnings(&mut out, &document.metadata, labels);

    // A monochrome switch, for a reader and for a laser printer. The
    // page is fully usable without it.
    out.push_str(
        "<div class=\"controls needs-script no-print\">\n\
         <button type=\"button\" id=\"print-page\">",
    );
    out.push_str(&escape::text(labels.print));
    out.push_str("</button>\n<button type=\"button\" id=\"toggle-mono\">");
    out.push_str(&escape::text(labels.monochrome));
    out.push_str("</button>\n</div>\n");

    if document.panels.is_empty() {
        out.push_str("<p class=\"no-departures\">");
        out.push_str(&escape::text(labels.no_departures));
        out.push_str("</p>\n");
    } else {
        out.push_str("<div class=\"panels\">\n");
        for panel in &document.panels {
            render_panel(&mut out, panel, document, config, labels);
        }
        out.push_str("</div>\n");
    }

    page::legend(&mut out, &document.legend, labels);
    page::colophon(&mut out, &document.metadata, labels);
    out.push_str("<script>\n");
    out.push_str(include_str!("../assets/page.js"));
    out.push_str("\n</script>\n");
    page::foot(&mut out);
    out
}

fn render_panel(
    out: &mut String,
    panel: &TimetablePanel,
    document: &TimetableDocument,
    config: &PublicationConfig,
    labels: &Labels,
) {
    let heading_id = format!("panel-{}-heading", escape::css_ident(&panel.key));
    out.push_str("<section class=\"panel\" id=\"panel-");
    out.push_str(&escape::attr(&escape::css_ident(&panel.key)));
    out.push_str("\" aria-labelledby=\"");
    out.push_str(&escape::attr(&heading_id));
    out.push_str("\" style=\"");
    // The line color styles this panel only, so a document with two
    // lines keeps both identities.
    if let Some(color) = panel.line.color.as_deref().and_then(escape::css_color) {
        out.push_str("--line-color: ");
        out.push_str(&color);
        out.push(';');
    }
    if let Some(color) = panel.line.text_color.as_deref().and_then(escape::css_color) {
        out.push_str("--line-text: ");
        out.push_str(&color);
        out.push(';');
    }
    out.push_str("\">\n<div class=\"panel-head\">\n<span class=\"line-name\">");
    out.push_str(&escape::text(&panel.line.name));
    out.push_str("</span>\n<h2 class=\"direction\" id=\"");
    out.push_str(&escape::attr(&heading_id));
    out.push_str("\">");
    out.push_str(&escape::text(&panel.direction_label));
    out.push_str("</h2>\n");
    if let Some(platform) = &panel.platform_label {
        out.push_str("<span class=\"platform\">");
        out.push_str(&escape::text(platform));
        out.push_str("</span>\n");
    }
    out.push_str("</div>\n");

    if panel.destination_summary.len() > 1 {
        out.push_str("<p class=\"panel-destinations\">");
        out.push_str(&escape::text(&panel.destination_summary.join(" \u{00B7} ")));
        out.push_str("</p>\n");
    }

    let columns = panel.columns();
    let count = columns.len().min(4);
    out.push_str("<div class=\"panel-columns\" data-columns=\"");
    out.push_str(&count.to_string());
    out.push_str("\">\n");
    for (index, column) in columns.iter().enumerate() {
        render_column(out, column, index, panel, document, labels);
    }
    out.push_str("</div>\n");

    if !panel.frequency_notes.is_empty() {
        out.push_str("<ul class=\"bands\">\n");
        for note in &panel.frequency_notes {
            out.push_str("<li class=\"band-row\"><span class=\"band-time\">");
            out.push_str(&escape::text(&note.text));
            out.push_str("</span> <span class=\"band-dest\">");
            out.push_str(&escape::text(&note.destination));
            out.push_str("</span></li>\n");
        }
        out.push_str("</ul>\n");
    }
    out.push_str("</section>\n");
    let _ = config;
}

fn render_column(
    out: &mut String,
    groups: &[HourGroup],
    index: usize,
    panel: &TimetablePanel,
    document: &TimetableDocument,
    labels: &Labels,
) {
    out.push_str("<table class=\"hours\">\n<caption class=\"visually-hidden\">");
    out.push_str(&escape::text(&format!(
        "{} \u{00B7} {} \u{00B7} {} {}",
        panel.line.name,
        panel.direction_label,
        document.service_day_label,
        index + 1
    )));
    out.push_str("</caption>\n<thead><tr><th scope=\"col\" class=\"col-hour\">");
    out.push_str(&escape::text(labels.hour));
    out.push_str("</th><th scope=\"col\">");
    out.push_str(&escape::text(labels.minutes));
    out.push_str("</th></tr></thead>\n<tbody>\n");

    for group in groups {
        let past_midnight = group.service_hour >= 24;
        out.push_str("<tr class=\"hour-row");
        if past_midnight {
            out.push_str(" past-midnight");
        }
        out.push_str("\">\n<th scope=\"row\" class=\"hour-cell\">");
        out.push_str(&format!("{:02}", group.display_hour));
        out.push_str("</th>\n<td class=\"minutes\">");
        if group.departures.is_empty() {
            out.push_str("<span class=\"empty-hour\" aria-hidden=\"true\">\u{2014}</span>");
        } else {
            out.push_str("<ol>\n");
            for departure in &group.departures {
                render_departure(out, departure, labels);
            }
            out.push_str("</ol>");
        }
        out.push_str("</td>\n</tr>\n");
    }
    out.push_str("</tbody>\n</table>\n");
}

fn render_departure(out: &mut String, departure: &TimetableDeparture, labels: &Labels) {
    let approximate = departure.flags.contains(&DepartureFlag::Approximate);
    out.push_str("<li class=\"dep");
    if approximate {
        out.push_str(" approximate");
    }
    out.push_str("\">");

    // The accessible name spells everything out, because the visual
    // form abbreviates and relies on position.
    let mut spoken = format!(
        "{:02}:{:02}",
        departure.scheduled_time.hours() % 24,
        departure.display_minute
    );
    if let Some(seconds) = departure.display_seconds {
        spoken.push_str(&format!(":{seconds:02}"));
    }
    spoken.push_str(" \u{2192} ");
    spoken.push_str(&departure.destination_full);
    if let Some(platform) = &departure.platform {
        spoken.push_str(&format!(", {} {platform}", labels.platform));
    }
    for flag in &departure.flags {
        spoken.push_str(", ");
        spoken.push_str(flag.explanation(labels));
    }

    out.push_str("<span class=\"visually-hidden\">");
    out.push_str(&escape::text(&spoken));
    out.push_str("</span>\n<span aria-hidden=\"true\" class=\"min\">");
    out.push_str(&format!("{:02}", departure.display_minute));
    out.push_str("</span>");
    if let Some(seconds) = departure.display_seconds {
        out.push_str("<span aria-hidden=\"true\" class=\"sec\">");
        out.push_str(&format!("{seconds:02}"));
        out.push_str("</span>");
    }
    out.push_str("<span aria-hidden=\"true\" class=\"dest\" title=\"");
    out.push_str(&escape::attr(&departure.destination_full));
    out.push_str("\">");
    out.push_str(&escape::text(&departure.destination));
    out.push_str("</span>");
    if let Some(name) = &departure.trip_short_name {
        out.push_str("<span aria-hidden=\"true\" class=\"train-name\">");
        out.push_str(&escape::text(name));
        out.push_str("</span>");
    }
    for flag in &departure.flags {
        out.push_str("<span aria-hidden=\"true\" class=\"flag flag-");
        out.push_str(flag.key());
        out.push_str("\">");
        out.push_str(&escape::text(flag.symbol()));
        out.push_str("</span>");
    }
    out.push_str("</li>\n");
}
