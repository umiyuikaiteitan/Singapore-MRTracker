# Architecture

This document describes the design of the Singapore-MRTracker
library.

## Layers

The workspace has four crates. The dependencies point in one
direction only:

```text
┌─────────────────────────────────────────────────────┐
│                      mrt-live                       │
│   view models: NetworkStatus, LiveBoard             │
└───────┬──────────────────┬──────────────────┬───────┘
        │                  │                  │
┌───────▼───────┐  ┌───────▼───────┐  ┌───────▼───────┐
│   mrt-gtfs    │  │  mrt-gtfs-rt  │  │  mrt-datamall │
│ static feeds  │  │  realtime pb  │  │  API client   │
└───────────────┘  └───────────────┘  └───────────────┘
```

- `mrt-gtfs`, `mrt-gtfs-rt`, and `mrt-datamall` do not know each
  other.
- `mrt-live` knows all three. It merges their outputs.
- Applications can use any subset of the crates.

## mrt-gtfs

The crate has four internal layers. Data flows down:

1. **Source** (`source.rs`). The `FeedSource` trait supplies the
   bytes of one feed file. `DirectorySource` reads a directory.
   `ZipSource` reads a zip archive, also when the feed files are in a
   subdirectory. A byte-order mark at the start of a file is removed.
2. **Feed** (`feed.rs`, `model.rs`). `GtfsFeed::load` parses the CSV
   tables into raw record structures. The records keep the data as
   the feed supplies it. Optional files give empty tables.
3. **Filter** (`filter.rs`). `RailFilter` selects routes by GTFS
   `route_type` and cuts every dependent table down to the rail
   subset. The default filter accepts the standard and the extended
   rail route types.
4. **Network** (`network.rs`, `schedule.rs`). `RailNetwork` links the
   records:
   - Stops collapse into stations. A GTFS parent station groups its
     platforms. A stop without a parent becomes its own station.
   - Routes become lines. Trips with the same line, direction, and
     station sequence share one stop pattern.
   - `calendar.txt` and `calendar_dates.txt` become a service
     calendar with weekly rules and date exceptions.
   - Schedule queries expand frequency-based trips and answer
     departure and destination-board requests. A board query also
     examines the previous service day, because trips can run past
     midnight (GTFS times can be greater than `24:00:00`).

Supporting types: `GtfsTime` (seconds on a service day) and
`ServiceDate` (a civil date with weekday math). Both types are
implemented in the crate with the well-known civil calendar
algorithms, so the crate needs no date-time dependency.

### Identifier model

`StationId`, `LineId`, and `PatternId` are plain indexes into the
vectors of `RailNetwork`. This model:

- makes lookups constant-time,
- keeps the memory layout compact,
- ports directly to other languages (an index is an index
  everywhere).

String identifiers from the feed stay available on the model types
(`gtfs_id`, `route_id`), and lookup maps translate between the two
worlds.

## mrt-gtfs-rt

The crate decodes GTFS-Realtime Protocol Buffer messages.

- `transit_realtime.rs` is generated from `gtfs-realtime.proto` with
  `prost-build` and committed to the repository. Users do not need
  `protoc`. Run `scripts/regenerate-gtfs-rt.sh` to update the file.
- `RailRtFeed` flattens the deep Protocol Buffer structure into
  simple structures with plain fields. Renderers use the flat form;
  special cases can fall back to the full model.

## mrt-datamall

The crate talks to the LTA DataMall OData API.

- The `Transport` trait isolates the HTTP stack. The default
  implementation uses `ureq` and sits behind the `http-ureq`
  feature. Tests use a mock transport with recorded responses.
- `AccountKey` wraps the secret API key. Its debug output is
  redacted. The key travels only in the `AccountKey` request header.
- Dataset endpoints return pre-signed download links that expire
  after a short time. The client therefore offers combined
  `fetch_*` methods that get the link and download the file in one
  step. Downloads do not carry the account key, because the link
  carries its own signature.
- `get_raw` gives access to endpoints that the crate does not model
  yet.

## mrt-live

The crate merges the three data sources into view models:

- `NetworkStatus::from_alerts` maps the legacy alerts into one
  status entry per known line. The list is stable, so status boards
  can render a fixed layout.
- `LiveBoardBuilder` decorates the static departure board with live
  layers: delays and cancellations from trip updates, crowd levels
  from `PCDRealTime`, and notices from the alerts. Every layer is
  optional.
- `match_train_line` maps GTFS route names to the DataMall line
  codes with simple heuristics.

The crate does no input/output. The application fetches the data and
passes it in. This keeps render loops testable and fast.

## Error handling

Each crate has one error enumeration (`GtfsError`, `RtError`,
`DataMallError`). Errors carry context: the file name, the URL, or
the HTTP status. The library does not panic on bad input data.

## Testing strategy

- Unit tests sit next to the code and cover the parsers and the
  calculations.
- Integration tests load a miniature Singapore-flavored GTFS feed
  from `crates/mrt-gtfs/tests/fixtures/mini`. The feed contains two
  MRT lines, one frequency-based LRT line, a bus route (which the
  rail filter must remove), interchanges, calendar exceptions, and a
  trip past midnight.
- The DataMall client tests replay the official LTA sample
  responses through a mock transport.
- The GTFS-Realtime tests encode synthetic Protocol Buffer messages
  and decode them again.
- The tests marked `#[ignore]` verify the client against the live
  DataMall API. Run them with an account key:
  `cargo test --workspace -- --ignored`.

## Porting notes

A port to another language keeps this shape:

1. Port `GtfsTime`, `ServiceDate`, and the civil calendar functions
   first. They are small and have exhaustive tests.
2. Port the raw record structures and the CSV mapping.
3. Port the network build steps in order (calendar, stations, lines,
   patterns and trips, transfers). Keep the index identifier model.
4. Port the schedule queries.
5. For GTFS-Realtime, use the standard `gtfs-realtime.proto` with
   the Protocol Buffer toolchain of the target language, and mirror
   the flat `RailRtFeed` view.
6. For the API client, mirror the `Transport` seam so the tests stay
   network-free.

Copy the test fixtures. They are language-independent.
