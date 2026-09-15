//! The hub page.
//!
//! One page per service date, listing every line and every station.
//! It reuses the theme and the page frame of the generated documents,
//! so the site reads as one product rather than a folder of files.
//!
//! The whole station list is in the document. The search box filters
//! it and is an enhancement: without JavaScript a visitor scrolls, or
//! uses the browser's own find command, and every link still works.

use mrt_publication::{Labels, PublicationConfig, PublicationMetadata};
use mrt_publication_html::escape;

use crate::build::WrittenPages;
use crate::plan::{DateEntry, LineEntry, SitePlan, StationEntry, WindowEntry};

/// What the hub says about itself and where it links back to.
#[derive(Clone, Debug)]
pub struct SiteInfo {
    /// The name of the site, in the masthead.
    pub title: String,
    /// A relative link back to the live board, when one is deployed
    /// beside this section.
    pub board_href: Option<String>,
    /// The label of that link.
    pub board_label: String,
}

impl Default for SiteInfo {
    fn default() -> Self {
        SiteInfo {
            title: "Singapore rail timetables".to_string(),
            board_href: Some("../index.html".to_string()),
            board_label: "Live departure board".to_string(),
        }
    }
}

/// Render the hub for one service date.
///
/// The hub lists only the pages in `written`: a station whose
/// timetable failed, or a diagram window that failed, is dropped
/// rather than linked to a file that does not exist. The build report
/// carries what is missing.
pub fn render_hub(
    plan: &SitePlan,
    date: &DateEntry,
    config: &PublicationConfig,
    metadata: &PublicationMetadata,
    info: &SiteInfo,
    written: &WrittenPages,
) -> String {
    // What this date can actually offer.
    let stations: Vec<&StationEntry> = plan
        .stations
        .iter()
        .filter(|station| written.contains(&plan.timetable_path(station, date)))
        .collect();
    let lines: Vec<(&LineEntry, Vec<&WindowEntry>)> = plan
        .lines
        .iter()
        .filter_map(|line| {
            let windows: Vec<&WindowEntry> = plan
                .windows
                .iter()
                .filter(|window| written.contains(&plan.diagram_path(line, date, window)))
                .collect();
            (!windows.is_empty()).then_some((line, windows))
        })
        .collect();

    let labels = Labels::for_language(config.language);
    let theme = crate::page::theme_block(config);
    let mut out = String::with_capacity(96 * 1024);

    crate::page::head(
        &mut out,
        &format!("{} \u{00B7} {}", info.title, date.short),
        config.language.tag(),
        &theme,
        include_str!("../assets/hub.css"),
    );

    // Masthead.
    out.push_str("<header class=\"masthead\">\n<h1>");
    out.push_str(&escape::text(&info.title));
    out.push_str("</h1>\n<p class=\"subtitle\">");
    out.push_str(&escape::text(&format!(
        "{} {} \u{00B7} {} stations \u{00B7} {} lines",
        labels.service_date,
        date.short,
        stations.len(),
        lines.len()
    )));
    out.push_str("</p>\n</header>\n");

    out.push_str(
        "<p class=\"hub-intro\">Pick a station for its departure timetable, \
                  or a line for its train diagram. Every page is one self-contained \
                  file: it prints, it works offline once loaded, and it makes no \
                  network request.",
    );
    if let Some(href) = &info.board_href {
        out.push_str(" For departures right now, see the <a href=\"");
        out.push_str(&escape::attr(href));
        out.push_str("\">");
        out.push_str(&escape::text(&info.board_label));
        out.push_str("</a>.");
    }
    out.push_str("</p>\n");

    render_date_tabs(&mut out, plan, date);
    render_lines(&mut out, plan, date, &lines);
    render_stations(&mut out, plan, date, &stations);

    crate::page::colophon(&mut out, metadata, labels, plan);
    out.push_str("<script>\n");
    out.push_str(include_str!("../assets/hub.js"));
    out.push_str("\n</script>\n");
    crate::page::foot(&mut out);
    out
}

fn render_date_tabs(out: &mut String, plan: &SitePlan, current: &DateEntry) {
    if plan.dates.len() < 2 {
        return;
    }
    out.push_str("<nav aria-label=\"Service date\">\n<ul class=\"date-tabs\">\n");
    for entry in &plan.dates {
        let here = entry.key == current.key;
        out.push_str("<li><a");
        if here {
            out.push_str(" class=\"current\" aria-current=\"page\"");
        }
        out.push_str(" href=\"");
        out.push_str(&escape::attr(&hub_name(plan, entry)));
        out.push_str("\"><span class=\"relation\">");
        out.push_str(&escape::text(&entry.relation));
        out.push_str("</span><span class=\"full\">");
        out.push_str(&escape::text(&entry.day_month));
        out.push_str("</span></a></li>\n");
    }
    out.push_str("</ul>\n</nav>\n");
}

