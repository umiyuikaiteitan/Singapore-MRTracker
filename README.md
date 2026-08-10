# Singapore-MRTracker

A Rust library and framework that ingests GTFS data for the Singapore
rail network (MRT and LRT). Use it to build interactive live train
maps, destination boards, and LED panels.

> **About the language in this document.** This document applies the
> ASD-STE100 (Simplified Technical English) writing rules where
> possible: short sentences, active voice, one instruction per
> sentence, and one name for one thing.

## What the project does

- It downloads the official GTFS datasets for trains from LTA
  DataMall.
- It parses GTFS Schedule feeds and builds a linked rail network
  model: lines, stations, interchanges, patterns, and schedules.
- It decodes the GTFS-Realtime feeds for trains: trip updates and
  service alerts.
- It reads the live rail status APIs: train service alerts and
  platform crowd density.
- It merges the static and the live data into render-ready view
  models: a network status and a live destination board.

## Repository layout

The project is a Cargo workspace with four crates. Each crate is
usable on its own.

| Crate | Purpose |
|-------|---------|
| `crates/mrt-gtfs` | Parse GTFS Schedule feeds. Select the rail subset. Build the network model. Answer schedule queries. |
| `crates/mrt-gtfs-rt` | Decode GTFS-Realtime messages: trip updates, service alerts, vehicle positions. |
| `crates/mrt-datamall` | Talk to the LTA DataMall API: dataset links, downloads, alerts, crowd density. |
| `crates/mrt-live` | Merge the static network with the live data into view models for maps, boards, and panels. |

Other important paths:

| Path | Content |
|------|---------|
| `docs/ARCHITECTURE.md` | The design of the library and the porting notes. |
| `docs/DATA-SOURCES.md` | The DataMall endpoints and their response formats. |
| `scripts/regenerate-gtfs-rt.sh` | The generator for the vendored Protocol Buffer code. |
| `crates/*/examples/` | Small example programs. |

## Data sources

The library reads these LTA DataMall resources:

| Dataset | Endpoint | Format |
|---------|----------|--------|
| GTFS Schedule for trains | `GTFSScheduleTrain` | Link to a GTFS zip archive |
| GTFS-Realtime trip updates | `GTFSRealtimeTrainTripUpdates` | Link to a Protocol Buffer file |
| GTFS-Realtime service alerts | `GTFSRealTimeTrainServiceAlerts` | Link to a Protocol Buffer file |
| Train service alerts (legacy) | `TrainServiceAlerts` | JSON |
| Platform crowd density, live | `PCDRealTime` | JSON |
| Platform crowd density, forecast | `PCDForecast` | JSON |
| Passenger volume by station | `PV/Train` | Link to a CSV zip archive |

See `docs/DATA-SOURCES.md` for the response formats and examples.

## Quick start

### Requirements

- Rust 1.75 or newer.
- An LTA DataMall account key, for the live data only. The static
  parts of the library work without a key.

### Build and test

1. Clone the repository.
2. Run `cargo build --workspace`.
3. Run `cargo test --workspace`.

The tests do not use the network. The tests use a miniature GTFS feed
and recorded API responses.

### Get and set the account key

LTA issues the account key when you register at
<https://datamall.lta.gov.sg>. The key is a secret.

1. Request the key from LTA DataMall.
2. Set the key as an environment variable:

   ```sh
   export LTA_DATAMALL_ACCOUNT_KEY=<your key>
   ```

3. Do not write the key into source code.
4. Do not commit the key to the repository. The `.gitignore` file
   excludes `.env` and `*.key` files as a safety net.

The `AccountKey` type hides the key from debug output, so the key
does not leak into logs.

### Run the examples

Show the live rail status and download the official GTFS Schedule
feed:

```sh
cargo run -p mrt-datamall --example download_gtfs -- data/gtfs_schedule.zip
```

Inspect a feed and list the interchanges:

