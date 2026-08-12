# The Singapore train GTFS profile

This document records what the project knows about the LTA train GTFS
Schedule feed, and — just as important — what it does **not** know.

Singapore data is treated as a *profile* of standard GTFS, not as a
separate format. Every rule below either comes from the general GTFS
specification, from the LTA DataMall API User Guide, or from an
observation that this repository already records. Nothing here is a
guess about feed content.

## Status of this document

> **No LTA account key was available when this profile was written.**
>
> The rows marked **unverified** below have *not* been checked against
> a downloaded feed. The implementation does not depend on them: where
> a fact is unverified, the code reads what the feed actually says,
> falls back to a documented default, or emits a diagnostic. It never
> assumes the unverified value.

To fill in the unverified rows, run:

```sh
export LTA_DATAMALL_ACCOUNT_KEY=<your key>
cargo run -p mrt-schedule-cli -- fetch --source datamall --out data/train.zip
cargo run -p mrt-schedule-cli -- validate --feed data/train.zip --strict
cargo run -p mrt-schedule-cli -- stations --feed data/train.zip
```

`validate --strict` reports every deviation from the letter of the
specification, and `stations` lists the lines and stations that the
feed really carries. Record the answers here and mark the rows
verified.

## 1. Acquisition

| Item | Value | Source |
|------|-------|--------|
| Endpoint | `GTFSScheduleTrain` | API User Guide 6.9 |
| Base URL | `https://datamall2.mytransport.sg/ltaodataservice` | API User Guide 6.9 |
| Authentication | The `AccountKey` request header | API User Guide 6.9 |
| Response | An OData envelope whose `value` array carries `timestamp` and `link` | official sample |
| Behind the link | `gtfs_schedule.zip`, a GTFS Schedule feed | API User Guide 6.9 |
| Link lifetime | A pre-signed URL, about 15 minutes (`X-Amz-Expires=900`) | `docs/DATA-SOURCES.md` |
| Download auth | None. The link carries its own signature | API User Guide 6.9 |

Consequences that the implementation enforces:

- The account key never leaves the DataMall request. `download` sends
  no headers at all, so the key cannot reach the storage host.
- The link is downloaded immediately after it is issued. An expired
  link produces a *new* link request rather than a retry.
- Every signed query string is redacted before it can reach a log, a
  manifest, or a page. See `mrt_datamall::redact_url`.

## 2. Files

The API User Guide names this basic dataset:

`agency.txt`, `routes.txt`, `trips.txt`, `stops.txt`, `stop_times.txt`,
`calendar.txt`, `calendar_dates.txt`.

The loader also reads `frequencies.txt`, `transfers.txt`, and
`shapes.txt` when they are present, and treats every one of them as
optional. A feed that omits an optional file simply produces an empty
table.

| Question | Answer | Status |
|----------|--------|--------|
| Are the files at the root of the archive? | Standard GTFS requires it. The loader accepts a subdirectory in lenient mode and refuses one in `--strict`. | **unverified** |
| Is `frequencies.txt` present? | The code handles both. | **unverified** |
| Is `shapes.txt` present? | The code handles both. | **unverified** |
| Do the files carry a byte-order mark? | The loader strips one either way. | **unverified** |

## 3. Time zone and calendar

| Item | Value | Status |
|------|-------|--------|
| `agency_timezone` | Expected `Asia/Singapore` | **unverified** |
| Calendar files | Both `calendar.txt` and `calendar_dates.txt` are named in the guide | guide |

The generator takes the time zone from `agency.txt` and states it in
every document and manifest. When the agencies disagree, `--strict`
validation reports it. The configuration can override the value.

Singapore observes no daylight saving time and has a fixed `+08:00`
offset, so a service day never gains or loses an hour. The library
still keeps every time as service-day seconds and never converts to a
wall clock before display.

## 4. Route types