/// Get the file name of the hub for a date.
///
/// The first date owns `index.html`, so the section has an entry
/// point that needs no query string.
pub fn hub_name(plan: &SitePlan, date: &DateEntry) -> String {
    if date.key == plan.first_date().key {
        "index.html".to_string()
    } else {
        format!("day-{}.html", date.key)
    }
}

fn render_lines(
    out: &mut String,
    plan: &SitePlan,
    date: &DateEntry,
    lines: &[(&LineEntry, Vec<&WindowEntry>)],
) {
    if lines.is_empty() {
        return;
    }
    out.push_str(
        "<section class=\"hub-section\" aria-labelledby=\"lines-heading\">\n\
         <h2 id=\"lines-heading\">Train diagrams</h2>\n<ul class=\"line-cards\">\n",
    );
    for (line, windows) in lines {
        out.push_str("<li class=\"line-card\"");
        if let Some(color) = &line.color {
            if let Some(safe) = escape::css_color(color) {
                out.push_str(" style=\"--line-color: ");
                out.push_str(&safe);
                out.push('"');
            }
        }
        out.push_str(">\n<h3>");
        out.push_str(&escape::text(&line.name));
        out.push_str("</h3>\n");
        if let Some(long) = &line.long_name {
            out.push_str("<p class=\"long\">");
            out.push_str(&escape::text(long));
            out.push_str("</p>\n");
        }
        out.push_str("<ul>\n");
        for window in windows {
            out.push_str("<li><a href=\"");
            out.push_str(&escape::attr(&plan.diagram_path(line, date, window)));
            out.push_str("\" title=\"");
            out.push_str(&escape::attr(&format!(
                "{} {} \u{00B7} {}",
                line.name, window.label, date.short
            )));
            out.push_str("\">");
            out.push_str(&escape::text(&window.label));
            out.push_str("</a></li>\n");
        }
        out.push_str("</ul>\n</li>\n");
    }
    out.push_str("</ul>\n</section>\n");
}

fn render_stations(
    out: &mut String,
    plan: &SitePlan,
    date: &DateEntry,
    stations: &[&StationEntry],
) {
    out.push_str(
        "<section class=\"hub-section\" aria-labelledby=\"stations-heading\">\n\
         <h2 id=\"stations-heading\">Departure timetables</h2>\n",
    );

    // The box starts hidden and the script reveals it, so a visitor
    // without JavaScript never sees a search field that cannot search.
    out.push_str(
        "<div class=\"search\" hidden>\n<label for=\"station-search\">Find a station</label>\n",
    );
    out.push_str(
        "<input type=\"search\" id=\"station-search\" autocomplete=\"off\" \
         placeholder=\"Name or code, for example Jurong East or NS1\">\n",
    );
    out.push_str("<p class=\"count\" id=\"station-count\">");
    out.push_str(&escape::text(&format!("{} stations", stations.len())));
    out.push_str("</p>\n</div>\n");

    if stations.is_empty() {
        out.push_str(
            "<p class=\"no-matches\">No station timetable is available for this service date.</p>\n</section>\n",
        );
        return;
    }

    // The line colour of the first line of a station tints its code
    // chips, which is how a reader finds a line in a long list.
    let color_of = |route_id: &str| -> Option<&str> {
        plan.lines
            .iter()
            .find(|line| line.route_id == route_id)
            .and_then(|line| line.color.as_deref())
    };

    out.push_str("<ul class=\"stations\" id=\"station-list\">\n");
    for station in stations {
        out.push_str("<li class=\"station-row\" data-search=\"");
        out.push_str(&escape::attr(&station.search));
        out.push_str("\"><a href=\"");
        out.push_str(&escape::attr(&plan.timetable_path(station, date)));
        out.push_str("\"><span class=\"codes\">");
        for (index, code) in station.codes.iter().enumerate() {
            let color = station
                .lines
                .get(index)
                .or_else(|| station.lines.first())
                .and_then(|route_id| color_of(route_id))
                .and_then(escape::css_color);
            out.push_str("<span class=\"code\"");
            if let Some(color) = color {
                out.push_str(" style=\"--line-color: ");
                out.push_str(&color);
                out.push('"');
            }
            out.push('>');
            out.push_str(&escape::text(code));
            out.push_str("</span>");
        }
        out.push_str("</span><span class=\"name\">");
        out.push_str(&escape::text(&station.name));
        out.push_str("</span></a></li>\n");
    }
    out.push_str("</ul>\n");
    out.push_str(
        "<p class=\"no-matches\" id=\"no-matches\" hidden>No station matches that search.</p>\n",
    );
    out.push_str("</section>\n");
}