```sh
cargo run -p mrt-gtfs --example inspect_feed -- data/gtfs_schedule.zip
```

Show a destination board from the static schedule:

```sh
cargo run -p mrt-gtfs --example departure_board -- data/gtfs_schedule.zip NS1 20260810 08:00:00
```

Show a live destination board with alerts, crowd data, and trip
updates:

```sh
cargo run -p mrt-live --example live_board -- NS1 20260810 08:00:00
```

## Library overview

The typical data flow:

```text
DataMall ──> mrt-datamall ──> GTFS zip ──> mrt-gtfs ──> RailNetwork ─┐
                        └──> GTFS-RT pb ──> mrt-gtfs-rt ─────────────┤
                        └──> alerts + crowd JSON ────────────────────┤
                                                                     v
                                                  mrt-live ──> view models
```

A minimal program:

```rust,no_run
use mrt_datamall::DataMallClient;
use mrt_gtfs::{GtfsFeed, RailNetwork, ZipSource};
use mrt_live::LiveBoardBuilder;

fn main() {
    // Step 1: download the official GTFS Schedule feed for trains.
    let client = DataMallClient::from_env().unwrap();
    let bytes = client.fetch_gtfs_schedule().unwrap();

    // Step 2: build the rail network model.
    let mut source = ZipSource::from_reader(std::io::Cursor::new(bytes)).unwrap();
    let feed = GtfsFeed::load(&mut source).unwrap();
    let network = RailNetwork::from_feed(&feed).unwrap();

    // Step 3: build a live destination board.
    let alerts = client.train_service_alerts().unwrap();
    let station = network.station_by_code("NS1").unwrap();
    let board = LiveBoardBuilder::new(&network)
        .with_alerts(&alerts)
        .build(station, "20260810".parse().unwrap(), "08:00:00".parse().unwrap(), 1800);

    for row in &board.rows {
        println!("{} {} in {} s", row.line_code, row.destination, row.departs_in_secs);
    }
}
```

All view models serialize to JSON with `serde`. A web map or an LED
panel driver can consume the JSON directly.

## Design rules

These rules keep the library modular and easy to port to other
languages:

- **No hidden input/output.** The parsers read from the `FeedSource`
  trait. The API client sends requests through the `Transport` trait.
  Tests supply memory sources and mock transports.
- **Plain data models.** The network model uses index identifiers and
  flat structures. It does not use language-specific tricks.
- **Small dependency set.** The core logic uses `csv`, `serde`,
  `prost`, and the standard library. Date and time calculations are
  implemented in the library with well-known civil calendar
  algorithms.
- **Synchronous code.** Async runtimes differ between languages.
  Callers can wrap the client in the concurrency model of their
  choice.

## Development

- Format the code: `cargo fmt --all`.
- Lint the code: `cargo clippy --workspace --all-targets`.
- Run the tests: `cargo test --workspace`.

The GitHub Actions workflow in `.github/workflows/ci.yml` runs the
same three commands for every push and pull request. Keep the tests
green. Add a test for every bug fix, so the bug cannot return.

The file `crates/mrt-gtfs-rt/src/transit_realtime.rs` is generated
from `gtfs-realtime.proto`. Do not edit it by hand. Run
`scripts/regenerate-gtfs-rt.sh` to update it.

## Roadmap

The library is the base for these planned applications:

- An interactive live train map.
- Station destination boards.
- Physical LED panel drivers.
- Ports of the core model to other languages.

## Attribution

- Transit data: © Land Transport Authority of Singapore. The data
  comes from LTA DataMall and is subject to the [Singapore Open Data
  Licence](https://datamall.lta.gov.sg/content/datamall/en/SingaporeOpenDataLicence.html).
- `crates/mrt-gtfs-rt/proto/gtfs-realtime.proto`: © The GTFS
  Specifications Authors, Apache License 2.0.
- This project is not affiliated with the Land Transport Authority.
