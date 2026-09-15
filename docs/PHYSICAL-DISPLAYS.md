# Physical Singapore MRT displays

Two independent devices share a versioned data contract:

1. **LED network PCB:** one addressable RGB pixel for every operating
   physical MRT station, with interchange codes grouped on one pixel.
2. **E-ink departure board:** its own ESP32 and 7.5-inch monochrome panel,
   showing departures for the configured station.

The network inventory contains 146 stations at its 15 September 2026
review, including CCL6. It excludes LRT and unopened stations. See the
[source audit and exclusions](../hardware/NETWORK-COVERAGE.md) before
updating coverage. The current board is a labeled station matrix rather
than a geographic rail diagram. Its address order is an explicit physical
contract, independent of GTFS file ordering.

## What the data can support

This implementation shows **upcoming station departures, not train
locations**. The repository's DataMall client fetches train trip updates;
it does not establish a vehicle-position endpoint. The adapter does not
turn timetables into moving train dots. Within the next 30 minutes it
selects the next boardable departure for each mapped station, colors it
by line, and varies brightness with its wait. Interchanges aggregate their
explicit aliases. Check the [projection reference](../crates/mrt-display/README.md)
for matching rules and brightness levels.

| State | LED behavior | E-ink behavior |
| --- | --- | --- |
| `realtime` | At least one displayed result has matched fresh RT evidence; other pixels may be scheduled | Each row says predicted or scheduled |
| `schedule` | Upcoming scheduled activity | Scheduled/approximate rows clearly labeled |
| `stale` | All pixels off | Explicit stale notice and scheduled fallback |
| `unavailable` | All pixels off | No departures; explanatory notice |
| Expired / connection lost | Device clears pixels without waiting for another response | Clears old rows and shows offline at its next permitted refresh |

A fresh RT feed by itself never upgrades an unmatched row. GTFS approximate
frequency service remains approximate. Canceled trips, skipped/no-pickup
stops, and terminal-only arrivals do not create boardable departures.
Unsupported update forms retain scheduled provenance. The bridge accepts
only complete GTFS-RT snapshots; differential feeds require a stateful merge
before this adapter.

E-ink retains an image with the power removed. Printed snapshot and expiry
timestamps are therefore part of its display, and its minutes are a snapshot
rather than a continuously running countdown.

## Run the bridge

Use Rust and the existing GTFS download command. Keep the DataMall account
key on this host; it is never serialized into device frames.

```sh
export LTA_DATAMALL_ACCOUNT_KEY='<your key>'
cargo run -p mrt-datamall --example download_gtfs -- data/gtfs_schedule.zip
cargo run --release -p mrt-display-service -- \
  --feed data/gtfs_schedule.zip \
  --layout hardware/layouts/singapore-mrt.json \
  --addr 0.0.0.0:8787
```

Use `--offline` to deliberately disable live fetching. Without a key the
same bridge works from the local schedule. `--feed` accepts a ZIP or an
extracted GTFS directory. An invalid station mapping fails startup rather
than quietly dropping a station. Missing legacy aliases produce warnings
when another explicit alias resolves.

The server defaults to `127.0.0.1:8787`. Binding `0.0.0.0` allows the ESP32s
to reach it on the LAN. This is a trusted-LAN prototype: it serves HTTP
without authentication. Do not expose the port directly to the Internet.
Use your LAN IP in each device's `FRAME_URL`.

| Endpoint | Purpose |
| --- | --- |
| `GET /` | Preview the LED board and the separate e-ink output |
| `GET /v1/layout` | Exact station-to-pixel mapping and coordinates |
| `GET /v1/frame` | Complete version-one frame; no delta state required |
| `GET /healthz` | Process status, last fetch attempt/success, source timestamp |

DataMall polling runs in a background thread, at most one trip-update
refresh per 30 seconds plus request duration. A stalled upstream request
does not block the HTTP thread. Source freshness is checked on every
projection, not merely when a fetch succeeds. Device frames normally live
for 45 seconds, reduced by the remaining lifetime of applied RT evidence.
Devices poll every 15 seconds and validate both wall-clock expiry and a
monotonic lease. A cached/replayed frame cannot extend its original lease.

The bridge loads the static GTFS once at startup. Refresh the file and
restart after timetable changes. Calendar coverage outside the loaded
feed produces an unavailable frame. Inside its coverage, an empty window
can simply mean no service; it is not automatically a data outage.
`/healthz` reports process health, not a guarantee of current train data.
Service alerts/crowd endpoints are not polled by this bridge; alerts embedded
in the trip-update feed can be applied by the projector.

## Offline fixture and replay

The repository miniature feed only has four TEL stations; it intentionally
cannot satisfy the full-network physical layout. Use its matching four-pixel
layout for software verification:

```sh
cargo run -p mrt-display-service -- \
  --feed crates/mrt-gtfs/tests/fixtures/mini \
  --layout crates/mrt-display/tests/fixtures/tel-four.json \
  --offline --snapshot --at 1786312740
```

This deterministic snapshot is 10 August 2026 at 05:59 SGT. Add
`--rt captured.pb` for a complete GTFS-RT snapshot with matching timestamps.
`--at` and `--rt` require `--snapshot`: a running server cannot be frozen at
a past time and accidentally give old arrivals a new lifetime.

Omit `--snapshot --at ...` to start the local preview at the actual time.
The fixture is small and may have no departures in the current window.

## Configure independent devices

See [firmware build and wiring](../firmware/README.md). `pio run -e led`
builds the network controller; `pio run -e epaper` builds the separate
departure board. Both validate `singapore-mrt-v1` and 146 pixels by default.
The e-ink device does not drive the LED power circuit.

To change the e-ink station, copy the layout, change `board_station`, and
start a second bridge on another port with that file. Leave its physical
mapping and `layout_id` intact if the LEDs have not changed. Point the e-ink
device at this bridge. Do not change firmware indices to match a feed's
array order. When physical mapping changes, assign a new `layout_id` and
update the device configuration together.

## Fabricate and validate

The [hardware build guide](../hardware/README.md) identifies the exact
Gerber/drill set, substrate dimensions, bank power inputs, LED breakout
pinout, BOM, and separate enclosure files. All outputs are reproducible
from versioned sources. The LED carrier needs external level shifting,
power distribution, and one RGB module per station. It is not a bare-chip
pick-and-place design.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 hardware/scripts/generate.py
python3 hardware/scripts/validate.py
pio run -d firmware -e led -e epaper
```

Do the documented unpowered continuity, low-current one-bank, pixel-order,
offline-expiry, and panel-fit checks before assembling the complete network.
Software current limiting supplements the bank fuses and conductor ratings.
No assembled hardware or live account-backed installation is claimed by
the source files alone.
