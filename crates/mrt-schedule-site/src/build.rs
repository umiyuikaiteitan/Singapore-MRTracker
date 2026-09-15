//! Build the whole site section.
//!
//! # Why two passes
//!
//! A page can fail: the feed may not support it, or the write may not
//! land. Whatever fails must leave no link behind, and a link is only
//! safe to write once the set of files that exist is closed. So the
//! build runs in two passes:
//!
//! 1. **Content.** Every timetable, diagram, and drawing is built and
//!    written *without* its navigation block. What lands is the
//!    manifest: [`WrittenPages`].
//! 2. **Navigation.** The hubs are rendered from that manifest, and
//!    the navigation block of every surviving page is rendered from it
//!    too and spliced into the file.
//!
//! Nothing in the first pass links anywhere, and nothing in the second
//! pass links to a path that is not in the manifest. A page that
//! failed therefore cannot be reached from any file in the section.

use std::path::Path;

use mrt_gtfs::{RailNetwork, ServiceDate};
use mrt_publication::{
    build_diagram, build_timetable, DiagramTarget, DocumentSeed, PublicationConfig,
    PublicationMetadata,
};
use mrt_publication_html::{
    nav as html_nav, render_diagram_svg, render_diagram_with_nav, render_timetable_with_nav,
    NavGroup, NavLink, PageNav,
};
use mrt_schedule_cli::fsutil::write_atomic_str;

use crate::hub::{hub_name, render_hub, SiteInfo};
use crate::plan::{DateEntry, LineEntry, SitePlan, StationEntry, WindowEntry};

/// Where the navigation block belongs in a rendered document.
///
/// `mrt-publication-html` writes the block immediately after the
/// opening `<body>`, so splicing it in at this marker produces exactly
/// the bytes that rendering the page with its navigation would have.
const NAV_ANCHOR: &str = "</head>\n<body>\n<div class=\"page\">\n";

/// What the builder wrote, and what it could not.
#[derive(Clone, Debug, Default)]
pub struct BuildReport {
    /// How many files reached the output directory, of every kind.
    pub files: usize,
    /// How many of those files are content: a station timetable, a
    /// train diagram, or a standalone drawing.
    ///
    /// The rest is infrastructure — the hubs and `data/index.json` —
    /// which a build writes even when it produced nothing to read. A
    /// section with no content file is empty however many files it
    /// holds, so this is the count the empty-site guard looks at.
    pub content_files: usize,
    /// How many bytes they hold.
    pub bytes: u64,
    /// The pages that could not be built, with the reason.
    pub failures: Vec<String>,
    /// The site-relative paths of the planned pages that are
    /// consequently absent. Nothing in the site links to any of them.
    pub missing: Vec<String>,
}

/// The site-relative paths of the files a build actually wrote.
///
/// Every internal link in the section — the hub lists, the date tabs,
/// and the navigation block of each page — is filtered through this
/// set rather than through the plan, so a file that failed leaves no
/// dangling link behind.
#[derive(Clone, Debug, Default)]
pub struct WrittenPages {
    paths: std::collections::BTreeSet<String>,
}

impl WrittenPages {
    fn insert(&mut self, path: String) {
        self.paths.insert(path);
    }

    /// Report whether the file at this site-relative path was written.
    pub fn contains(&self, path: &str) -> bool {
        self.paths.contains(path)
    }
}

/// Decide whether a build with failed pages may still exit zero.
///
/// `MRT_SITE_ALLOW_PARTIAL=1` is the only accepted value: the opt-in
/// must be deliberate, not a leftover truthy string.
pub fn accepts_partial(value: Option<&str>) -> bool {
    value == Some("1")
}

/// What a finished build amounts to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Every planned page was written.
    Complete,
    /// Pages are missing, and the opt-in accepts that.
    AcceptedPartial,
    /// Pages are missing and nothing accepted that.
    Incomplete,
    /// No content page survived, so there is no site to publish.
    Empty,
}

impl Verdict {
    /// Judge a finished build.
    ///
    /// An empty section is a failure whatever the opt-in says: the
    /// opt-in accepts gaps in a site, not the absence of one. Hubs and
    /// `data/index.json` are written for any build, so counting files
    /// cannot tell an empty section from a full one — only the content
    /// files can.
    pub fn of(report: &BuildReport, allow_partial: bool) -> Verdict {
        if report.content_files == 0 {
            Verdict::Empty
        } else if report.failures.is_empty() {
            Verdict::Complete
        } else if allow_partial {
            Verdict::AcceptedPartial
        } else {
            Verdict::Incomplete
        }
    }

