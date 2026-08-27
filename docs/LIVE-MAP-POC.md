# The live map proof of concept

This document plans the first of the applications that `README.md`
lists under **Roadmap**: an interactive live train map. It describes
what the map can honestly show given the data LTA publishes, where the
code goes, what it borrows from the OpenFantasyMap project, and how it
should look.

It is a plan, and the phases record their own progress. Phase 1 is
implemented: the map view model lives in `crates/mrt-live/src/map.rs`
with its tests and committed snapshot beside it. Phase 0 remains open —
it needs a real DataMall account key. Phases 2 to 4 are not started.

## What the proof of concept is

One screen: the whole rail network as a schematic, with trains moving
along it, refreshed while the page is open.

- One page, one view. No station picker, no route planner, no
  timetable. The board and the timetables already exist for that.
- The whole network at once, at a fixed schematic scale.
- Trains drawn on the edge between two stations, in the colour of
  their line, with a marker for how well their position is known.
- Line status, station names, and a freshness indicator.

## What it is not

**Not a dispatching display.** GTFS carries passenger-facing routes,
trips, and stop times, and nothing about block occupancy, signalling,
or which physical unit is where. The same boundary that
`docs/KNOWN-LIMITATIONS.md` draws around the train diagram applies
here, and more sharply, because a moving dot invites a reader to
believe it is a measurement.

**Not geographic tracking.** The map is schematic. See "Why a
schematic" below: it is the honest rendering, not a stylistic
preference.

**Not a phone application.** One self-contained page, readable in a
desktop or mobile browser, deployable both from `mrt-board-web` and as
a static page on GitHub Pages.

**Not a replacement for the board.** The board answers "what is next
here". The map answers "what is the network doing". They share view
model crates and the same visual language.

## The data reality

### There are no vehicle positions

LTA DataMall publishes three train feeds relevant here:

| Dataset | Endpoint | What it carries |
|---------|----------|-----------------|
| GTFS Schedule | `GTFSScheduleTrain` | Routes, trips, stop times, calendar |
| GTFS-Realtime trip updates | `GTFSRealtimeTrainTripUpdates` | Per-trip delay, per-stop predictions, cancellations |
| GTFS-Realtime service alerts | `GTFSRealTimeTrainServiceAlerts` | Effects, active periods, informed entities |

Plus the legacy `TrainServiceAlerts` JSON and `PCDRealTime` platform
crowd density, which updates about every ten minutes.

There is no `VehiclePositions` endpoint. `mrt-gtfs-rt` decodes the
`VehiclePosition` message because it decodes the whole standard
`gtfs-realtime.proto`, and nothing in this repository consumes it.
There is no evidence that the LTA feeds carry one, and this document
does not assume one appears.

So a train's position on the map is **derived**, always. It comes from:

1. the scheduled trajectory of a run — `RailNetwork::query_trip_instances`
   returns a `TripInstance` with one `ScheduledCall` per station,
   carrying `arrival`, `departure`, and a `TimeQuality`
   (`crates/mrt-gtfs/src/query.rs:114`, `:128`, `:183`);
2. a shift applied from the GTFS-Realtime trip update for the same
   `trip_id` — the per-stop `StopTimeUpdate` events where they exist,
   otherwise the trip-level `delay_secs`
   (`crates/mrt-gtfs-rt/src/lib.rs:114`, `:161`);
3. linear interpolation between the adjusted departure at the station
   behind and the adjusted arrival at the station ahead.

That third step invents the shape of the motion. A real train
accelerates, coasts, and brakes; the interpolation says it travels at a
constant fraction per second. Between two stations two minutes apart
the error can reach twenty seconds of travel, which on a schematic
edge is a visible part of the edge.

### What "live" honestly means here

"Live" on this map means: the schedule, corrected by the most recent
prediction the operator published, drawn at the time your browser last
fetched it. It does not mean "where the train is".

