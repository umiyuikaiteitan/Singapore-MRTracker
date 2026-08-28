//! Build the whole site section.

use std::path::Path;

use mrt_gtfs::{RailNetwork, ServiceDate};
use mrt_publication::{
    build_diagram, build_timetable, DiagramTarget, DocumentSeed, PublicationConfig,
    PublicationMetadata,
};
use mrt_publication_html::{
    render_diagram_svg, render_diagram_with_nav, render_timetable_with_nav, NavGroup, NavLink,
    PageNav,
};
use mrt_schedule_cli::fsutil::write_atomic_str;

use crate::hub::{hub_name, render_hub, SiteInfo};
use crate::plan::{DateEntry, LineEntry, SitePlan, StationEntry, WindowEntry};

/// What the builder wrote, and what it could not.
#[derive(Clone, Debug, Default)]
pub struct BuildReport {
    /// How many files reached the output directory.
    pub files: usize,
    /// How many bytes they hold.
    pub bytes: u64,
    /// The pages that could not be built, with the reason.
    pub failures: Vec<String>,
    /// The site-relative paths of the planned pages that are
    /// consequently absent. No hub links to any of them.
    pub missing: Vec<String>,
}

/// The site-relative paths of the pages a build actually wrote.
///
/// The hub is rendered from this set rather than from the plan, so a
/// page that failed leaves no dangling link behind.
#[derive(Clone, Debug, Default)]
pub struct WrittenPages {
    paths: std::collections::BTreeSet<String>,
}

impl WrittenPages {
    fn insert(&mut self, path: String) {
        self.paths.insert(path);
    }

    /// Report whether the page at this site-relative path was written.
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

impl SiteBuild<'_> {
    /// Write the whole section into `out`.
    ///
    /// A page that cannot be built or written is recorded in the
    /// report and dropped from every hub, so the site never links to a
    /// file that does not exist. A hub or the index that cannot be
    /// written is fatal instead: without them the section has no entry
    /// point, and no partial site can stand.
    pub fn write(&self, out: &Path) -> Result<BuildReport, String> {
        let mut report = BuildReport::default();
        let mut written = WrittenPages::default();
        std::fs::create_dir_all(out.join("t"))
            .and_then(|_| std::fs::create_dir_all(out.join("d")))
            .map_err(|e| format!("cannot create {}: {e}", out.display()))?;

        // A real feed makes this the long part of a Pages run, so it
        // reports progress rather than going quiet for minutes.
        let total = self.plan.page_count();
        let mut done = 0usize;
        for date in &self.plan.dates {
            for station in &self.plan.stations {
                self.write_timetable(out, station, date, &mut report, &mut written);
                done += 1;
                self.progress(done, total);
            }
            for line in &self.plan.lines {
                for window in &self.plan.windows {
                    self.write_diagram(out, line, date, window, &mut report, &mut written);
                    done += 1;
                    self.progress(done, total);
                }
            }
        }

        // The hubs last: they link only to the pages that exist.
        for date in &self.plan.dates {
            let metadata = PublicationMetadata::new(self.seed, date.date, Vec::new());
            let page = render_hub(self.plan, date, self.config, &metadata, self.info, &written);
            let path = out.join(hub_name(self.plan, date));
            write_atomic_str(&path, &page)
                .map_err(|e| format!("cannot write the hub {}: {e}", path.display()))?;
            report.files += 1;
            report.bytes += page.len() as u64;
        }

        let index = serde_json::to_string_pretty(&SiteIndex {
            schema_version: mrt_publication::SCHEMA_VERSION,
            feed_sha256: &self.seed.feed_sha256,
            feed_timestamp: self.seed.feed_timestamp.as_deref(),
            timezone: &self.seed.timezone,
            missing: &report.missing,
            plan: self.plan,
        })
        .map_err(|e| format!("cannot serialize the site index: {e}"))?;
        let path = out.join("data/index.json");
        write_atomic_str(&path, &index)
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        report.files += 1;
        report.bytes += index.len() as u64;

        Ok(report)
    }

    /// Report progress on a round number of pages.
    fn progress(&self, done: usize, total: usize) {
        let step = (total / 10).max(50);
        if done % step == 0 || done == total {
            eprintln!("  {done}/{total} pages");
        }
    }

