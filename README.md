# Singapore-MRTracker

A Rust library and framework that ingests GTFS data for the Singapore
rail network (MRT and LRT). Use it to build interactive live train
maps, destination boards, and LED panels.

(meta note, from yui: this is one of my first large-scale exercises in vibecoding. will keep a cautious eye on it, but it all seems to work for now :3)

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
- It publishes the schedule as printed products: a Japanese-style
  station departure timetable (発車時刻表) and a planning-style
  time-distance train diagram (列車ダイヤグラム), as self-contained
  HTML, standalone SVG, and versioned JSON.

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
| `crates/mrt-map-web` | Serve the live schematic train map in the browser (proof of concept). |
| `crates/mrt-map-static` | Generate the map as a static site for GitHub Pages, separate from the board. |
| `crates/mrt-publication` | Project the schedule into timetable and train-diagram view models. Pure: no input, no output. |
| `crates/mrt-publication-html` | Render those view models as self-contained HTML and standalone SVG. |
| `crates/mrt-schedule-cli` | The generator: fetch, cache, build, and write timetables and diagrams. |
| `crates/mrt-schedule-site` | Generate a browsable static site of timetables and diagrams for GitHub Pages. |

Other important paths:

| Path | Content |
|------|---------|
| `docs/ARCHITECTURE.md` | The design of the library and the porting notes. |
| `docs/DATA-SOURCES.md` | The DataMall endpoints and their response formats. |
| `docs/DEPLOY-PAGES.md` | How to host the board on GitHub Pages. |
| `docs/SINGAPORE-GTFS-PROFILE.md` | What the LTA train feed carries, and which assumptions are still unverified. |
| `docs/CLI.md` | The `mrt-schedule-cli` reference. |
| `docs/CONFIGURATION.md` | Every configuration option. |
| `docs/KNOWN-LIMITATIONS.md` | What the generator does not do, and why. |
| `docs/LIVE-MAP-POC.md` | The plan for the interactive live train map. |
| `config/singapore.yaml` | A complete, commented configuration. |
| `examples/` | Generated example pages, refreshed by the tests. |
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

A link opens a station directly through the `station` parameter. It
takes any code of the station, in any spelling, so every code of an
interchange opens the same board.

```text
?station=NS1   ?station=ns1   ?station=ns-1   ?station=EW24
```

The board reads Singapore time (UTC+8) wherever it is opened, and the
status line reports when the page last reached the live status feed.

To host the board without a server, generate it as a static site.
See `docs/DEPLOY-PAGES.md` for the GitHub Pages workflow:

```sh
cargo run --release -p mrt-board-static -- site data/gtfs_schedule.zip
```

### Run the map UI

The live map is its own site, separate from the board, so it deploys
on its own subdomain:

```sh
cargo run -p mrt-map-web
```

Then open <http://127.0.0.1:8601>. The server takes the same feed
argument as the board (`cargo run -p mrt-map-web --
data/gtfs_schedule.zip`), listens where `MRT_MAP_ADDR` says, and draws
the OpenFantasyMap layout named by `MRT_MAP_LAYOUT` (the default is
the miniature fixture layout, `config/layout-mini.geojson` — a layout
of the real network is future work). Without an account key every
train is schedule-only and the page says so.

To host the map without a server, generate it as a static site:

```sh
cargo run --release -p mrt-map-static -- map-site data/gtfs_schedule.zip
```

### Generate a timetable and a train diagram

A station departure timetable, in the grammar of a Japanese
発車時刻表 — dark hour cells, large minute numerals, small
destination annotations, one panel per platform and direction:

```sh
cargo run -p mrt-schedule-cli -- timetable \
  --feed data/gtfs_schedule.zip \
  --station NS1 \
  --date 2026-08-10 \
  --config config/singapore.yaml \
  --out dist/ns1.html
```

A planning-style train diagram — time across, stations down, one
polyline per train:

```sh
cargo run -p mrt-schedule-cli -- diagram \
  --feed data/gtfs_schedule.zip \
  --line EWL \
  --date 2026-08-10 \
  --from 05:00:00 --until 10:00:00 \
  --config config/singapore.yaml \
  --out dist/ewl.html
```

Both pages are one self-contained file: no external stylesheet,
script, font, or image, a `default-src 'none'` policy, readable
without JavaScript, and printable on A4 and A3. `--format svg` writes
the drawing on its own, and `--format json` writes the versioned view
model for another renderer.

`examples/` holds a generated timetable and diagram, built from the
miniature test feed. See `docs/CLI.md` for the full reference.

### Publish them as a browsable site

`mrt-schedule-site` turns the same generator into a section of the
GitHub Pages site, beside the live board:

```sh
cargo run --release -p mrt-schedule-site -- site/timetables data/gtfs_schedule.zip
```

It writes a hub that lists every line and every station, one
timetable per station and service date, and one diagram per line,
date, and time window. The hub's station list is in the document, so
it works without JavaScript; the search box is an enhancement that
matches a name or any code in any spelling. See `docs/DEPLOY-PAGES.md`
for the workflow and the options.

The generator will not invent data. A headway-based service with
`exact_times=0` becomes `06:30-09:00  every 4 min approximately`
rather than a list of minutes that the feed does not contain; a
platform appears only when the feed names one; and a GTFS `trip_id`
never appears as a train number.

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

RailNetwork ──> mrt-publication ──> timetable and diagram documents
                                                                     v
                                     mrt-publication-html ──> HTML, SVG
                                                                     v
                                     mrt-schedule-cli ──> files, manifest
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
- **No invented data.** A renderer prints what the feed carries, or an
  explicit configuration override, or nothing. Everything it cannot
  represent becomes a diagnostic rather than a plausible guess.
- **Deterministic artifacts.** The same feed, service date,
  configuration, and generator version produce byte-identical files.
  Generation time lives in the manifest, not in the documents.

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

The publication snapshots pin the view models and the drawing. Accept
an intended change with:

```sh
UPDATE_SNAPSHOTS=1 cargo test -p mrt-publication-html
```

`scripts/visual-regression.sh` renders the example pages with a
headless browser and compares them with the baselines in
`examples/baseline/`. It needs Chromium, so it stays out of
`cargo test`.

## Roadmap

The library is the base for these planned applications:

- An interactive live train map. See
  [`docs/LIVE-MAP-POC.md`](docs/LIVE-MAP-POC.md) for the
  plan: a schematic whole-network map, why the positions are derived
  rather than measured, and how the map says so.
- Station destination boards (draft in `crates/mrt-board-web`).
- Physical LED panel drivers.
- Ports of the core model to other languages.
- A live overlay on the train diagram: scheduled against actual,
  from the GTFS-Realtime trip updates.

## Attribution

- Transit data: © Land Transport Authority of Singapore. The data
  comes from LTA DataMall and is subject to the [Singapore Open Data
  Licence](https://datamall.lta.gov.sg/content/datamall/en/SingaporeOpenDataLicence.html).
- `crates/mrt-gtfs-rt/proto/gtfs-realtime.proto`: © The GTFS
  Specifications Authors, Apache License 2.0.
- `crates/mrt-board-web/assets/lta-identity.ttf`: the LTA Identity
  typeface, taken from the MRT-RATIS project for private use.
- This project is not affiliated with the Land Transport Authority.