The design rule **no invented data** (`README.md:286`) is the
interesting constraint of this whole exercise, because a derived
position is by construction invented. The resolution is not to refuse
to draw it, but to make the derivation visible. The same way
`TimeQuality` marks a computed stop time and the timetable prints
`every 4 min approximately` rather than fabricated minutes, every
position on the map carries its provenance and renders accordingly:

| Situation | What the map shows |
|-----------|--------------------|
| A run standing at a station, adjusted by a stop-level prediction | A pill on the station disc. This is the strongest claim the data supports. |
| A run between two stations, both calls from the feed, RT-adjusted | A pill on the edge at the interpolated fraction, drawn with the estimate treatment. |
| A run whose stop times were interpolated by `mrt-gtfs` (`TimeQuality::Interpolated`) | The estimate treatment, and the run's tooltip says the schedule itself was computed. |
| A run in a non-exact headway band (`exact_times=0`) | Not drawn as a train. The line carries a band label instead: "every 4 min approximately". Individual positions do not exist. |
| A run with `canceled` set on its trip update | Not drawn as a train. The edge or the line is marked. |
| No trip update matched the run | Drawn from the schedule alone, marked as schedule-only. |
| The realtime snapshot is older than the staleness threshold | The whole map switches to the schedule-only treatment and the freshness lamp says so, as the board's status line already does. |

There is no "confidence 87%" number anywhere. Provenance is a small
enumeration, like `TimeQuality`, not a score.

### Geometry: station points yes, track lines no

`Station` carries `lat` and `lon` as `Option<f64>`
(`crates/mrt-gtfs/src/network.rs:59`). `shapes.txt` is parsed into
`GtfsFeed.shapes` and then never consumed by `RailNetwork`, and
whether the real feed ships the file at all is recorded as
**unverified** (`docs/SINGAPORE-GTFS-PROFILE.md:73`).

So the network's topology comes from `StopPattern` — line, direction,
and an ordered station list (`crates/mrt-gtfs/src/network.rs:84`) —
which is exactly an edge list. Distances along a pattern come from
`RailNetwork::cumulative_station_distance`, which uses great-circle
distance between station positions (`crates/mrt-gtfs/src/query.rs:734`).

### Why a schematic

Drawing trains on a geographic basemap would require track alignments
this project does not have. A position interpolated along the straight
line between two station coordinates, laid over a map tile, puts a
train in the sea off Marina Bay or through a block of flats: precision
the data cannot support, in the medium that most strongly implies it
is real.

A schematic makes no claim about where anything physically is. It
claims order along a line and an approximate share of the way between
two stations, which is exactly what the data supports. The Mini
Metro-flavoured reduction is not decoration; it is the rendering that
matches the evidence.

### Assumptions to verify before building

All of these are already marked unverified in
`docs/SINGAPORE-GTFS-PROFILE.md`. The map needs the answers more than
the timetable does.

| Question | Why the map needs it | Profile row |
|----------|----------------------|-------------|
| Is `shapes.txt` present, and does it cover every trip? | Decides whether a future geographic mode is even possible. The schematic does not need it. | §2 |
| Does the feed use `location_type=1` parent stations? | Without them an interchange splits into several stations, and the schematic would draw Jurong East two or three times. | §6 |
| Is `frequencies.txt` present, and with which `exact_times`? | Decides how much of the network is undrawable as individual trains. | §9 |
| Does every route carry `route_color`? | The map has hard-coded LTA colours as a fallback; a feed colour would be preferred, and must pass the escape filter. | §5 |
| How many `route_id` values make up one line? | The Circle Line is known to be split. The layout binds by line name, not route id. | §5 |
| Do trip updates cover every running trip, and how often are they published? | Decides how much of the map is schedule-only in practice. | §11 |

## Where the code goes

The dependency direction in `docs/ARCHITECTURE.md` allows exactly one
placement. The map view model consumes `mrt-gtfs` and `mrt-gtfs-rt`,
does no input or output, and sits beside `LiveBoard`. Either:

- add `map.rs` to `mrt-live`, or
- add a sibling crate `mrt-map` that depends on `mrt-gtfs` and
  `mrt-gtfs-rt` only.