    fn write_timetable(
        &self,
        out: &Path,
        station: &StationEntry,
        date: &DateEntry,
        report: &mut BuildReport,
        written: &mut WrittenPages,
    ) {
        let relative = self.plan.timetable_path(station, date);
        let nav = self.timetable_nav(station, date);
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
                return;
            }
        };
        let page = render_timetable_with_nav(&document, self.config, Some(&nav));
        self.write_page(out, relative, &page, report, written);
    }

    fn write_diagram(
        &self,
        out: &Path,
        line: &LineEntry,
        date: &DateEntry,
        window: &WindowEntry,
        report: &mut BuildReport,
        written: &mut WrittenPages,
    ) {
        let page_path = self.plan.diagram_path(line, date, window);
        let drawing_path = self.plan.drawing_path(line, date, window);
        let nav = self.diagram_nav(line, date, window);
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
                return;
            }
        };
        let page = render_diagram_with_nav(&document, self.config, Some(&nav));
        self.write_page(out, page_path, &page, report, written);
        let drawing = render_diagram_svg(&document, self.config);
        self.write_page(out, drawing_path, &drawing, report, written);
    }

    /// Build the navigation of a station timetable.
    ///
    /// Every link is relative to the page itself, which lives one
    /// directory below the hub.
    fn timetable_nav(&self, station: &StationEntry, date: &DateEntry) -> PageNav {
        let mut groups = Vec::new();
        if self.plan.dates.len() > 1 {
            groups.push(NavGroup::new(
                "Date",
                self.plan
                    .dates
                    .iter()
                    .map(|entry| {
                        NavLink::new(
                            format!("../{}", self.plan.timetable_path(station, entry)),
                            entry.relation.clone(),
                        )
                        .titled(entry.short.clone())
                        .current(entry.key == date.key)
                    })
                    .collect(),
            ));
        }

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
                Some(
                    NavLink::new(
                        format!("../{}", self.plan.diagram_path(line, date, window)),
                        line.name.clone(),
                    )
                    .titled(format!("{} train diagram", line.name)),
                )
            })
            .collect();
        if !diagrams.is_empty() {
            groups.push(NavGroup::new("Diagram", diagrams));
        }

        PageNav {
            home: Some(NavLink::new(
                format!("../{}", hub_name(self.plan, date)),
                "All stations",
            )),
            site_name: Some(self.info.title.clone()),
            groups,
        }
    }

    fn diagram_nav(&self, line: &LineEntry, date: &DateEntry, window: &WindowEntry) -> PageNav {
        let mut groups = Vec::new();
        if self.plan.dates.len() > 1 {
            groups.push(NavGroup::new(
                "Date",
                self.plan
                    .dates
                    .iter()
                    .map(|entry| {
                        NavLink::new(
                            format!("../{}", self.plan.diagram_path(line, entry, window)),
                            entry.relation.clone(),
                        )
                        .titled(entry.short.clone())
                        .current(entry.key == date.key)
                    })
                    .collect(),
            ));
        }
        groups.push(NavGroup::new(
            "Window",
            self.plan
                .windows
                .iter()
                .map(|entry| {
                    NavLink::new(
                        format!("../{}", self.plan.diagram_path(line, date, entry)),
                        entry.label.clone(),
                    )
                    .current(entry.key == window.key)
                })
                .collect(),
        ));
        if self.plan.lines.len() > 1 {
            groups.push(NavGroup::new(
                "Line",
                self.plan
                    .lines
                    .iter()
                    .map(|entry| {
                        NavLink::new(
                            format!("../{}", self.plan.diagram_path(entry, date, window)),
                            entry.name.clone(),
                        )
                        .titled(
                            entry
                                .long_name
                                .clone()
                                .unwrap_or_else(|| entry.name.clone()),
                        )
                        .current(entry.key == line.key)
                    })
                    .collect(),
            ));
        }
        groups.push(NavGroup::new(
            "Drawing",
            vec![NavLink::new(
                format!("../{}", self.plan.drawing_path(line, date, window)),
                "SVG",
            )
            .titled("The drawing on its own, as a standalone file")],
        ));

        PageNav {
            home: Some(NavLink::new(
                format!("../{}", hub_name(self.plan, date)),
                "All stations",
            )),
            site_name: Some(self.info.title.clone()),
            groups,
        }
    }

    /// Write one planned page, recording either the success or the gap.
    fn write_page(
        &self,
        out: &Path,
        relative: String,
        body: &str,
        report: &mut BuildReport,
        written: &mut WrittenPages,
    ) {
        match write_atomic_str(&out.join(&relative), body) {
            Ok(()) => {
                report.files += 1;
                report.bytes += body.len() as u64;
                written.insert(relative);
            }
            Err(error) => {
                report.failures.push(format!("{relative}: {error}"));
                report.missing.push(relative);
            }
        }
    }
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
}
