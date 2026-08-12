//! End-to-end tests for the command line.
//!
//! The tests call `run` in process, so they need no built binary and
//! no network. They check the exit-code contract, the artifacts, the
//! manifest, and the promise that no secret ever reaches an output.

use std::path::{Path, PathBuf};

use mrt_schedule_cli::{run, ExitCode};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../mrt-gtfs/tests/fixtures/mini")
}

fn config() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config/singapore.yaml")
}

fn feed_arg() -> String {
    fixture().display().to_string()
}

/// Run the command line and return the exit code.
fn cli(arguments: &[&str]) -> i32 {
    run(arguments.iter().map(|s| s.to_string()))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

// ----------------------------------------------------------------------
// Artifacts
// ----------------------------------------------------------------------

#[test]
fn a_timetable_run_writes_html_and_a_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let html = dir.path().join("nested/te1.html");
    let manifest = dir.path().join("manifest.json");

    let code = cli(&[
        "timetable",
        "--feed",
        &feed_arg(),
        "--station",
        "TE1",
        "--date",
        "2025-05-05",
        "--config",
        config().to_str().unwrap(),
        "--out",
        html.to_str().unwrap(),
        "--manifest",
        manifest.to_str().unwrap(),
        "--quiet",
    ]);
    assert_eq!(code, ExitCode::Success.code());

    let page = read(&html);
    assert!(page.starts_with("<!doctype html>"));
    assert!(page.contains("Woodlands North departure timetable"));
    assert!(page.contains("Springleaf"));

    let manifest: serde_json::Value = serde_json::from_str(&read(&manifest)).unwrap();
    assert_eq!(manifest["command"], "timetable");
    assert_eq!(manifest["service_date"], "20250505");
    assert_eq!(manifest["timezone"], "Asia/Singapore");
    assert_eq!(manifest["schema_version"], "1.0");
    assert_eq!(manifest["feed_from_cache"], false);
    assert_eq!(manifest["feed_sha256"].as_str().unwrap().len(), 64);
    let artifact = &manifest["artifacts"][0];
    assert_eq!(artifact["kind"], "timetable");
    assert_eq!(artifact["format"], "html");
    assert_eq!(artifact["bytes"].as_u64().unwrap(), page.len() as u64);
    assert_eq!(
        artifact["sha256"],
        mrt_datamall::sha256_hex(page.as_bytes())
    );
}

#[test]
fn a_diagram_run_writes_svg_html_and_json() {
    let dir = tempfile::tempdir().unwrap();
    for (format, name) in [("svg", "d.svg"), ("html", "d.html"), ("json", "d.json")] {
        let out = dir.path().join(name);
        let code = cli(&[
            "diagram",
            "--feed",
            &feed_arg(),
            "--line",
            "TE",
            "--date",
            "2025-05-05",
            "--from",
            "05:00",
            "--until",
            "10:00",
            "--format",
            format,
            "--out",
            out.to_str().unwrap(),
            "--quiet",
        ]);
        assert_eq!(code, ExitCode::Success.code(), "format {format} failed");
        assert!(out.exists());
    }

    let svg = read(&dir.path().join("d.svg"));
    assert!(svg.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    assert!(svg.contains("<svg xmlns=\"http://www.w3.org/2000/svg\""));

    let json: serde_json::Value = serde_json::from_str(&read(&dir.path().join("d.json"))).unwrap();
    assert_eq!(json["metadata"]["schema_version"], "1.0");
    assert!(json["runs"].as_array().unwrap().len() > 3);
    assert_eq!(json["time_axis"]["start"], "05:00:00");
    assert_eq!(json["time_axis"]["end"], "10:00:00");
}

#[test]
fn the_output_is_written_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("page.html");
    // Put a longer file in place first: a partial write would leave
    // the tail of the old content behind.
    std::fs::write(&out, "x".repeat(200_000)).unwrap();

    assert_eq!(
        cli(&[
            "timetable",
            "--feed",
            &feed_arg(),
            "--station",
            "TE1",
            "--date",
            "2025-05-05",
            "--out",
            out.to_str().unwrap(),
            "--quiet",
        ]),
        0
    );
    let page = read(&out);
    assert!(page.starts_with("<!doctype html>"));
    assert!(page.trim_end().ends_with("</html>"));
    assert!(!page.contains("xxxx"));

    // No temporary file survives.
    let leftovers: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|name| name != "page.html")
        .collect();
    assert!(leftovers.is_empty(), "left behind {leftovers:?}");
}

