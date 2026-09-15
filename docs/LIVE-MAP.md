# Published Singapore train map

The production map is at
https://umiyuikaiteitan.github.io/Singapore-MRTracker/map/ and linked from the
departure board. It is a geographic estimate over OpenStreetMap tiles, with an
option to hide tiles. All scripts and styles are hosted with the site.

## Build

The existing Pages workflow downloads the full official train GTFS feed once.
After OSM reconstruction, it runs:

```sh
cargo run --release -p mrt-map-static --bin mrt-map-schedule -- map-schedule.json feed.zip
node scripts/build-live-map.mjs singapore-starter.zip map-schedule.json site/map
node scripts/check-live-map.mjs site/map/network.json
```

The schedule exporter reuses `RailNetwork::query_trip_instances`: real service
calendars and exceptions, cross-midnight runs, exact frequency instances, and
non-exact headway bands. It covers 36 hours from the build. It does not synthesize
service calendars; the five-table editor ZIP is not a full schedule input.

Each run is joined by original trip/platform identifiers to its own reconstructed
shape and stop distance markers. The 13 background ribbons remove duplicate
directions; individual train runs still use their correct 35 directional shapes.
The build rejects missing shapes, mismatched calls, invalid time/distance order,
and unplaceable train legs. No `MRT_MAP_LAYOUT` repository variable is required.
The former `mrt-map-web`/`mrt-map-static` schematic prototype is still available
for separate development; the production browser map does not use that renderer.

## Playback and freshness

The browser advances through dwell times and every leg, including loops, without
waiting for a server snapshot. It reloads the schedule every five minutes and
stops drawing trains after its validity horizon. Users can filter trains by line,
toggle stations/tiles, fit the network, and select trains for their estimate source.

Live service reports use the departure board's config chain: the SRCF forwarder,
the public `live-data` branch, then the bundled report. The map polls every 30
seconds. The fallback branch refreshes every five minutes (GitHub schedules are
best effort). No DataMall key enters the browser or map build data.

Predictions require both a fresh report and a fresh GTFS feed timestamp (120
seconds), matching service date, and matching frequency start when relevant.
Old report schemas without prediction timestamps can provide notices but cannot
move trains. Stale/mismatched/conflicting predictions revert to the original
schedule; cancelled runs disappear only while their matching update is current.
Per-stop updates do not ambiguously affect repeated visits to the same platform.
The compact board report now includes feed timestamp and optional trip date/start/
measurement time; existing board consumers ignore these additive fields.

LTA publishes no measured vehicle locations. Hollow markers are schedule
estimates; solid markers incorporate fresh matching predictions. OSM geometry
is inferred routing, not an authoritative track assignment. Coverage and ODbL
attribution are published next to the map.
