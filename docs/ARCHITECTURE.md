# Architecture

This document describes the design of the Singapore-MRTracker
library.

## Layers

The dependencies point in one direction only. Two stacks sit on the
same three source crates: a **live** stack that answers "what is next,
right now", and a **publication** stack that produces printed
timetables and diagrams.

```text
        ┌──────────────────────┐  ┌──────────────────────┐
        │  mrt-schedule-site   │  │   mrt-schedule-cli   │
        │ hub, nav, whole site │  │ files, cache, manifest│
        └──────────┬───────────┘  └──────────┬───────────┘
                   └─────────────┬───────────┘
┌─────────────────┐                 │
│    mrt-live     │      ┌──────────▼───────────┐
│ NetworkStatus,  │      │ mrt-publication-html │
│ LiveBoard       │      │   HTML, CSS, SVG     │
└───┬─────┬────┬──┘      └──────────┬───────────┘
    │     │    │                    │
    │     │    │         ┌──────────▼───────────┐
    │     │    │         │   mrt-publication    │
    │     │    │         │ timetable & diagram  │
    │     │    │         │     view models      │
    │     │    │         └──────────┬───────────┘
    │     │    │                    │
┌───▼─────┴────┴───┐  ┌─────────────▼──┐  ┌───────────────┐
│    mrt-gtfs      │  │  mrt-gtfs-rt   │  │  mrt-datamall │
│  static feeds    │  │  realtime pb   │  │  API client   │
└──────────────────┘  └────────────────┘  └───────────────┘
```

- `mrt-gtfs`, `mrt-gtfs-rt`, and `mrt-datamall` do not know each
  other.
- `mrt-live` knows all three. It merges their outputs.
- `mrt-publication` knows only `mrt-gtfs`. It does no input or
  output.
- `mrt-publication-html` knows only `mrt-publication`. It never
  queries the network model.
- `mrt-schedule-cli` and `mrt-schedule-site` are the only crates in
  the publication stack that touch files, the clock, or the network.
  The site generator reuses the command line's atomic writer and YAML
  reader rather than growing its own.
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
4. **Network** (`network.rs`, `schedule.rs`, `query.rs`).
   `RailNetwork` links the records:
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
   - `RailNetwork::query_trip_instances` is the renderer-facing API.
     It returns complete `TripInstance` values with every
     `ScheduledCall` — platform, headsign, boarding rules, and where
     each time came from — plus the `FrequencyBand` entries that the
     policy left unexpanded, plus the diagnostics that explain
     everything it could not represent. Renderers use this API and
     never touch the private `TripSchedule`.

Two more modules support it:

- `validate.rs` checks a parsed feed and returns diagnostics. Lenient
  mode reports what breaks the output; strict mode also reports every
  deviation from the letter of the specification.
- `diag.rs` holds `Diagnostic` and `Severity`. A query or a projection
  reports what it could not do instead of silently dropping data.

### Safety of the zip source

A downloaded archive is untrusted input. `ZipSource` refuses an
archive before it reads any content when the archive holds an absolute
entry path, a `..` traversal, a symbolic link, a second copy of a feed
file, more entries than `ZipLimits::max_entries`, or an expansion
beyond `ZipLimits::max_total_bytes`. Strict mode additionally requires
the feed files at the archive root, as standard GTFS specifies.

Supporting types: `GtfsTime` (seconds on a service day) and
`ServiceDate` (a civil date with weekday math). Both types are
implemented in the crate with the well-known civil calendar
algorithms, so the crate needs no date-time dependency.

The `alias` module makes station codes usable in URLs.
`alias::normalize` reduces any spelling to a comparison key (`NS1`,
`ns-1`, and `NS 1` all become `ns1`), and
`RailNetwork::station_by_alias` resolves that key against every code
of every station, so each code of an interchange opens the same
board. Station names stay out of the alias table on purpose: the
feed carries names that two stations share, for example `Bukit
Panjang` on the Downtown Line and on the Bukit Panjang LRT, and a
name in a link would name an arbitrary one. `station_by_name`
remains for callers that accept the ambiguity, such as the command
line examples.

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

## mrt-publication

A pure projection crate: no input, no output, no HTML. It takes a
`RailNetwork`, a `PublicationConfig`, and a `DocumentSeed`, and
returns serializable documents.

- `timetable.rs` builds a `TimetableDocument`: one panel per line,
  platform, and direction; hour rows in *service-day* order, so a day
  that starts at `04:00` ends with `00`, `01`, `02`, `03`; and column
  breaks weighted by the printed height of each row.
- `corridor.rs` builds the vertical axis of a diagram. A
  `CorridorNode` carries a station *and* an occurrence index, because
  an unrolled loop contains the same station more than once and a
  `StationId` alone cannot name a position. A pattern joins the spine
  only when it is a subsequence of it, forwards or backwards;
  anything else gets its own panel and a diagnostic.
- `diagram.rs` turns runs into polylines: a horizontal dwell segment
  where a train stands, a sloped travel segment between stations, and
  a clean cut at each edge of the requested window. Opposite
  directions therefore slope opposite ways with no special case.