Prefer `mrt-live` for the proof of concept: it already merges these
sources, it already has the alert and crowd plumbing, and a second
crate can be split out later without changing the types. Renderers
stay strictly downstream: `mrt-board-web` serves the JSON, a static
generator writes it. Neither the layout file nor the renderer may
reach back into `RailNetwork`.

## Phases

### Phase 0: verify the feed

Run the profile checks with a real account key and record the answers.

```sh
export LTA_DATAMALL_ACCOUNT_KEY=<your key>
cargo run -p mrt-schedule-cli -- fetch --source datamall --out data/train.zip
cargo run -p mrt-schedule-cli -- validate --feed data/train.zip --strict
cargo run -p mrt-schedule-cli -- stations --feed data/train.zip
```

Capture one GTFS-Realtime trip update snapshot every minute for a
morning peak and an off-peak hour, and count: how many running trips
carry an update, whether `stop_updates` are populated or only
`delay_secs`, and how often the feed timestamp advances.

**Acceptance.** Every row of the table above is answered and the
corresponding rows in `docs/SINGAPORE-GTFS-PROFILE.md` are marked
verified with the date they were checked. The realtime coverage
figures are written down in §11 of the same document. If coverage
turns out to be near zero, this proof of concept becomes a schedule
animation and says so on the page; that is a legitimate outcome of
this phase, not a failure of it.

### Phase 1: the map view model

A `NetworkSnapshot` in Rust, built from a `RailNetwork`, an optional
`RailRtFeed`, optional alerts, a service date, and a clock passed in
by the caller. Shape, roughly:

- `lines`: line id, display name, colour, state.
- `edges`: pattern id, from-station, to-station, index in the pattern.
- `trains`: the run's `instance_id` and `source_trip_id`, its line,
  the edge it occupies (or the station it stands at), a `progress`
  fraction in `[0, 1]`, the destination text, a `PositionQuality`,
  and the delay in seconds where one is known.
- `bands`: headway services that were not expanded, per line.
- `freshness`: the realtime feed timestamp, the snapshot clock, and
  the resulting staleness state.
- `diagnostics`: a `Vec<Diagnostic>`, in the existing style — every
  run that could not be placed says why.

Rules:

- The builder reads no clock. The caller passes `date`, `clock`, and
  the realtime `now_unix`, exactly as `LiveBoardBuilder::build` does
  (`crates/mrt-live/src/lib.rs:298`).
- `PositionQuality` is a small enumeration mirroring `TimeQuality`:
  `at-station`, `interpolated-realtime`, `interpolated-schedule`,
  `schedule-only`, and it is what the renderer switches on.
- A run whose bracketing calls have `TimeQuality::Missing` is not
  placed, and produces a diagnostic.

**Acceptance.** Given the miniature fixture in
`crates/mrt-gtfs/tests/fixtures/mini`, a fixed date, a fixed clock,
and a synthetic trip update, the builder produces a JSON snapshot that
is byte-identical across runs and is committed as the acceptance test,
in the manner of the publication snapshots. Tests cover: a train at a
station, a train mid-edge, a cancelled trip, a headway band, a trip
past midnight, a loop pattern that visits a station twice, and a stale
realtime feed. No test touches the network.

### Phase 2: the layout

The schematic layout is data, not code. It is authored in
OpenFantasyMap, exported as GeoJSON, and committed under `config/` or
`assets/`.

Steps:

1. Draw the network in OpenFantasyMap — one line per MRT/LRT line,
   stations placed along it, interlined trunk sections where lines
   share an alignment.
2. Export GeoJSON. OpenFantasyMap round-trips its whole model through
   feature `properties`: `ofm: "line"` carries mode, colour, nodes and
   segments; `ofm: "station"` carries the name and the arc-length
   position `t` along the line
   (OpenFantasyMap `static/app.js:1660`, `:1697`).
3. Bind each layout station to a real station. A small binder — a Rust
   module in the renderer crate, or a build step — joins layout
   stations to `RailNetwork` stations and reports what it could not
   match.