    /// Report whether the build may be deployed.
    pub fn is_publishable(self) -> bool {
        matches!(self, Verdict::Complete | Verdict::AcceptedPartial)
    }
}

/// A page that the first pass wrote and the second pass must finish.
enum NavTarget<'a> {
    Timetable(&'a StationEntry, &'a DateEntry),
    Diagram(&'a LineEntry, &'a DateEntry, &'a WindowEntry),
}

/// Everything the builder needs.
pub struct SiteBuild<'a> {
    /// The rail network.
    pub network: &'a RailNetwork,
    /// The presentation configuration.
    pub config: &'a PublicationConfig,
    /// The provenance of the feed.
    pub seed: &'a DocumentSeed,
    /// What the site says about itself.
    pub info: &'a SiteInfo,
    /// The pages to build.
    pub plan: &'a SitePlan,
}

impl<'a> SiteBuild<'a> {
    /// Write the whole section into `out`.
    ///
    /// A page that cannot be built or written is recorded in the
    /// report and reaches no hub and no navigation block, so the site
    /// never links to a file that does not exist. A hub or the index
    /// that cannot be written is fatal instead: without them the
    /// section has no entry point, and no partial site can stand.
    ///
    /// When no content page survives at all, the build writes no hub
    /// and no index: an entry point over nothing is worse than an
    /// empty directory, because it looks like a site. The report then
    /// carries `content_files == 0`, which the caller must treat as a
    /// failure.
    pub fn write(&self, out: &Path) -> Result<BuildReport, String> {
        let plan = self.plan;
        let mut report = BuildReport::default();
        let mut written = WrittenPages::default();
        std::fs::create_dir_all(out.join("t"))
            .and_then(|_| std::fs::create_dir_all(out.join("d")))
            .map_err(|e| format!("cannot create {}: {e}", out.display()))?;

        // Pass one: the content, with no navigation yet. A real feed
        // makes this the long part of a Pages run, so it reports
        // progress rather than going quiet for minutes.
        let total = plan.page_count();
        let mut done = 0usize;
        let mut pending: Vec<NavTarget<'a>> = Vec::with_capacity(total);
        for date in &plan.dates {
            for station in &plan.stations {
                if self.write_timetable(out, station, date, &mut report, &mut written) {
                    pending.push(NavTarget::Timetable(station, date));
                }
                done += 1;
                self.progress(done, total, "pages");
            }
            for line in &plan.lines {
                for window in &plan.windows {
                    if self.write_diagram(out, line, date, window, &mut report, &mut written) {
                        pending.push(NavTarget::Diagram(line, date, window));
                    }
                    done += 1;
                    self.progress(done, total, "pages");
                }
            }
        }

        // Nothing to link to: write no entry point over an empty
        // section. The caller fails the build on this count.
        if report.content_files == 0 {
            return Ok(report);
        }

        // The hubs: they link only to the pages that exist.
        for date in &plan.dates {
            let metadata = PublicationMetadata::new(self.seed, date.date, Vec::new());
            let page = render_hub(plan, date, self.config, &metadata, self.info, &written);
            let name = hub_name(plan, date);
            let path = out.join(&name);
            write_atomic_str(&path, &page)
                .map_err(|e| format!("cannot write the hub {}: {e}", path.display()))?;
            report.files += 1;
            report.bytes += page.len() as u64;
            written.insert(name);
        }

        // Pass two: the navigation, now that the set of files is
        // closed. Every href it emits names a path in `written`.
        let pass_two = pending.len();
        for (index, target) in pending.iter().enumerate() {
            self.splice_nav(out, target, &written, &mut report);
            self.progress(index + 1, pass_two, "navigation blocks");
        }

        let index = serde_json::to_string_pretty(&SiteIndex {
            schema_version: mrt_publication::SCHEMA_VERSION,
            feed_sha256: &self.seed.feed_sha256,
            feed_timestamp: self.seed.feed_timestamp.as_deref(),
            timezone: &self.seed.timezone,
            missing: &report.missing,
            plan,
        })
        .map_err(|e| format!("cannot serialize the site index: {e}"))?;
        let path = out.join("data/index.json");
        write_atomic_str(&path, &index)
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        report.files += 1;
        report.bytes += index.len() as u64;

        Ok(report)
    }