#[test]
fn the_same_inputs_produce_the_same_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let mut pages = Vec::new();
    for name in ["a.html", "b.html"] {
        let out = dir.path().join(name);
        cli(&[
            "timetable",
            "--feed",
            &feed_arg(),
            "--station",
            "TE1",
            "--date",
            "2025-05-05",
            "--config",
            config().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--quiet",
        ]);
        pages.push(read(&out));
    }
    assert_eq!(pages[0], pages[1]);
}

// ----------------------------------------------------------------------
// Exit codes
// ----------------------------------------------------------------------

#[test]
fn an_unknown_command_or_option_exits_with_two() {
    assert_eq!(cli(&["nonsense"]), ExitCode::Usage.code());
    assert_eq!(
        cli(&["timetable", "--feed", &feed_arg(), "--nope"]),
        ExitCode::Usage.code()
    );
    // A window that ends before it starts is a configuration error.
    assert_eq!(
        cli(&[
            "diagram",
            "--feed",
            &feed_arg(),
            "--line",
            "TE",
            "--date",
            "2025-05-05",
            "--from",
            "10:00",
            "--until",
            "09:00",
            "--quiet",
        ]),
        ExitCode::Usage.code()
    );
}

#[test]
fn a_missing_feed_exits_with_three() {
    assert_eq!(
        cli(&[
            "timetable",
            "--feed",
            "/nowhere/feed.zip",
            "--station",
            "NS1",
            "--date",
            "2025-05-05",
            "--quiet",
        ]),
        ExitCode::SourceFailure.code()
    );
}

#[test]
fn a_broken_feed_exits_with_four() {
    let dir = tempfile::tempdir().unwrap();
    let feed = dir.path().join("feed");
    std::fs::create_dir_all(&feed).unwrap();
    std::fs::write(feed.join("stops.txt"), "stop_id,stop_name\nS1,Alpha\n").unwrap();
    // routes.txt, trips.txt, and stop_times.txt are missing.
    assert_eq!(
        cli(&[
            "timetable",
            "--feed",
            feed.to_str().unwrap(),
            "--station",
            "S1",
            "--date",
            "2025-05-05",
            "--quiet",
        ]),
        ExitCode::InvalidFeed.code()
    );
}

#[test]
fn an_unresolved_selector_exits_with_five() {
    for arguments in [
        vec![
            "timetable",
            "--station",
            "ZZ99",
            "--date",
            "2025-05-05",
            "--quiet",
        ],
        vec![
            "diagram",
            "--line",
            "NOT-A-LINE",
            "--date",
            "2025-05-05",
            "--quiet",
        ],
        vec![
            "diagram",
            "--corridor",
            "not-configured",
            "--date",
            "2025-05-05",
            "--quiet",
        ],
        vec![
            "diagram",
            "--pattern",
            "9999",
            "--date",
            "2025-05-05",
            "--quiet",
        ],
    ] {
        let mut full = vec!["--feed", &feed_arg()]
            .into_iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        full.splice(0..0, arguments.iter().map(|s| s.to_string()));
        let borrowed: Vec<&str> = full.iter().map(String::as_str).collect();
        assert_eq!(
            cli(&borrowed),
            ExitCode::Unresolved.code(),
            "{arguments:?} did not report an unresolved selector"
        );
    }
}