Binding by name is not good enough: `docs/ARCHITECTURE.md` records
that two stations share the name `Bukit Panjang`. The layout must
carry station **codes**, resolved through
`RailNetwork::station_by_alias`, which already accepts any spelling.

This needs a small change in OpenFantasyMap: a station `code` (or
`ref`) property that survives export. The change belongs in that
project, and it is already scheduled there — the mid-term entry of
workstream 2 in OpenFantasyMap's `docs/ROADMAP.md` commits to station
identity metadata "so exported stations can be keyed to real network
station codes", and names this proof of concept as the first external
consumer of the extracted renderer and the acceptance test for it. The
export does not carry the property yet: at the commit inspected
(`5ffd4e9`) an `ofm: "station"` feature holds `id`, `lineId`, `name`,
and `t` only. Until that work ships, a side-car mapping file keyed by
layout station id is an acceptable stop-gap, and should be treated as
one.

**Acceptance.** A committed layout file plus a binder that, run
against a real feed, reports zero unmatched stations in both
directions, or names every one it could not match. An unmatched
station is a diagnostic and a visible gap on the page, never a
silently dropped one. Changing the layout requires no Rust change.

### Phase 3: the renderer

One self-contained page. SVG, inline CSS, inline script, no external
request — the discipline `mrt-publication-html` already follows, and
`default-src 'none'` where the deployment allows it.

- **Without JavaScript** the page shows the static network: every
  line, every station, every name, from the layout. That is the
  progressive-enhancement floor. It is a useful artefact on its own.
- **With JavaScript** the page fetches the snapshot JSON and places
  trains. It polls, like the board: `mrt-board-web` polls
  `/api/board` every 15 seconds behind a 20-second server-side TTL
  (`crates/mrt-board-web/src/main.rs:40`), and the static site reads a
  `live.json` that a scheduled workflow refreshes
  (`.github/workflows/rt-refresh.yml`). The map adds
  `/api/map-snapshot` to the server and a `map.json` to the static
  build. No new transport mechanism.
- Between polls the script may **advance the interpolation locally**,
  because the snapshot carries the fraction and the scheduled edge
  duration. It must not extrapolate past the next scheduled arrival:
  a train that should have arrived and has no new snapshot stops at
  the station and takes the stale treatment. Sliding a dot forever on
  a dead feed is the exact failure this project's rules exist to
  prevent.
- Every value that came from the feed — station names, headsigns,
  colours — goes through the escaping discipline of
  `mrt-publication-html/src/escape.rs`. A `route_color` reaches a
  stylesheet only after the strict filter.

**Acceptance.** The page renders the whole network from the layout
with JavaScript disabled. With a fixture snapshot it places trains at
known positions, and a committed normalized-SVG snapshot pins the
static layer. The page makes no request other than the snapshot
fetch. Feed text containing markup renders as text, proven by a test
in the style of the existing security tests.

### Phase 4: live polish

- **Disruption.** `LineState::Disrupted` names affected station codes
  and a direction (`crates/mrt-live/src/lib.rs:100`). Grey the line
  and mark the affected segment. A disrupted line is not deleted from
  the map; its trains become schedule-only or disappear according to
  what the feed says, and the line carries the alert text.
- **Freshness.** A lamp with the feed timestamp, in the board's
  grammar: green when the snapshot is current, amber when it is
  ageing, red when it is stale, with the age in words next to it.
- **Crowd density, optional.** `PCDRealTime` updates roughly every ten
  minutes. If shown, it belongs on station discs as a low-contrast
  ring, separated from train state and labelled with its own age. It
  is tempting and low-value; leave it out unless it reads cleanly.
- **The bridge to the diagram.** The existing roadmap also lists a
  live overlay on the train diagram (`README.md:337`). The
  `NetworkSnapshot` train records carry `instance_id` and
  `source_trip_id`, which the profile already identifies as the
  attachment points for a live layer
  (`docs/SINGAPORE-GTFS-PROFILE.md:234`). The same records can drive a
  "now" marker on a `DiagramRun`. Phase 4 ends by writing down that
  interface, not by building it.