    /// Report progress on a round number of items.
    fn progress(&self, done: usize, total: usize, what: &str) {
        let step = (total / 10).max(50);
        if done % step == 0 || done == total {
            eprintln!("  {done}/{total} {what}");
        }
    }

    /// Build and write one station timetable, without its navigation.
    ///
    /// Returns whether the page landed, so the second pass knows which
    /// files are still there to finish.
    fn write_timetable(
        &self,
        out: &Path,
        station: &StationEntry,
        date: &DateEntry,
        report: &mut BuildReport,
        written: &mut WrittenPages,
    ) -> bool {
        let relative = self.plan.timetable_path(station, date);
        let document = match build_timetable(
            self.network,
            station.id,
            date.date,
            None,
            self.config,
            self.seed,
        ) {
            Ok(document) => document,
            Err(error) => {
                report
                    .failures
                    .push(format!("{} on {}: {error}", station.name, date.iso));
                report.missing.push(relative);
                return false;
            }
        };
        let page = render_timetable_with_nav(&document, self.config, None);
        self.write_content(out, relative, &page, report, written)
    }

    /// Build and write one diagram page and its drawing.
    ///
    /// The page needs a navigation block and the drawing does not, so
    /// only the page is reported back as pending.
    fn write_diagram(
        &self,
        out: &Path,
        line: &LineEntry,
        date: &DateEntry,
        window: &WindowEntry,
        report: &mut BuildReport,
        written: &mut WrittenPages,
    ) -> bool {
        let page_path = self.plan.diagram_path(line, date, window);
        let drawing_path = self.plan.drawing_path(line, date, window);
        let document = match build_diagram(
            self.network,
            &DiagramTarget::Line(line.id),
            date.date,
            window.from,
            window.until,
            self.config,
            self.seed,
        ) {
            Ok(document) => document,
            Err(error) => {
                report.failures.push(format!(
                    "{} {} on {}: {error}",
                    line.name, window.label, date.iso
                ));
                report.missing.push(page_path);
                report.missing.push(drawing_path);
                return false;
            }
        };
        let page = render_diagram_with_nav(&document, self.config, None);
        let landed = self.write_content(out, page_path, &page, report, written);
        let drawing = render_diagram_svg(&document, self.config);
        self.write_content(out, drawing_path, &drawing, report, written);
        landed
    }

    /// Splice the navigation block into a page the first pass wrote.
    ///
    /// A page whose block cannot be written keeps exactly what the
    /// first pass produced: a complete, self-contained document with
    /// no navigation. It is still a file that every link to it
    /// resolves to, so the failure costs navigation, not integrity.
    fn splice_nav(
        &self,
        out: &Path,
        target: &NavTarget<'_>,
        written: &WrittenPages,
        report: &mut BuildReport,
    ) {
        let (relative, nav) = match target {
            NavTarget::Timetable(station, date) => (
                self.plan.timetable_path(station, date),
                self.timetable_nav(station, date, written),
            ),
            NavTarget::Diagram(line, date, window) => (
                self.plan.diagram_path(line, date, window),
                self.diagram_nav(line, date, window, written),
            ),
        };
        let mut block = String::new();
        html_nav::render(&mut block, &nav);
        if block.is_empty() {
            return;
        }

        let path = out.join(&relative);
        let body = match std::fs::read_to_string(&path) {
            Ok(body) => body,
            Err(error) => {
                report
                    .failures
                    .push(format!("{relative}: cannot re-read the page: {error}"));
                return;
            }
        };
        let Some(anchor) = body.find(NAV_ANCHOR).map(|at| at + NAV_ANCHOR.len()) else {
            report
                .failures
                .push(format!("{relative}: the page has no place for navigation"));
            return;
        };

        let mut page = String::with_capacity(body.len() + block.len());
        page.push_str(&body[..anchor]);
        page.push_str(&block);
        page.push_str(&body[anchor..]);
        match write_atomic_str(&path, &page) {
            Ok(()) => report.bytes += block.len() as u64,
            Err(error) => report
                .failures
                .push(format!("{relative}: cannot add the navigation: {error}")),
        }
    }

