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
use mrt_schedule_site::{default_windows, SiteBuild, SiteInfo, SitePlan, Verdict};

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
    build_prepared(days, |_, _| {})
}

/// Build the site after `prepare` has had its way with the output
/// directory, so a test can sabotage individual target paths. The plan
/// is handed over too, so a test can sabotage all of them.
fn build_prepared(days: u32, prepare: impl FnOnce(&Path, &SitePlan)) -> Built {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("timetables");
    let network = network();
    let config = PublicationConfig::default();
    let seed = seed();
    let info = SiteInfo::default();
    let plan = SitePlan::build(&network, today(), days, default_windows());
    prepare(&root, &plan);
    let report = SiteBuild {
        network: &network,
        config: &config,
        seed: &seed,
        info: &info,
        plan: &plan,
    }
    .write(&root)
    .unwrap();
    Built { dir, plan, report }
}

/// Collect every `href` of a page.
fn hrefs(html: &str) -> Vec<String> {
    attributes(html, "href=\"")
}

/// Collect every value of one quoted attribute.
///
/// The generator writes its own markup with double-quoted attributes
/// and escapes every value, so scanning for the literal opener finds
/// exactly the links it emitted. This is not an HTML parser and does
/// not need to be.
fn attributes(html: &str, opener: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find(opener) {
        let after = &rest[start + opener.len()..];
        let Some(end) = after.find('"') else { break };
        out.push(after[..end].to_string());
        rest = &after[end..];
    }
    out
}

/// Every file the section holds, in a stable order.
fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut walk = vec![root.to_path_buf()];
    while let Some(directory) = walk.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Walk every generated HTML file and check every internal link.
