# mrt-display

Pure Rust projection for an addressable LED station map and a separate
e-ink departure board. No credentials, fetching, filesystem access or clock
reads occur in this crate.

```rust,no_run
use mrt_display::{build_frame, FrameOptions, Layout};
// The application loads a RailNetwork and parses a Layout with serde_json.
# fn example(network: &mrt_gtfs::RailNetwork, layout: &Layout) {
let options = FrameOptions::new(
    1_786_314_000,
    "20260810".parse().unwrap(),
    "06:00:00".parse().unwrap(),
);
let frame = build_frame(network, layout, None, &options).unwrap();
# }
```

`service_date`/`clock` must be the actual local calendar date and 24-hour
Singapore time associated with `now_unix`. The caller must validate schedule
coverage; an empty query cannot distinguish a nighttime gap from an expired
calendar. Use `Frame::unavailable` when the application knows no valid source
exists.

## Meaning and wire contract

Pixels show whether a station has an upcoming boarding departure within the
configured horizon. **They are not vehicle locations.** Exact-looking train
movement must not be synthesized from a timetable. Each frame includes every
index in ascending wired order, including black pixels, so disappeared
service cannot leave old LEDs illuminated.

Version 1 always emits `schema_version`, `layout_id`, `generated_at`,
`valid_until`, `source_state`, `pixels` and `board`. A pixel has `index,r,g,b`;
a board has `station_name,rows,notice`; a row has
`line,destination,minutes,approximate,realtime`. Times are POSIX seconds.
Minute waits round **up**, so a 30-second wait displays 1 minute. Render
`approximate` with `~` and render row-specific realtime status explicitly.

| Source state | Meaning | LED frame |
| --- | --- | --- |
| `schedule` | Timetable only; even a fresh RT feed may lack matching predictions | Dim line color (maximum channel 24) |
| `realtime` | A selected departure uses a fresh delay, or a cancellation affects visible service | Matched departures brighter (maximum channel 96); schedule rows remain dim |
| `stale` | Realtime input has stale, future or missing freshness evidence | Black; board explicitly shows timetable fallback |
| `unavailable` | Application reports unusable source data | Black with empty rows |

`realtime` at frame level does not make every row realtime. Service alerts
can remove a departure without changing provenance of the surviving rows.
The frame deadline is capped by the expiry of every used measurement. Text
from live alerts also expires with its feed. Consumers must enforce
`valid_until` independently, show the notice, reject mismatched layout IDs,
and enforce their own brightness/current limit.

Bounds: 512 pixels, eight rows, 300-second maximum frame TTL; UTF-8 byte limits
are station 80, line 12, destination 96 and notice 240. Defaults are six rows,
30-minute lookahead, and 120-second TTL/RT freshness. A configurable RT age
limit may be 1–3600 seconds, but does not lengthen the frame TTL.

## Conservative matching

- Layout station references must be public codes or exact GTFS station IDs.
  Optional `station_aliases` explicitly list other interchange platform codes
  or historical identifiers. Distinct resolved parents are merged; identifiers
  resolving to the same parent are deduplicated. At least one identifier per
  physical marker must resolve, and ambiguous identifiers always fail. Missing
  identifiers produce `Layout::warnings` and a visible frame notice, including
  when a historical alias replaces a missing primary identifier. Applications
  should log every warning and review the physical inventory when feeds change.
  A board station naming a configured marker uses the same merged group.
  Names never choose an arbitrary interchange record. Each optional line filter
  must serve the station. Coordinates and unique electrical indices are validated.
- Freshness requires a feed header timestamp at or before `now_unix`, with
  age strictly below `max_rt_age_secs`. Trip measurement timestamps, when
  present, must satisfy the same rule. Missing trip timestamps inherit the
  header timestamp; missing header timestamps invalidate the feed.
- A trip update must match its trip ID, any supplied route/direction/date,
  and any supplied start time. A frequency template additionally needs both
  start date and start time, so one update cannot cancel every run. Duplicate
  matches are ignored. Date-less updates cannot attach to previous-day runs.
- A numerical departure/arrival delay or absolute event timestamp at the
  actual platform, or a
  trip-level delay with **no** stop updates, changes a row to realtime.
  Missing/unsupported predictions remain visibly scheduled. This avoids
  propagation through `NO_DATA` that the existing flattened RT model cannot
  represent. Absolute timestamps take precedence over delay, as required by
  the [GTFS-Realtime reference](https://gtfs.org/documentation/realtime/reference/#message-stoptimeevent).
  Arrival-only timestamps preserve scheduled dwell in the departure countdown.
  Stop-sequence-only updates are not used; repeated visits to the same platform
  also cannot receive stop-specific updates because the current query model
  does not retain original GTFS stop-sequence numbers.
- Delays up to one hour in either direction are searched and applied. A
  matched out-of-range delay suppresses the candidate with a notice.
  Previous-day service after 24:00 and next-day departures crossing midnight
  are included. Pickup restrictions, terminal calls, canceled trips and
  skipped platforms are respected.
- Non-exact frequency runs are explicitly expanded as approximate estimates;
  interpolated and `timepoint=0` calls also retain `approximate=true`.
- Fresh active GTFS-RT no-service alerts suppress matching departures. Fields
  within each informed-entity selector are ANDed. Agency-only selectors are
  unsupported because the linked network does not retain agency membership.
  Other alert text may appear but does not create a realtime countdown.

`tests/fixtures/tel-four.json` deliberately maps only TE1–TE4, the stations
in the repository's miniature GTFS fixture. Physical full-network layouts
must be paired with a real GTFS feed containing all configured stations.

Run `cargo test -p mrt-display` from the repository root.