- `config.rs` and `text.rs` hold the presentation choices and the
  interface labels. Feed text is never translated; interface text
  never leaks into feed text.

The crate's rule is that it never invents schedule data. Destination
text comes from `stop_headsign`, `trip_headsign`, the real terminus,
or an explicit override, in that order. A platform comes from the
platform the run actually uses. Non-exact headway service becomes a
band, not a list of minutes.

Documents read no clock, so the same feed, date, configuration, and
generator version produce byte-identical output. Generation time lives
in the manifest instead.

## mrt-publication-html

Renders the documents as one self-contained file each.

- `escape.rs` holds the only functions that put feed text into markup.
  Colours and font names pass a strict filter before they reach the
  stylesheet, so a hostile `route_color` cannot inject a declaration.
- The pages carry `default-src 'none'`, so they make no network
  request of any kind.
- Everything reads without JavaScript: the timetable is a table, and
  the diagram is an SVG plus a call table for every run. The scripts
  add zoom, filters, and highlighting, and reveal their own controls,
  which start hidden.
- Print profiles cover A4 portrait and landscape for a timetable, A3
  landscape for a diagram, plus a monochrome profile.
- No font file and no logo is embedded. The theme names a font stack
  that ends in a generic family.

## mrt-schedule-cli

The orchestration layer, and the only crate in the publication stack
that touches the outside world.

- A content-addressed cache stores each archive under its own
  SHA-256, with `current.json` pointing at the newest.
- A small YAML reader turns the configuration into a
  `serde_json::Value`, which serde deserializes into
  `PublicationConfig`. One schema definition, no second parser.
- Artifacts are written through a temporary file and an atomic
  rename.
- The manifest records the feed hash, the feed timestamp, the
  configuration hash, the generator version, the artifacts, and the
  diagnostics. It is the only output that reads a clock.
- The exit codes are a contract: 2 usage, 3 source, 4 feed, 5
  unresolved, 6 not representable under the policy, 7 output.

## mrt-schedule-site

Publishes the generated documents as a browsable section of a static
site, beside the live board and under the same domain.

- `plan.rs` decides what the site contains — which service dates,
  stations, lines, and diagram windows — before anything is rendered,
  so the hub, the navigation blocks, and the file names agree on one
  set of names. Every path it produces is relative, because a GitHub
  Pages project site lives under `/<repository>/`.
- `hub.rs` renders the hub: line cards, date tabs, and the station
  list. The list is markup, not a script that builds one, so the page
  works without JavaScript; the search box stays hidden until a
  script can drive it.
- `build.rs` renders every page through `mrt-publication-html` with a
  `PageNav` block, and writes each file atomically.

The pages are pre-generated rather than rendered in the browser. That
keeps one renderer, one set of tests, and one escaping discipline; a
browser-side renderer would mirror the markup and drift from it. It
also means a visitor loads one file and can then print it, save it,
or read it with no signal.

## mrt-live

The crate merges the three data sources into view models:

- `NetworkStatus::from_alerts` maps the legacy alerts into one
  status entry per known line. The list is stable, so status boards
  can render a fixed layout.
- `LiveBoardBuilder` decorates the static departure board with live
  layers: delays and cancellations from trip updates, crowd levels
  from `PCDRealTime`, notices from the legacy alerts, and the
  GTFS-Realtime service alerts. Every layer is optional.
- A service alert reaches a departure when it names the trip, the
  route of the line, or a platform of the station, and one of its
  active periods covers the build time. A no-service alert cancels
  the departure; reduced service, significant delays, or a detour
  set the row's `alerted` flag, because an alert carries no delay
  figure. The alert text joins the notices.
- A modified schedule is a notice only. The LTA feed uses that
  effect for planned adjustments that run for months, for example
  the Sengkang West LRT loop closure, and the published timetable
  already carries them. Marking their departures would leave whole
  lines permanently flagged and bury the live disruptions.
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
  rail filter must remove), interchanges, calendar exceptions, a trip
  past midnight, a line with two platforms per station, a short turn,
  an exact headway block, a trip with missing intermediate times, a
  pass-through call, a branch that no single station axis can hold
  with the main line, and a loop whose first station repeats.
- Security tests build hostile zip archives and check that the loader
  refuses them, and render a feed whose text fields try to break out
  of the markup.
- Snapshot tests pin the timetable and diagram view models and the
  normalized SVG. Run with `UPDATE_SNAPSHOTS=1` to accept a change.
- The site tests build the whole section from the fixture and check
  that every link resolves to a file that exists, that no path is
  absolute, that the hub lists every station in the document, and
  that no page can reach the network.
- Visual tests check the visual grammar of both reference pages and
  refresh the committed examples in `examples/`.
  `scripts/visual-regression.sh` adds a pixel comparison; it needs a
  browser, so it stays out of `cargo test`.
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
7. For the publication layer, port the projections before the
   renderers. The view models are plain data, and the JSON snapshots
   in `crates/mrt-publication-html/tests/snapshots` are the
   acceptance test for a port.

Copy the test fixtures. They are language-independent.