**Acceptance.** A disrupted line, a stale feed, and an empty realtime
snapshot each render a state a reader can name without a legend, and
each is covered by a fixture. The diagram interface is written down in
`docs/ARCHITECTURE.md`.

## What comes from OpenFantasyMap

OpenFantasyMap is a browser transit-line-design tool: FastAPI, Leaflet,
vanilla JavaScript. It is not a dependency of this project and must
not become one. It contributes a tool and a set of idioms.

| Take | Where | Why |
|------|-------|-----|
| The authoring pipeline | The editor itself | Drawing the schematic by hand in SVG is a week of tedium. Drawing it in an editor that already handles branching, interlining, and stations sliding along a route is an afternoon. |
| The GeoJSON round-trip | `static/app.js:1649` | The export carries the whole model in `properties`, so the layout file is complete and re-editable. |
| Dual-stroke casing | `static/app.js:491` | A dark casing at twice the weight under the coloured stroke. This is what makes crossing lines readable without a basemap. |
| Interline striping | `static/app.js:504` | `dashArray` plus a phase offset per service draws two or three lines sharing one alignment. Singapore has several shared corridors. |
| Hollow station discs | `static/style.css:166` | A dark fill ringed in the line colour. Reads at small sizes and stays legible when a train pill sits on it. |
| The geometry helpers | `static/geometry.js` | `pointAtFraction` is literally "place a train at fraction t along this edge". `projectToPolyline`, `routeLengthMeters`, and `splitAtProjection` are the rest of the arithmetic. Plain functions on a `window.OFMGeometry` namespace (`:428`), no editor state, usable as-is. |

| Leave | Why |
|-------|-----|
| The rendering functions in `app.js` | They close over editor state — selection, tools, undo. Extracting them means rewriting them. |
| Leaflet and the basemap | There is no basemap on a schematic. |
| The dark dashboard chrome | Generic; the map has its own identity, below. |
| The FastAPI server and OSM matching | Authoring-time only. The published page is static. |

## Design language

The map extends the visual identity this repository already has. It
does not introduce a new one, and specifically it must not look like a
generic generated web page.

**Not this.** Glass panels and backdrop blur. Purple or teal
gradients. Glow shadows. Large rounded cards. A default Inter or Geist
stack. Rows of decorative icons. An illustrated empty state. Anything
that would look the same if the subject were a crypto dashboard.

**This instead.**

- **The board's grammar.** The RATIS dot-matrix board uses a 5×7
  bitmap font and three lamp colours with operational meaning: red for
  cancelled or delayed by 60 seconds or more, amber for early, green
  for on time (`crates/mrt-board-web/assets/index.html:174`, `:280`).
  The map reuses those semantics for train state. Colour means
  something; it is never decoration.
- **Official line colours.** Already coded, by station-code prefix:
  NS `#d42e12`, EW and CG `#009645`, NE `#9900aa`, CC and CE
  `#fa9e0d`, DT `#005ec4`, TE `#9d5b25`, LRT `#748477`
  (`crates/mrt-board-web/assets/index.html:321`). The line ribbon is
  the strongest element on the page.
- **Typography.** The board uses the LTA Identity typeface, which this
  repository holds for private use only (`README.md:343`). A published
  page must name a fallback stack that ends in a generic family and
  degrade cleanly, exactly as `docs/KNOWN-LIMITATIONS.md:85` requires
  of the publication pages. Do not embed the file in a public
  artefact.
- **Mini Metro as the reference for reduction.** Flat bold line
  ribbons. 45° and 90° geometry only. Stations as plain discs, with a
  distinct simple shape for interchanges. Trains as small
  line-coloured pills sliding along the ribbon. No basemap, no
  terrain, no labels other than station names and line names. The void
  background is the point: it says the drawing is a diagram.
- **The estimate treatment.** A train whose position is interpolated
  is drawn differently from one standing at a station — a softer
  outline, or a short trailing bar spanning the uncertainty, rather
  than a hard-edged pill at a precise point. The visual difference
  must be legible without reading a legend, and the legend must exist
  anyway.
