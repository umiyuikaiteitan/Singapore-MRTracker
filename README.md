# Singapore-MRTracker

A Rust library and framework that ingests GTFS data for the Singapore
rail network (MRT and LRT). Use it to build interactive live train
maps, destination boards, and LED panels.

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
  models, and serves a RATIS-style destination board in the browser.

## Repository layout

The project is a Cargo workspace. Each library crate is usable on its
own.

| Crate | Purpose |
|-------|---------|
| `crates/mrt-gtfs` | Parse GTFS Schedule feeds. Select the rail subset. Build the network model. Answer schedule queries. |
| `crates/mrt-gtfs-rt` | Decode GTFS-Realtime messages: trip updates, service alerts, vehicle positions. |
| `crates/mrt-datamall` | Talk to the LTA DataMall API: dataset links, downloads, alerts, crowd density. |
| `crates/mrt-live` | Merge the static network with the live data into view models for maps, boards, and panels. |
| `crates/mrt-board-web` | Serve a dot-matrix destination board in the browser (draft). |
| `crates/mrt-board-static` | Generate the board as a static site for GitHub Pages. |

Other important paths:

| Path | Content |
|------|---------|
| `docs/ARCHITECTURE.md` | The design of the library and the porting notes. |
| `docs/DATA-SOURCES.md` | The DataMall endpoints and their response formats. |
| `docs/DEPLOY-PAGES.md` | How to host the board on GitHub Pages. |
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
| Station crowd density, live | `PCDRealTime` | JSON |
| Station crowd density, forecast | `PCDForecast` | JSON |
| Passenger volume by station | `PV/Train` | Link to a CSV zip archive |

See `docs/DATA-SOURCES.md` for the response formats and examples.

## Quick start

### Requirements

- Rust 1.75 or newer.
- An LTA DataMall account key, for the live data. The static parts of
  the library work without a key.

### Build and test

1. Clone the repository.
2. Run `cargo build --workspace`.
3. Run `cargo test --workspace`.

### Get and set the account key

LTA issues the account key when you register at
<https://datamall.lta.gov.sg>. The key is a secret: keep it out of
source code and commits, and supply it at run time. The `.gitignore`
file excludes `.env` and `*.key` files as a safety net, and the
`AccountKey` type redacts the key in debug output and logs.

Set the key as an environment variable:

```sh
export LTA_DATAMALL_ACCOUNT_KEY=<your key>
```

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

Show a destination board in the terminal:

```sh
cargo run -p mrt-gtfs --example departure_board -- data/gtfs_schedule.zip NS1 20260810 08:00:00
cargo run -p mrt-live --example live_board -- NS1 20260810 08:00:00
```

### Run the board UI

```sh
cargo run -p mrt-board-web
```

Then open <http://127.0.0.1:8600>. The server downloads the official
GTFS Schedule feed at startup, or reads a local copy when you pass a
path:

```sh
cargo run -p mrt-board-web -- data/gtfs_schedule.zip
```

To host the board without a server, generate it as a static site.
See `docs/DEPLOY-PAGES.md` for the GitHub Pages workflow:

```sh
cargo run --release -p mrt-board-static -- site data/gtfs_schedule.zip
```

## Library overview

The typical data flow:

```text
DataMall ──> mrt-datamall ──> GTFS zip ──> mrt-gtfs ──> RailNetwork ─┐
                        └──> GTFS-RT pb ──> mrt-gtfs-rt ─────────────┤
                        └──> alerts + crowd JSON ────────────────────┤
                                                                     v
                                                  mrt-live ──> view models
                                                                     v
                                                  mrt-board-web ──> browser
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

All view models serialize to JSON with `serde`, so a web map or an
LED panel driver can consume them directly.

## Design rules

These rules keep the library modular and easy to port to other
languages:

- **Explicit input/output.** The parsers read from the `FeedSource`
  trait. The API client sends requests through the `Transport` trait.
  Tests supply memory sources and mock transports.
- **Plain data models.** The network model uses index identifiers and
  flat structures.
- **Small dependency set.** The core logic uses `csv`, `serde`,
  `prost`, and the standard library. Date and time calculations use
  well-known civil calendar algorithms, implemented in the library.
- **Synchronous code.** Callers wrap the client in the concurrency
  model of their choice.

## Development

- Format the code: `cargo fmt --all`.
- Lint the code: `cargo clippy --workspace --all-targets`.
- Run the tests: `cargo test --workspace`.

The GitHub Actions workflow in `.github/workflows/ci.yml` runs the
same three commands for every push and pull request. Keep the tests
green. Add a test for every bug fix, so the bug cannot return.

The tests marked `#[ignore]` talk to the live DataMall API. Run them
with your account key when you change the client or the models:

```sh
LTA_DATAMALL_ACCOUNT_KEY=<your key> cargo test --workspace -- --ignored
```

The file `crates/mrt-gtfs-rt/src/transit_realtime.rs` is generated
from `gtfs-realtime.proto`. Run `scripts/regenerate-gtfs-rt.sh` to
update it.

## Roadmap

The library is the base for these planned applications:

- An interactive live train map.
- Station destination boards (draft in `crates/mrt-board-web`).
- Physical LED panel drivers.
- Ports of the core model to other languages.

## Attribution

- Transit data: © Land Transport Authority of Singapore. The data
  comes from LTA DataMall and is subject to the [Singapore Open Data
  Licence](https://datamall.lta.gov.sg/content/datamall/en/SingaporeOpenDataLicence.html).
- `crates/mrt-gtfs-rt/proto/gtfs-realtime.proto`: © The GTFS
  Specifications Authors, Apache License 2.0.
- `crates/mrt-board-web/assets/lta-identity.ttf`: the LTA Identity
  typeface, taken from the MRT-RATIS project for private use.
- This project is not affiliated with the Land Transport Authority.