The rail filter accepts the standard rail types 0, 1, 2, and 12, and
the extended types 100–117, 400–405, and 900–906. Singapore MRT lines
are metro (type 1); the LRT lines appear as tram (0) or monorail (12)
depending on the publisher.

| Question | Answer | Status |
|----------|--------|--------|
| Which `route_type` values does the train feed use? | The filter accepts every plausible rail value, so the answer does not change behaviour. | **unverified** |
| Does the feed carry non-rail routes? | The train endpoint should carry rail only; the filter removes anything else regardless. | **unverified** |

## 5. Routes and lines

One observation is already recorded in this repository, in
`crates/mrt-gtfs/src/network.rs` and in the tests:

> The official LTA feed splits the Circle Line into several route
> entries that share one display name.

This is why `RailNetwork::is_interchange` compares line *names* rather
than counting route entries, and why a diagram of "the Circle Line"
may need a configured corridor.

| Question | Answer | Status |
|----------|--------|--------|
| Do `route_short_name` values match the DataMall line codes (`NSL`, `EWL`, `TEL`, …)? | The generator prints whatever the feed carries. | **unverified** |
| Does every route carry `route_color`? | A missing or malformed colour falls back to the theme accent; `validate` reports a malformed one. | **unverified** |
| How many route entries make up one physical line? | At least the Circle Line uses several. | observed, historic |

## 6. Stations and platforms

The station codes that the alerts and crowd APIs use are documented in
`docs/DATA-SOURCES.md`:

- MRT: `NS`, `EW`, `CG`, `NE`, `CC`, `CE`, `DT`, `TE`, plus a number.
- LRT: `BP`, `STC`, `SE`, `SW`, `PTC`, `PE`, `PW`, plus a number.

The network model groups stops into stations by `parent_station`, and
a stop without a parent becomes its own station. An interchange
therefore carries several codes, for example `NS1` and `EW24` for
Jurong East, and every code resolves to the same station.

| Question | Answer | Status |
|----------|--------|--------|
| Does the feed use `location_type=1` parent stations? | The model works with and without them. Without them an interchange splits into separate stations, which the diagram and timetable would show as separate places. | **unverified** |
| Is `stop_code` the public station code? | The alias index assumes so; it is the only field with that meaning. | **unverified** |
| Is `platform_code` populated? | The timetable prints a platform only when the feed supplies one, and never derives one from the direction. | **unverified** |
| Do the two directions of a line use distinct platform stops? | Only then can the timetable group by platform. Without it, both directions share one panel per direction. | **unverified** |

If `platform_code` turns out to be absent, set
`timetable.group_by_platform: false` and, where the platform numbers
are known from another source, list them under
`labels.platform_overrides`, keyed by the GTFS `stop_id`.

## 7. Trips

| Field | Use here | Status |
|-------|----------|--------|
| `trip_headsign` | The destination, after `stop_headsign` | **unverified** whether populated |
| `trip_short_name` | The **only** value that may appear as a train number | **unverified**; likely absent |
| `direction_id` | A grouping key with no compass meaning | **unverified** |
| `block_id` | Retained; not yet used to join consecutive trips | **unverified** |
| `shape_id` | Used only through `shapes.txt` | **unverified** |

If `trip_short_name` is absent — which is common for metro feeds — the
diagram simply draws no run labels, and the timetable prints no train
number. It never falls back to `trip_id`, because that is an internal
key and printing it would invent a train number that no passenger can
use.

## 8. Stop times

| Field | Use here | Status |
|-------|----------|--------|
| `arrival_time`, `departure_time` | The schedule | **unverified** whether every call carries both |
| `stop_headsign` | The destination, before `trip_headsign` | **unverified** |
| `pickup_type` | A call with `1` produces no timetable departure | **unverified** |
| `drop_off_type` | With `pickup_type=1`, marks a pass-through in the diagram | **unverified** |
| `timepoint` | `0` marks the feed's own times as approximate | **unverified** |
| `shape_dist_traveled` | Weights time interpolation and distance spacing | **unverified** |