- **Restraint from the publication stack.** Self-contained page. CSP
  as tight as the deployment allows. A CSS file with a comment
  explaining each choice. Visual regression baselines, in
  `examples/baseline/`, under the existing script.
- **Print.** A schematic prints well and a moving map does not. The
  print profile drops the trains and prints the network, dated.

## Risks and open questions

| Risk | Effect if it goes badly | What to do |
|------|-------------------------|------------|
| Trip update coverage is sparse or absent | Most trains are schedule-only; the map animates a timetable | Phase 0 measures it before anything is built. The page states the coverage figure. |
| Trip updates carry only `delay_secs`, no `stop_updates` | Positions shift uniformly; a train recovering time is drawn wrong | Acceptable, and marked: the quality enumeration distinguishes the two cases. |
| Realtime feed cadence is unknown | The poll interval and the staleness threshold are guesses | Phase 0 measures the timestamp advance. Set the threshold from the measurement, not from a round number. |
| Interpolation error between sparse stations | A train drawn up to a fifth of an edge from its true position | The estimate treatment, and the legend. Do not add easing curves that model acceleration; that would be a second invention on top of the first. |
| `frequencies.txt` with `exact_times=0` covers much of the LRT | Whole lines carry bands and no trains | Correct behaviour, and it must not look like a bug. Band lines get an explicit label. |
| No parent stations in the feed | Interchanges split; the schematic and the network model disagree | Phase 0 answers it. The binder reports the mismatch rather than merging by name. |
| LTA Identity typeface licence | A public page cannot ship the font | Named fallback stack, as the publication pages already do. |
| GitHub Pages refresh cadence | The `rt-refresh` workflow runs at :11 and :41 (`.github/workflows/rt-refresh.yml`), so a Pages deployment is at best half-hourly. Scheduled runs are also throttled under load. | State the deployment mode on the page. Half-hourly data is a schedule animation with a delay hint and should be labelled as one; second-level freshness needs the self-hosted server. |
| No CORS on DataMall, and the key must stay server-side | The browser cannot fetch the feed directly | Unchanged from the board: the server or the workflow fetches, the page reads a snapshot. |
| OpenStreetMap and Overpass | Not needed. The schematic uses no external geodata. | Nothing to do; noted so it is not reintroduced. |

## Design rules compliance

The rules in `README.md:270` and `docs/ARCHITECTURE.md`, one line each.

| Rule | How the proof of concept honours it |
|------|-------------------------------------|
| Explicit input/output | The snapshot builder does no input or output; the caller fetches the feeds and passes them in, as `LiveBoardBuilder` does. Tests use the fixture feed and synthetic trip updates. |
| Plain data models | `NetworkSnapshot` is flat: index ids for lines, patterns, and stations, plain fields, `Serialize`. No graph objects, no back-references. |
| Small dependency set | No new dependency. `serde` for the view model, the existing crates for everything else. The renderer is hand-written SVG and vanilla JavaScript; the geometry helpers are copied functions, not a package. |
| Synchronous code | The builder is a pure function. Polling is the caller's concern, exactly as it is for the board. |
| No invented data | The one derived quantity — position — carries a provenance marker, renders as an estimate when it is one, is refused entirely for headway bands and cancelled trips, and degrades to schedule-only when realtime is stale. Everything unplaceable becomes a diagnostic. |
| Deterministic artefacts | The builder reads no clock; date, clock, and realtime `now` are arguments. Committed JSON snapshots from the fixture are the acceptance test, as they are for the publication view models. |
| One-way dependencies | The view model lives in `mrt-live` (or a sibling depending only on `mrt-gtfs` and `mrt-gtfs-rt`). Renderers are downstream and never query `RailNetwork`. The layout is data. |
| Progressive enhancement and escaping | The page shows the whole static network without JavaScript; the script adds motion and reveals its own controls. Feed text and colours pass the existing escape filter before they reach markup. |
| Diagnostics over silence | Unmatched layout stations, unplaceable runs, stale feeds, and missing realtime coverage are all reported on the page and in the JSON, not hidden. |