///
/// Returns how many links were resolved, so a caller can assert that
/// the walk actually saw a site rather than an empty directory.
fn check_every_link(root: &Path) -> usize {
    let mut checked = 0usize;
    for path in files_under(root) {
        if path.extension().and_then(|e| e.to_str()) != Some("html") {
            continue;
        }
        let page = path.strip_prefix(root).unwrap().display().to_string();
        let html = std::fs::read_to_string(&path).unwrap();
        let base = path.parent().unwrap().to_path_buf();
        let mut links = hrefs(&html);
        links.extend(attributes(&html, "src=\""));
        for link in links {
            if link.starts_with('#') || link.starts_with("data:") {
                continue;
            }
            assert!(
                !link.starts_with('/') && !link.contains("://"),
                "{page} carries the absolute link {link}"
            );
            let target = normalize(&base.join(&link));
            if !target.starts_with(root) {
                // The only link out of the section is the board that
                // sits beside it, which this build does not own.
                assert_eq!(
                    link, "../index.html",
                    "{page} links out of the section, to {link}"
                );
                continue;
            }
            assert!(
                target.is_file(),
                "{page} links to {link}, which is not a file in the site"
            );
            checked += 1;
        }
    }
    checked
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

    // The second pass splices the block into the page the first pass
    // wrote, so it must land exactly where a one-pass render put it:
    // inside the page frame, above the masthead, once.
    assert_eq!(page.matches("class=\"site-nav no-print\"").count(), 1);
    let frame = page.find("<div class=\"page\">").unwrap();
    let nav = page.find("<nav class=\"site-nav no-print\"").unwrap();
    let masthead = page.find("<header class=\"masthead\">").unwrap();
    assert!(frame < nav && nav < masthead);
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
// Partial builds
// ----------------------------------------------------------------------

#[test]
fn a_failed_page_is_dropped_from_the_hub_and_reported() {
    // Occupying a target path with a directory makes the atomic write
    // of that one page fail, which stands in for any per-page failure.
    let site = build_prepared(2, |root, _| {
        std::fs::create_dir_all(root.join("t/te1-20250505.html")).unwrap();
    });

    assert_eq!(site.report.failures.len(), 1, "{:?}", site.report.failures);
    assert_eq!(site.report.missing, vec!["t/te1-20250505.html".to_string()]);

    // The hub of the failed date drops the station...
    let index = site.read("index.html");
    assert!(!index.contains("href=\"t/te1-20250505.html\""));
    assert!(!index.contains(">Woodlands North<"));
    // ...but still lists every other one.
    let other = site.plan.stations.iter().find(|s| s.key != "te1").unwrap();
    assert!(index.contains(&format!(
        "href=\"{}\"",
        site.plan.timetable_path(other, site.plan.first_date())
    )));

    // The other service date is unaffected and still lists it.
    let tomorrow = site.read("day-20250506.html");
    assert!(tomorrow.contains("href=\"t/te1-20250506.html\""));
    assert!(tomorrow.contains(">Woodlands North<"));

    // And the machine-readable index names the gap.
    let data: serde_json::Value = serde_json::from_str(&site.read("data/index.json")).unwrap();
    assert_eq!(data["missing"][0], "t/te1-20250505.html");
}

#[test]
fn a_failed_diagram_drops_only_its_window() {
    let site = build_prepared(1, |root, _| {
        std::fs::create_dir_all(root.join("d/te-20250505-morning.html")).unwrap();
    });
    assert!(site
        .report
        .missing
        .contains(&"d/te-20250505-morning.html".to_string()));

    let index = site.read("index.html");
    assert!(!index.contains("href=\"d/te-20250505-morning.html\""));
    // The other windows of the line survive...
    assert!(index.contains("href=\"d/te-20250505-midday.html\""));
    // ...and another line is untouched.
    assert!(index.contains("href=\"d/ns-20250505-morning.html\""));
}

#[test]
fn a_line_with_no_surviving_window_loses_its_card() {
    let site = build_prepared(1, |root, _| {
        for window in ["morning", "midday", "evening", "night"] {
            std::fs::create_dir_all(root.join(format!("d/te-20250505-{window}.html"))).unwrap();
        }
    });
    let te = site.plan.lines.iter().find(|l| l.key == "te").unwrap();
    let index = site.read("index.html");
    assert!(!index.contains(&format!(">{}</h3>", te.name)));
    // The others keep their cards.
    let ns = site.plan.lines.iter().find(|l| l.key == "ns").unwrap();
    assert!(index.contains(&format!(">{}</h3>", ns.name)));
}

/// The paths a partial build is made to fail on.
///
/// One of each kind that something links to: a timetable another
/// timetable's date row names, a diagram a timetable's diagram row
/// names, a diagram window a sibling diagram names, and a drawing a
/// diagram's own drawing row names.
const SABOTAGED: [&str; 5] = [
    "t/te1-20250505.html",
    "t/ns1-20250506.html",
    "d/te-20250506-morning.html",
    "d/ns-20250505-midday.html",
    "d/te-20250505-morning.svg",
];

fn partial_site() -> Built {
    build_prepared(2, |root, _| {
        for occupied in SABOTAGED {
            std::fs::create_dir_all(root.join(occupied)).unwrap();
        }
    })
}

#[test]
fn every_link_of_a_partial_site_still_resolves() {
    let site = partial_site();
    assert_eq!(
        site.report.failures.len(),
        SABOTAGED.len(),
        "{:?}",
        site.report.failures
    );
    for path in SABOTAGED {
        assert!(
            site.report.missing.contains(&path.to_string()),
            "{path} is not reported missing"
        );
    }

    // Not a sample of pages: every generated file in the section, so a
    // surviving document cannot quietly link to a failed one.
    let checked = check_every_link(&site.root());
    assert!(checked > 500, "only {checked} links checked");
}

#[test]
fn a_surviving_page_drops_the_navigation_link_to_a_failed_sibling() {
    let site = partial_site();

    // Tomorrow's timetable survived; today's did not. The date row of
    // the survivor would have named it, and now has nothing left to
    // offer, so the whole row goes.
    let timetable = site.read("t/te1-20250506.html");
    assert!(timetable.contains("class=\"site-nav no-print\""));
    assert!(!timetable.contains("href=\"../t/te1-20250505.html\""));
    assert!(!timetable.contains(">Date</span>"));
    // Tomorrow's diagram for the same line failed too, so the diagram
    // row of that page goes as well...
    assert!(!timetable.contains("href=\"../d/te-20250506-morning.html\""));
    assert!(!timetable.contains(">Diagram</span>"));
    // ...while another station's surviving page keeps the diagram row
    // that still has a target.
    let other = site.read("t/ns1-20250505.html");
    assert!(other.contains(">Diagram</span>"));
    assert!(other.contains("href=\"../d/ns-20250505-morning.html\""));

    // The diagram page keeps the windows that exist and drops the one
    // that does not, on the line where a window failed.
    let diagram = site.read("d/ns-20250505-morning.html");
    assert!(!diagram.contains("href=\"../d/ns-20250505-midday.html\""));
    assert!(diagram.contains("href=\"../d/ns-20250505-evening.html\""));

    // The drawing row goes when the drawing itself failed.
    let drawing_page = site.read("d/te-20250505-morning.html");
    assert!(!drawing_page.contains("href=\"../d/te-20250505-morning.svg\""));
    assert!(!drawing_page.contains(">Drawing</span>"));
    // Its sibling window kept its own drawing.
    let sibling = site.read("d/te-20250505-midday.html");
    assert!(sibling.contains("href=\"../d/te-20250505-midday.svg\""));
    assert!(sibling.contains(">Drawing</span>"));
    // And the date row of that page drops tomorrow, which failed.
    assert!(!drawing_page.contains("href=\"../d/te-20250506-morning.html\""));

    // The hub still reaches every page that exists.
    assert!(site
        .read("index.html")
        .contains("href=\"t/ns1-20250505.html\""));
}

#[test]
fn a_build_with_no_content_page_writes_no_hub() {
    // Every planned target is occupied by a directory, so every write
    // fails: the build produced nothing to read.
    let site = build_prepared(1, |root, plan| {
        for date in &plan.dates {
            for station in &plan.stations {
                std::fs::create_dir_all(root.join(plan.timetable_path(station, date))).unwrap();
            }
            for line in &plan.lines {
                for window in &plan.windows {
                    std::fs::create_dir_all(root.join(plan.diagram_path(line, date, window)))
                        .unwrap();
                    std::fs::create_dir_all(root.join(plan.drawing_path(line, date, window)))
                        .unwrap();
                }
            }
        }
    });

    assert_eq!(site.report.content_files, 0);
    assert_eq!(site.report.files, 0);
    // No hub, no index, no HTML at all: an entry point over nothing
    // would read as a working site with no trains in it.
    assert!(!site.root().join("index.html").exists());
    assert!(!site.root().join("data/index.json").exists());
    let written: Vec<PathBuf> = files_under(&site.root());
    assert!(written.is_empty(), "{written:?}");

    // Every planned page is accounted for as missing.
    let planned = site.plan.stations.len() + site.plan.lines.len() * site.plan.windows.len() * 2;
    assert_eq!(site.report.missing.len(), planned);
    assert_eq!(site.report.failures.len(), planned);

    // And no opt-in publishes it.
    assert_eq!(Verdict::of(&site.report, true), Verdict::Empty);
    assert!(!Verdict::of(&site.report, true).is_publishable());
    assert!(!Verdict::of(&site.report, false).is_publishable());
}

#[test]
fn the_report_counts_content_apart_from_infrastructure() {
    let site = build(2);
    let content = site.plan.stations.len() * 2 + site.plan.lines.len() * 4 * 2 * 2;
    assert_eq!(site.report.content_files, content);
    // The rest is a hub per date plus the machine-readable index.
    assert_eq!(site.report.files - site.report.content_files, 3);
    assert_eq!(Verdict::of(&site.report, false), Verdict::Complete);

    // A partial build still has content, so the opt-in decides.
    let partial = partial_site();
    assert!(partial.report.content_files > 0);
    assert_eq!(Verdict::of(&partial.report, false), Verdict::Incomplete);
    assert_eq!(Verdict::of(&partial.report, true), Verdict::AcceptedPartial);
}

#[test]
fn the_reported_bytes_match_what_is_on_disk() {
    // The navigation pass rewrites each page, so the byte count must
    // follow the file rather than the first draft of it.
    let site = build(1);
    let on_disk: u64 = files_under(&site.root())
        .iter()
        .map(|path| std::fs::metadata(path).unwrap().len())
        .sum();
    assert_eq!(site.report.bytes, on_disk);
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
    // A clean build promises that nothing is missing.
    assert_eq!(index["missing"].as_array().unwrap().len(), 0);
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