#[test]
fn a_policy_refusal_exits_with_six() {
    assert_eq!(
        cli(&[
            "diagram",
            "--feed",
            &feed_arg(),
            "--line",
            "BP",
            "--date",
            "2025-05-05",
            "--frequency-policy",
            "reject-non-exact",
            "--quiet",
        ]),
        ExitCode::Unrepresentable.code()
    );
    // The same run under the default policy succeeds and draws a band.
    assert_eq!(
        cli(&[
            "diagram",
            "--feed",
            &feed_arg(),
            "--line",
            "BP",
            "--date",
            "2025-05-05",
            "--out",
            "-",
            "--format",
            "json",
            "--quiet",
        ]),
        ExitCode::Success.code()
    );
}

#[test]
fn an_unwritable_output_exits_with_seven() {
    assert_eq!(
        cli(&[
            "timetable",
            "--feed",
            &feed_arg(),
            "--station",
            "TE1",
            "--date",
            "2025-05-05",
            "--out",
            "/dev/null/impossible.html",
            "--quiet",
        ]),
        ExitCode::OutputFailure.code()
    );
}

#[test]
fn warnings_as_errors_turns_a_warning_into_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("d.html");
    let arguments = [
        "diagram",
        "--feed",
        &feed_arg(),
        "--line",
        "TE",
        "--date",
        "2025-05-05",
        "--out",
        out.to_str().unwrap(),
        "--quiet",
    ];
    // The TEL branch cannot share the main axis, which is a warning.
    assert_eq!(cli(&arguments), ExitCode::Success.code());

    let mut strict: Vec<&str> = arguments.to_vec();
    strict.push("--warnings-as-errors");
    assert_eq!(cli(&strict), ExitCode::InvalidFeed.code());
}

#[test]
fn help_and_version_succeed() {
    assert_eq!(cli(&["--help"]), 0);
    assert_eq!(cli(&["--version"]), 0);
    assert_eq!(cli(&[]), 0);
}

// ----------------------------------------------------------------------
// Configuration
// ----------------------------------------------------------------------

#[test]
fn the_shipped_configuration_loads_and_drives_the_output() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("ja.html");
    assert_eq!(
        cli(&[
            "timetable",
            "--feed",
            &feed_arg(),
            "--station",
            "TE1",
            "--date",
            "2025-05-05",
            "--config",
            config().to_str().unwrap(),
            "--language",
            "ja",
            "--out",
            out.to_str().unwrap(),
            "--quiet",
        ]),
        0
    );
    let page = read(&out);
    assert!(page.contains("<html lang=\"ja\">"));
    assert!(page.contains("発車時刻表"));
}

#[test]
fn a_broken_configuration_is_a_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.yaml");
    std::fs::write(&bad, "version: 1\ntimetable:\n  columns: 0\n").unwrap();
    assert_eq!(
        cli(&[
            "timetable",
            "--feed",
            &feed_arg(),
            "--station",
            "TE1",
            "--date",
            "2025-05-05",
            "--config",
            bad.to_str().unwrap(),
            "--quiet",
        ]),
        ExitCode::Usage.code()
    );

    let unknown = dir.path().join("unknown.yaml");
    std::fs::write(&unknown, "version: 1\nnot_a_field: 3\n").unwrap();
    assert_eq!(
        cli(&[
            "timetable",
            "--feed",
            &feed_arg(),
            "--station",
            "TE1",
            "--date",
            "2025-05-05",
            "--config",
            unknown.to_str().unwrap(),
            "--quiet",
        ]),
        ExitCode::Usage.code()
    );
}

