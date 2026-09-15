use std::path::PathBuf;
use std::process::{Command, Output};

fn snapshot(at: &str, full_network: bool) -> Output {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    // Cargo's check-only targets do not build binaries. Test runs must have
    // the explicit binary path; never accidentally execute a PATH lookup.
    let binary = match option_env!("CARGO_BIN_EXE_mrt-display-service") {
        Some(path) => path,
        None => panic!("run these integration tests through cargo test"),
    };
    Command::new(binary)
        .current_dir(root)
        .args([
            "--feed",
            "crates/mrt-gtfs/tests/fixtures/mini",
            "--layout",
            if full_network {
                "hardware/layouts/singapore-mrt.json"
            } else {
                "crates/mrt-display/tests/fixtures/tel-four.json"
            },
            "--offline",
            "--snapshot",
            "--at",
            at,
        ])
        .output()
        .expect("run display service")
}

#[test]
fn offline_cli_emits_the_device_contract_and_real_fixture_departures() {
    let result = snapshot("1786312740", false); // 2026-08-10 05:59 SGT
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let frame: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(frame["schema_version"], 1);
    assert_eq!(frame["layout_id"], "fixture-mini-v1");
    assert_eq!(frame["source_state"], "schedule");
    assert_eq!(frame["pixels"].as_array().unwrap().len(), 4);
    assert_eq!(frame["valid_until"], 1786312785u64);
    assert_eq!(frame["board"]["station_name"], "Woodlands North");
    assert!(!frame["board"]["rows"].as_array().unwrap().is_empty());
    assert!(frame["board"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["realtime"] == false));
}

#[test]
fn expired_schedule_produces_an_explicit_dark_frame() {
    let result = snapshot("1893456000", false); // 2030-01-01, outside fixture calendar
    assert!(result.status.success());
    let frame: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(frame["source_state"], "unavailable");
    assert!(frame["board"]["rows"].as_array().unwrap().is_empty());
    assert!(frame["pixels"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["r"] == 0 && p["g"] == 0 && p["b"] == 0));
}

#[test]
fn full_physical_layout_cannot_silently_drop_stations_missing_from_fixture() {
    let result = snapshot("1786312740", true);
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("invalid display layout"));
}
