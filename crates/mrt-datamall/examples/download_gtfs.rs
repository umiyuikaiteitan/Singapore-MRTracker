//! Download the official GTFS Schedule feed for trains and show the
//! live rail status.
//!
//! Set the account key first:
//!   export LTA_DATAMALL_ACCOUNT_KEY=<your key>
//!
//! Usage:
//!   cargo run -p mrt-datamall --example download_gtfs -- <output.zip>

use mrt_datamall::DataMallClient;

fn main() {
    let output = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "gtfs_schedule.zip".to_string());

    let client = DataMallClient::from_env().expect("set LTA_DATAMALL_ACCOUNT_KEY first");

    let alerts = client.train_service_alerts().expect("cannot read alerts");
    println!("Rail status: {:?}", alerts.status);
    for message in &alerts.messages {
        println!("  {}", message.content);
    }

    let link = client.gtfs_schedule_link().expect("cannot get the link");
    println!(
        "GTFS Schedule dataset from {}",
        link.timestamp.as_deref().unwrap_or("unknown time")
    );
    let bytes = client.download(&link.url).expect("cannot download");
    std::fs::write(&output, &bytes).expect("cannot write the file");
    println!("Wrote {} bytes to {output}", bytes.len());
}