#[test]
fn a_configured_corridor_reaches_the_diagram() {
    let dir = tempfile::tempdir().unwrap();
    let configuration = dir.path().join("corridor.yaml");
    std::fs::write(
        &configuration,
        "version: 1\n\
         corridors:\n\
         \x20 - id: tel-main\n\
         \x20   line: TE\n\
         \x20   axis:\n\
         \x20     - TE1\n\
         \x20     - TE2\n\
         \x20     - TE3\n\
         \x20     - TE4\n\
         \x20   branches:\n\
         \x20     - junction: TE2\n\
         \x20       axis:\n\
         \x20         - TB1\n\
         \x20         - TB2\n",
    )
    .unwrap();

    let out = dir.path().join("corridor.json");
    assert_eq!(
        cli(&[
            "diagram",
            "--feed",
            &feed_arg(),
            "--corridor",
            "tel-main",
            "--date",
            "2025-05-05",
            "--config",
            configuration.to_str().unwrap(),
            "--format",
            "json",
            "--out",
            out.to_str().unwrap(),
            "--quiet",
        ]),
        0
    );
    let json: serde_json::Value = serde_json::from_str(&read(&out)).unwrap();
    assert_eq!(json["corridor"]["id"], "tel-main");
    assert_eq!(json["corridor"]["nodes"].as_array().unwrap().len(), 6);
    // The branch run is drawn rather than dropped.
    let drawn: Vec<&str> = json["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["source_trip_id"].as_str().unwrap())
        .collect();
    assert!(drawn.contains(&"TE_B1"), "{drawn:?}");
}

// ----------------------------------------------------------------------
// Secrets
// ----------------------------------------------------------------------

#[test]
fn no_artifact_carries_the_account_key() {
    let dir = tempfile::tempdir().unwrap();
    let html = dir.path().join("page.html");
    let manifest = dir.path().join("manifest.json");
    let secret = "SECRET-ACCOUNT-KEY-9f3a";

    // The key is in the environment, and the run must ignore it: the
    // feed is local.
    std::env::set_var("LTA_DATAMALL_ACCOUNT_KEY", secret);
    let code = cli(&[
        "timetable",
        "--feed",
        &feed_arg(),
        "--station",
        "TE1",
        "--date",
        "2025-05-05",
        "--out",
        html.to_str().unwrap(),
        "--manifest",
        manifest.to_str().unwrap(),
        "--quiet",
    ]);
    std::env::remove_var("LTA_DATAMALL_ACCOUNT_KEY");
    assert_eq!(code, 0);

    for path in [&html, &manifest] {
        let text = read(path);
        assert!(
            !text.contains(secret),
            "{} carries the account key",
            path.display()
        );
        assert!(!text.contains("AccountKey"));
        assert!(!text.contains("X-Amz"));
    }
}

// ----------------------------------------------------------------------
// Other commands
// ----------------------------------------------------------------------

#[test]
fn validate_reports_the_feed_and_succeeds_on_a_sound_one() {
    assert_eq!(
        cli(&["validate", "--feed", &feed_arg(), "--quiet"]),
        ExitCode::Success.code()
    );
}

#[test]
fn validate_fails_on_a_feed_with_errors() {
    let dir = tempfile::tempdir().unwrap();
    let feed = dir.path().join("feed");
    std::fs::create_dir_all(&feed).unwrap();
    std::fs::write(
        feed.join("stops.txt"),
        "stop_id,stop_name\nS1,Alpha\nS2,Beta\n",
    )
    .unwrap();
    std::fs::write(
        feed.join("routes.txt"),
        "route_id,route_short_name,route_type\nR1,NS,1\n",
    )
    .unwrap();
    // The trip names a route that does not exist.
    std::fs::write(
        feed.join("trips.txt"),
        "route_id,service_id,trip_id\nGONE,WK,T1\n",
    )
    .unwrap();
    std::fs::write(
        feed.join("stop_times.txt"),
        "trip_id,arrival_time,departure_time,stop_id,stop_sequence\n\
         T1,06:00:00,06:00:00,S1,1\nT1,06:10:00,06:10:00,S2,2\n",
    )
    .unwrap();
    std::fs::write(
        feed.join("calendar.txt"),
        "service_id,monday,tuesday,wednesday,thursday,friday,saturday,sunday,start_date,end_date\n\
         WK,1,1,1,1,1,0,0,20250101,20271231\n",
    )
    .unwrap();

    assert_eq!(
        cli(&["validate", "--feed", feed.to_str().unwrap(), "--quiet"]),
        ExitCode::InvalidFeed.code()
    );
}

#[test]
fn stations_lists_the_lines_and_the_stations() {
    assert_eq!(
        cli(&["stations", "--feed", &feed_arg(), "--quiet"]),
        ExitCode::Success.code()
    );
}