Where intermediate times are missing, the generator interpolates only
*between* two known times, weights the interpolation by
`shape_dist_traveled`, then by the great-circle distance between the
stations, then by the position of the call, and marks every computed
time. A gap before the first or after the last known time stays
missing unless `missing_time_policy: interpolate-unbounded` is set.

## 9. Frequencies

`frequencies.txt` decides whether a schedule exists at all:

- `exact_times=1` — the template repeats exactly. Expanding it into
  single trips is correct, and the generator does so.
- `exact_times=0` or empty — the feed says "a train roughly every N
  seconds". The individual departure times **do not exist**. The
  generator produces a band such as
  `06:30–09:00  every 4 min approximately`, and refuses to print
  invented minutes unless the caller sets
  `frequency_policy: expand-approximate`, which marks every entry.

| Question | Answer | Status |
|----------|--------|--------|
| Does the train feed use `frequencies.txt`? | Both paths are implemented and tested. | **unverified** |
| Which `exact_times` values appear? | Both are handled; anything else is a validation error. | **unverified** |

## 10. Branches, loops, and short turns

Singapore's network contains all three shapes:

- **Branches.** The East West Line has the Changi Airport branch; the
  Circle Line has the HarbourFront extension.
- **Loops.** The Sengkang and Punggol LRT lines run as loops, so one
  run calls at the interchange twice.
- **Short turns.** Peak services that terminate short of the end of
  the line.

The generator handles each explicitly:

- A pattern that is a subsequence of the spine joins the same axis,
  forwards or backwards.
- A pattern that fits neither way gets its own panel, and the document
  carries a `corridor-split` warning that names the configuration
  option that would place it on one axis.
- A loop is unrolled once, and each repeat of a station becomes a
  separate corridor node with its own occurrence index. A run that
  goes round more often than the corridor was unrolled for is left out
  with a `run-off-corridor` diagnostic — never bent onto the axis.
- A short turn stays in the same direction panel of a timetable, with
  its own destination beside each departure.

| Question | Answer | Status |
|----------|--------|--------|
| How does the feed model the LRT loops? | Both a single looping pattern and two half-loop patterns are handled. | **unverified** |
| Are branch services separate `route_id` values? | Either way works; separate routes need a corridor to share one axis. | **unverified** |

## 11. Realtime

The GTFS-Realtime endpoints (`GTFSRealtimeTrainTripUpdates` and
`GTFSRealTimeTrainServiceAlerts`) are **not** part of this pipeline.
The static outputs are a published timetable, and mixing live delays
into them would make a printed page wrong the moment it is printed.

The interfaces leave room for a later live layer: `TripInstance`
carries `instance_id` and `source_trip_id`, and `DiagramRun` carries
its calls, which is what a delay overlay needs to attach to.

## 12. Observed deviations from standard GTFS

| Deviation | Effect | Source |
|-----------|--------|--------|
| One line split across several `route_id` values with the same short name | Interchange detection compares names; a diagram of that line may need a corridor | recorded in this repository |
| Long-running "modified service" alerts for planned changes, e.g. the Sengkang West LRT loop closure | Realtime only; the static timetable already carries the change | `docs/ARCHITECTURE.md`, `mrt-live` |

## 13. What to do when an assumption turns out to be wrong

Nothing in the pipeline hard-codes a Singapore-specific assumption.
Every one of them lives in exactly one of three places:

1. **The configuration** (`config/singapore.yaml`) — direction
   headings, platform overrides, destination abbreviations, corridors.
2. **The adapters** (`mrt-datamall`, the rail filter) — endpoints,
   route types.
3. **The diagnostics** — anything the generator cannot resolve is
   reported by code, not guessed.

So a surprise in the real feed becomes a configuration entry or a
diagnostic, not a patch spread through the rendering code.
