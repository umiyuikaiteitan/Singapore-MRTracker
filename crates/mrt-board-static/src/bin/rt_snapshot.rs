//! Write the live snapshot (alerts, crowd, trip updates) to a file.
//!
//! A frequent, lightweight workflow runs this tool and publishes the
//! output to a data branch, so the board page can fetch delays and
//! cancellations without a full site deployment.
//!
//! Usage:
//!
//! ```sh
//! export LTA_DATAMALL_ACCOUNT_KEY=<your key>
//! cargo run -p mrt-board-static --bin mrt-rt-snapshot -- live.json
//! ```

use mrt_datamall::DataMallClient;

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: mrt-rt-snapshot <output-file>");
        std::process::exit(2);
    });
    let client = DataMallClient::from_env().expect("set LTA_DATAMALL_ACCOUNT_KEY first");
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_secs() as i64;

    let snapshot = mrt_board_static::live_snapshot(&client, now_unix);
    std::fs::write(&out, snapshot.to_string()).expect("cannot write the snapshot");
    let trips = snapshot["trips"].as_object().map(|m| m.len()).unwrap_or(0);
    eprintln!("Wrote {out}: {trips} trips with real-time data.");
}
