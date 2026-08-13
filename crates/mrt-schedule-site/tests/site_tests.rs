//! End-to-end tests for the generated site.
//!
//! The tests build the whole section from the miniature feed and then
//! check the properties that make it a *site* rather than a folder:
//! every link resolves to a file that exists, every path is relative,
//! the hub works without JavaScript, and no page reaches the network.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mrt_gtfs::{GtfsFeed, RailNetwork, ServiceDate};
use mrt_publication::{DocumentSeed, PublicationConfig};
use mrt_schedule_site::{default_windows, SiteBuild, SiteInfo, SitePlan};

fn network() -> RailNetwork {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../mrt-gtfs/tests/fixtures/mini");
    RailNetwork::from_feed(&GtfsFeed::from_dir(dir).unwrap()).unwrap()
}

fn seed() -> DocumentSeed {
    DocumentSeed {
        generator_version: "site-test".into(),
        feed_sha256: "0".repeat(64),
        feed_timestamp: Some("2026-08-10T00:00:00+08:00".into()),
        timezone: "Asia/Singapore".into(),
        generated_from_cache: false,
        configuration_sha256: "0".repeat(64),
    }
}

/// The fixture runs weekday service, so the site starts on a Monday.
fn today() -> ServiceDate {
    "20250505".parse().unwrap()
}

struct Built {
    dir: tempfile::TempDir,
    plan: SitePlan,
    report: mrt_schedule_site::BuildReport,
}

impl Built {
    fn root(&self) -> PathBuf {
        self.dir.path().join("timetables")
    }

    fn read(&self, relative: &str) -> String {
        let path = self.root().join(relative);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
    }
}

fn build(days: u32) -> Built {
    let dir = tempfile::tempdir().unwrap();
    let network = network();
    let config = PublicationConfig::default();
    let seed = seed();
    let info = SiteInfo::default();
    let plan = SitePlan::build(&network, today(), days, default_windows());
    let report = SiteBuild {
        network: &network,
        config: &config,
        seed: &seed,
        info: &info,
        plan: &plan,
    }
    .write(&dir.path().join("timetables"))
    .unwrap();
    Built { dir, plan, report }
}

/// Collect every `href` of a page.
fn hrefs(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("href=\"") {
        let after = &rest[start + 6..];
        let Some(end) = after.find('"') else { break };
        out.push(after[..end].to_string());
        rest = &after[end..];
    }
    out
}

// ----------------------------------------------------------------------
// Structure
// ----------------------------------------------------------------------

#[test]
fn the_site_has_one_page_per_station_line_and_date() {
    let site = build(2);
    assert!(
        site.report.failures.is_empty(),
        "{:?}",
        site.report.failures
    );

    // 15 stations, 5 lines, 4 windows, 2 days, plus a hub per day and
    // the index. Each diagram also writes its standalone drawing.
    let expected = site.plan.stations.len() * 2 + site.plan.lines.len() * 4 * 2 * 2 + 2 + 1;
    assert_eq!(site.report.files, expected);
    assert!(site.root().join("index.html").exists());
    assert!(site.root().join("day-20250506.html").exists());
    assert!(site.root().join("t/te1-20250505.html").exists());
    assert!(site.root().join("d/te-20250505-morning.html").exists());
    assert!(site.root().join("d/te-20250505-morning.svg").exists());
    assert!(site.root().join("data/index.json").exists());
}

#[test]
fn every_link_resolves_to_a_file_that_exists() {
    let site = build(2);
    let mut checked = 0usize;

    let pages = [
        ("index.html", ""),
        ("day-20250506.html", ""),
        ("t/te1-20250505.html", "t"),
        ("d/te-20250505-morning.html", "d"),
    ];
    for (page, directory) in pages {
        let html = site.read(page);
        for href in hrefs(&html) {
            // The intro links out to the board, which this section
            // does not own.
            if href.starts_with("../index.html") {
                continue;
            }
            assert!(
                !href.starts_with('/') && !href.contains("://"),
                "{page} carries the absolute link {href}"
            );
            let base = if directory.is_empty() {
                site.root()
            } else {
                site.root().join(directory)
            };
            let target = normalize(&base.join(&href));
            assert!(
                target.exists(),
                "{page} links to {href}, which is not in the site"
            );
            checked += 1;
        }
    }
    assert!(checked > 50, "only {checked} links checked");
}

/// Resolve `..` without touching the file system, so the check works
/// for a path that does not exist yet.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[test]
fn the_first_date_owns_the_entry_point() {
    let site = build(3);
    let index = site.read("index.html");
    // The hub of today is index.html, and it marks itself current.
    assert!(index.contains("aria-current=\"page\""));
    assert!(index.contains(">Today<"));
    assert!(index.contains("href=\"day-20250506.html\""));
    // A single-date site emits no tab markup at all. (The rule stays
    // in the stylesheet; only the list disappears.)
    let single = build(1);
    assert!(!single
        .read("index.html")
        .contains("<ul class=\"date-tabs\">"));
}

// ----------------------------------------------------------------------
// The hub
// ----------------------------------------------------------------------