    /// Build the navigation of a station timetable.
    ///
    /// Every link is relative to the page itself, which lives one
    /// directory below the hub, and every link names a file that this
    /// build wrote.
    fn timetable_nav(
        &self,
        station: &StationEntry,
        date: &DateEntry,
        written: &WrittenPages,
    ) -> PageNav {
        let mut groups = Vec::new();
        let dates: Vec<NavLink> = self
            .plan
            .dates
            .iter()
            .filter_map(|entry| {
                let path = self.plan.timetable_path(station, entry);
                written.contains(&path).then(|| {
                    NavLink::new(format!("../{path}"), entry.relation.clone())
                        .titled(entry.short.clone())
                        .current(entry.key == date.key)
                })
            })
            .collect();
        groups.extend(nav_group("Date", dates));

        // The diagrams of the lines that serve this station, in the
        // window that covers the morning.
        let first_window = self.plan.windows.first();
        let diagrams: Vec<NavLink> = station
            .lines
            .iter()
            .filter_map(|route_id| {
                let line = self
                    .plan
                    .lines
                    .iter()
                    .find(|entry| &entry.route_id == route_id)?;
                let window = first_window?;
                let path = self.plan.diagram_path(line, date, window);
                written.contains(&path).then(|| {
                    NavLink::new(format!("../{path}"), line.name.clone())
                        .titled(format!("{} train diagram", line.name))
                })
            })
            .collect();
        groups.extend(nav_group("Diagram", diagrams));

        PageNav {
            home: self.home_link(date, written),
            site_name: Some(self.info.title.clone()),
            groups,
        }
    }

    fn diagram_nav(
        &self,
        line: &LineEntry,
        date: &DateEntry,
        window: &WindowEntry,
        written: &WrittenPages,
    ) -> PageNav {
        let mut groups = Vec::new();
        let dates: Vec<NavLink> = self
            .plan
            .dates
            .iter()
            .filter_map(|entry| {
                let path = self.plan.diagram_path(line, entry, window);
                written.contains(&path).then(|| {
                    NavLink::new(format!("../{path}"), entry.relation.clone())
                        .titled(entry.short.clone())
                        .current(entry.key == date.key)
                })
            })
            .collect();
        groups.extend(nav_group("Date", dates));

        let windows: Vec<NavLink> = self
            .plan
            .windows
            .iter()
            .filter_map(|entry| {
                let path = self.plan.diagram_path(line, date, entry);
                written.contains(&path).then(|| {
                    NavLink::new(format!("../{path}"), entry.label.clone())
                        .current(entry.key == window.key)
                })
            })
            .collect();
        groups.extend(nav_group("Window", windows));

        let lines: Vec<NavLink> = self
            .plan
            .lines
            .iter()
            .filter_map(|entry| {
                let path = self.plan.diagram_path(entry, date, window);
                written.contains(&path).then(|| {
                    NavLink::new(format!("../{path}"), entry.name.clone())
                        .titled(
                            entry
                                .long_name
                                .clone()
                                .unwrap_or_else(|| entry.name.clone()),
                        )
                        .current(entry.key == line.key)
                })
            })
            .collect();
        groups.extend(nav_group("Line", lines));

        let drawing = self.plan.drawing_path(line, date, window);
        let drawings: Vec<NavLink> = written
            .contains(&drawing)
            .then(|| {
                NavLink::new(format!("../{drawing}"), "SVG")
                    .titled("The drawing on its own, as a standalone file")
            })
            .into_iter()
            .collect();
        groups.extend(nav_group("Drawing", drawings));

        PageNav {
            home: self.home_link(date, written),
            site_name: Some(self.info.title.clone()),
            groups,
        }
    }

    /// The link back to the hub of a service date, when it exists.
    fn home_link(&self, date: &DateEntry, written: &WrittenPages) -> Option<NavLink> {
        let hub = hub_name(self.plan, date);
        written
            .contains(&hub)
            .then(|| NavLink::new(format!("../{hub}"), "All stations"))
    }

    /// Write one content file, recording either the success or the gap.
    fn write_content(
        &self,
        out: &Path,
        relative: String,
        body: &str,
        report: &mut BuildReport,
        written: &mut WrittenPages,
    ) -> bool {
        match write_atomic_str(&out.join(&relative), body) {
            Ok(()) => {
                report.files += 1;
                report.content_files += 1;
                report.bytes += body.len() as u64;
                written.insert(relative);
                true
            }
            Err(error) => {
                report.failures.push(format!("{relative}: {error}"));
                report.missing.push(relative);
                false
            }
        }
    }
}