#[test]
fn the_hub_lists_every_station_and_line_in_the_document() {
    let site = build(1);
    let index = site.read("index.html");

    for station in &site.plan.stations {
        assert!(
            index.contains(&format!(">{}<", station.name)),
            "the hub does not list {}",
            station.name
        );
    }
    for line in &site.plan.lines {
        assert!(
            index.contains(&format!(">{}</h3>", line.name)),
            "the hub does not list {}",
            line.name
        );
    }
    // The list is markup, not a script that builds one, so the hub
    // works with JavaScript switched off.
    let list_start = index.find("id=\"station-list\"").unwrap();
    let script_start = index.rfind("<script>").unwrap();
    assert!(list_start < script_start);
    // And the search box stays hidden until a script can drive it.
    assert!(index.contains("<div class=\"search\" hidden>"));
}

#[test]
fn a_station_is_searchable_by_name_and_by_every_code() {
    let site = build(1);
    let index = site.read("index.html");
    // Jurong East is an interchange: both codes reach it.
    assert!(index.contains("data-search=\"jurong east ns1 ew24\""));
    // The key holds no punctuation, so "ns-1" and "NS 1" both match
    // after the script squashes the query the same way.
    for station in &site.plan.stations {
        assert!(
            station
                .search
                .chars()
                .all(|c| c.is_alphanumeric() || c == ' '),
            "{} has punctuation in its search key",
            station.name
        );
    }
}

#[test]
fn the_hub_links_back_to_the_live_board() {
    let site = build(1);
    let index = site.read("index.html");
    assert!(index.contains("href=\"../index.html\""));
    assert!(index.contains("Live departure board"));
}

// ----------------------------------------------------------------------
// The document pages
// ----------------------------------------------------------------------

#[test]
fn a_station_page_carries_navigation_and_the_timetable() {
    let site = build(2);
    let page = site.read("t/te1-20250505.html");

    assert!(page.contains("class=\"site-nav no-print\""));
    assert!(page.contains("href=\"../index.html\""));
    assert!(page.contains("href=\"../t/te1-20250506.html\""));
    // The diagram of the line that serves this station.
    assert!(page.contains("href=\"../d/te-20250505-morning.html\""));
    // And the timetable itself is still there.
    assert!(page.contains("Woodlands North departure timetable"));
    assert!(page.contains("class=\"hour-cell\""));
}

#[test]
fn a_diagram_page_switches_window_line_and_date() {
    let site = build(2);
    let page = site.read("d/te-20250505-morning.html");

    assert!(page.contains("href=\"../d/te-20250505-midday.html\""));
    assert!(page.contains("href=\"../d/te-20250506-morning.html\""));
    assert!(page.contains("href=\"../d/ns-20250505-morning.html\""));
    assert!(page.contains("href=\"../d/te-20250505-morning.svg\""));
    assert!(page.contains("<svg xmlns=\"http://www.w3.org/2000/svg\""));
}

#[test]
fn no_page_reaches_the_network() {
    let site = build(1);
    for page in [
        "index.html",
        "t/te1-20250505.html",
        "d/te-20250505-morning.html",
    ] {
        let html = site.read(page);
        assert!(html.contains("default-src &#39;none&#39;"), "{page}");
        for forbidden in ["<link ", "src=\"http", "@import", "url(http", "fetch("] {
            assert!(!html.contains(forbidden), "{page} contains {forbidden}");
        }
    }
}

#[test]
fn the_navigation_never_appears_on_paper() {
    let site = build(2);
    let page = site.read("t/te1-20250505.html");
    // The block carries `no-print`, and the print rules hide it.
    assert!(page.contains("class=\"site-nav no-print\""));
    assert!(page.contains(".no-print {\n    display: none !important;"));
}

// ----------------------------------------------------------------------
// The machine-readable index
// ----------------------------------------------------------------------

#[test]
fn the_index_json_describes_the_whole_site() {
    let site = build(2);
    let index: serde_json::Value = serde_json::from_str(&site.read("data/index.json")).unwrap();

    assert_eq!(index["schema_version"], "1.0");
    assert_eq!(index["timezone"], "Asia/Singapore");
    assert_eq!(index["dates"].as_array().unwrap().len(), 2);
    assert_eq!(
        index["stations"].as_array().unwrap().len(),
        site.plan.stations.len()
    );
    assert_eq!(index["windows"][0]["from"], "05:00:00");
    // No internal identifier leaks: the index names files and codes.
    let text = site.read("data/index.json");
    assert!(!text.contains("StationId"));
    assert!(!text.contains("\"id\""));
}

// ----------------------------------------------------------------------
// Determinism and file naming
// ----------------------------------------------------------------------

#[test]
fn two_builds_of_the_same_plan_agree() {
    let a = build(1);
    let b = build(1);
    for page in [
        "index.html",
        "t/te1-20250505.html",
        "d/te-20250505-morning.svg",
    ] {
        assert_eq!(a.read(page), b.read(page), "{page} differs between builds");
    }
}

#[test]
fn every_file_name_is_safe_in_a_url() {
    let site = build(2);
    let mut names = BTreeSet::new();
    let mut walk = vec![site.root()];
    while let Some(directory) = walk.pop() {
        for entry in std::fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk.push(path);
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')),
                "{name} needs escaping in a URL"
            );
            assert!(names.insert(path.clone()), "{name} was written twice");
        }
    }
    assert!(names.len() > 100);
}