/// Make a navigation group, unless it would lead nowhere.
///
/// A row that holds nothing but the reader's own page is not
/// navigation, so it is dropped: that is what keeps the date row off a
/// single-date site, and what removes a row whose other targets all
/// failed.
fn nav_group(label: &str, links: Vec<NavLink>) -> Option<NavGroup> {
    let useful = links.iter().any(|link| !link.current);
    useful.then(|| NavGroup::new(label, links))
}

/// The machine-readable index of the site.
#[derive(serde::Serialize)]
struct SiteIndex<'a> {
    schema_version: &'a str,
    feed_sha256: &'a str,
    feed_timestamp: Option<&'a str>,
    timezone: &'a str,
    /// The planned pages that this build could not produce.
    missing: &'a [String],
    #[serde(flatten)]
    plan: &'a SitePlan,
}

/// Get today's date in a fixed time zone offset.
///
/// The site is built in an Actions runner that runs on UTC, and the
/// service date that matters is the one in Singapore.
pub fn today_at_offset(unix_seconds: i64, offset_seconds: i64) -> ServiceDate {
    let epoch: ServiceDate = "19700101".parse().expect("the epoch is a valid date");
    epoch.plus_days((unix_seconds + offset_seconds).div_euclid(86_400))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_service_date_follows_singapore_time() {
        // 2026-08-10 15:59:59 UTC is 23:59:59 in Singapore.
        assert_eq!(
            today_at_offset(1_786_377_599, 8 * 3600).to_string(),
            "20260810"
        );
        assert_eq!(
            today_at_offset(1_786_377_600, 8 * 3600).to_string(),
            "20260811"
        );
        // The same instant in UTC is still the tenth.
        assert_eq!(today_at_offset(1_786_377_600, 0).to_string(), "20260810");
    }

    #[test]
    fn only_an_explicit_opt_in_accepts_a_partial_site() {
        assert!(accepts_partial(Some("1")));
        assert!(!accepts_partial(None));
        assert!(!accepts_partial(Some("")));
        assert!(!accepts_partial(Some("0")));
        assert!(!accepts_partial(Some("true")));
        assert!(!accepts_partial(Some("yes")));
    }

    #[test]
    fn an_empty_site_fails_whatever_the_opt_in_says() {
        let empty = BuildReport {
            files: 0,
            content_files: 0,
            failures: vec!["everything".into()],
            ..Default::default()
        };
        assert_eq!(Verdict::of(&empty, false), Verdict::Empty);
        assert_eq!(Verdict::of(&empty, true), Verdict::Empty);
        assert!(!Verdict::of(&empty, true).is_publishable());

        // Hubs and the index alone are not a site either.
        let hubs_only = BuildReport {
            files: 3,
            content_files: 0,
            ..Default::default()
        };
        assert_eq!(Verdict::of(&hubs_only, true), Verdict::Empty);

        // One surviving page is, and the opt-in decides the rest.
        let partial = BuildReport {
            files: 4,
            content_files: 1,
            failures: vec!["one page".into()],
            ..Default::default()
        };
        assert_eq!(Verdict::of(&partial, false), Verdict::Incomplete);
        assert!(!Verdict::of(&partial, false).is_publishable());
        assert_eq!(Verdict::of(&partial, true), Verdict::AcceptedPartial);
        assert!(Verdict::of(&partial, true).is_publishable());

        let whole = BuildReport {
            files: 4,
            content_files: 3,
            ..Default::default()
        };
        assert_eq!(Verdict::of(&whole, false), Verdict::Complete);
        assert!(Verdict::of(&whole, false).is_publishable());
    }

    #[test]
    fn a_group_that_leads_nowhere_is_dropped() {
        // Only the page the reader is already on.
        assert!(nav_group("Date", vec![NavLink::new("a.html", "Today").current(true)]).is_none());
        assert!(nav_group("Date", Vec::new()).is_none());
        // One other page is enough to be worth a row.
        let group = nav_group(
            "Date",
            vec![
                NavLink::new("a.html", "Today").current(true),
                NavLink::new("b.html", "Tomorrow"),
            ],
        )
        .expect("a row with another target survives");
        assert_eq!(group.links.len(), 2);
    }
}
