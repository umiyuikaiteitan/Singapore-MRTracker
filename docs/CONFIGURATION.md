# The configuration reference

One YAML file drives both outputs. `config/singapore.yaml` is a
complete, commented example; this document is the reference.

```sh
cargo run -p mrt-schedule-cli -- timetable … --config config/singapore.yaml
```

Every option below is a **presentation** choice. No option makes the
generator invent schedule data. The two options that change what the
data *means* — `frequency_policy` and `missing_time_policy` — name the
policy explicitly, and the renderers mark every affected entry.

Unknown keys are an error, so a typo fails loudly instead of silently
doing nothing.

## The YAML subset

The reader accepts block mappings, block sequences, plain and quoted
scalars, `true`/`false`/`null`/`~`, numbers, the empty collections
`{}` and `[]`, comments, and a leading `---`. It does not read
anchors, aliases, tags, multi-line scalars, or non-empty flow
collections, and it reports the line number when it meets one.

Quote a time. `04:00:00` is a string, and an unquoted one still parses
correctly here, but quoting keeps the intent clear and matches how
other YAML tools read it.

## Top level

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `version` | integer | `1` | The configuration schema version. A different value is an error. |
| `profile` | string | — | A free-text name, for example `singapore-lta`. Recorded, not interpreted. |
| `timezone` | string | from the feed | The time zone that the output states. Overrides `agency_timezone`. |
| `day_start` | time | `"04:00:00"` | The first hour of the timetable service day. |
| `day_duration_hours` | integer | `24` | How many hours the service day covers. 1 to 48. |
| `frequency_policy` | enum | `bands` | How headway service is treated. See below. |
| `missing_time_policy` | enum | `interpolate-bounded` | How missing stop times are filled in. See below. |
| `language` | `en` \| `ja` | `en` | The language of the interface labels. |

With the defaults, a timetable for `2026-08-10` covers `04:00:00`
through `27:59:59` and displays the final rows as `00`, `01`, `02`,
`03`. Those rows are the small hours of 11 August, and every departure
in them carries the past-midnight mark.

### `frequency_policy`

| Value | Effect |
|-------|--------|
| `bands` | Service with `exact_times=0` becomes a band such as `06:30–09:00  every 4 min approximately`. No minute is invented. **The default.** |
| `expand-approximate` | The same service becomes single entries, every one of them marked approximate, dotted in the timetable and dashed in the diagram, and explained in the legend. |
| `reject-non-exact` | The run fails with exit code 6 when such service affects the requested output. |

Service with `exact_times=1` is always expanded, because a repeating
template really does describe exact times.

### `missing_time_policy`

| Value | Effect |
|-------|--------|
| `none` | A missing time stays missing. The diagram simply omits that point. |
| `interpolate-bounded` | Interpolate only *between* two known times, and mark the result. A gap before the first or after the last known time stays missing. **The default.** |
| `interpolate-unbounded` | Also extend the first and last known times outwards at the neighbouring rate. This invents times outside the range that the feed supplies. |

The weight of an interpolation comes from `shape_dist_traveled` when
every call of the trip has it, then from the great-circle distance
between the stations, then from the position of the call.

## `timetable`

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `layout` | enum | `responsive` | `single`, `split-at`, `balanced`, or `responsive`. |
| `columns` | integer | `2` | Columns for `balanced` and `responsive`. 1 to 6. |
| `split_at` | list of integers | `[]` | Service hours at which `split-at` breaks, for example `[16]`. |
| `show_empty_hours` | boolean | `true` | Keep an hour row that carries no departure. |
| `seconds` | enum | `show-if-nonzero` | `hide`, `show`, or `show-if-nonzero`. |
| `group_by_platform` | boolean | `true` | Give each platform its own panel, when the feed names platforms. |
| `split_by_destination` | boolean | `false` | Give each destination its own panel. |
| `mark_first_and_last` | boolean | `true` | Mark the first and last departure of each panel. |
| `show_trip_short_name` | boolean | `true` | Print `trips.trip_short_name` beside a departure. |
| `title` | text | `"{station} departure timetable"` | `{station}`, `{line}`, and `{date}` fill in. |

`responsive` uses the columns on a wide screen and in print, and
stacks them on a narrow screen, so a phone never scrolls sideways.

A balanced split weights each hour row by the number of printed lines
its departures need, not by the number of departures, so a quiet
morning and a busy evening still produce columns of similar height.

## `diagram`

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `station_spacing` | enum | `equal` | `equal`, `distance`, or `manual`. |
| `major_grid_minutes` | integer | `60` | The strong grid lines. |
| `medium_grid_minutes` | integer | `30` | The medium grid lines. |
| `minor_grid_minutes` | integer | `10` | The faint grid lines. |
| `show_dwell` | boolean | `true` | Draw the horizontal segment where a train stands at a station. |
| `show_trip_labels` | enum | `auto` | `never`, `auto`, or `always`. |
| `show_internal_trip_ids` | boolean | `false` | Put the GTFS `trip_id` in the hover details. |
| `pixels_per_hour` | number | `240` | The width of one hour in user units. |
| `row_height` | number | `34` | The height of one station row. |
| `title` | text | `"{corridor} train diagram"` | `{corridor}`, `{line}`, and `{date}` fill in. |

`distance` spacing needs station positions in the feed. Without them
the diagram falls back to `equal` spacing and says so with a
`distance-spacing-unavailable` diagnostic.

`show_internal_trip_ids` never puts the identifier on the drawing as a
train number. It only adds it to the hover details and to the call
tables, for debugging.

`auto` labelling places a label on the longest travel segment of each
run that still has room, and hides the rest, which the page reveals on
hover and on keyboard focus. `always` writes every label, overlaps
included.

## `labels`

Every entry here is an explicit override for something GTFS does not
carry. Nothing in this section is derived from the feed.

```yaml
labels:
  direction_overrides:
    # Keyed by "<route_id>:<direction_id>", or "<route_id>:none".
    "NS:0":
      en: Southbound
      ja: 下り
    "NS:1":
      en: Northbound
      ja: 上り
  destination_abbreviations:
    Marina Bay: Mar Bay
  platform_overrides:
    # Keyed by the GTFS stop_id of the platform.
    JUR_NS: A
  corridor_overrides:
    ewl-main:
      en: East West Line
```

`direction_overrides` is the **only** way a heading such as
"Northbound", 上り, or 下り appears. GTFS gives `direction_id` no
meaning, so the generator will not guess one: without an override, a
panel is headed by the destinations it really carries, and falls back
to a plain `Direction 0`.

`destination_abbreviations` shortens the annotation beside a minute.
The full name stays in the accessible text and in the tooltip.

## `theme`

| Key | Type | Default |
|-----|------|---------|
| `font_stack` | list of strings | `[Noto Sans, Noto Sans JP, Hiragino Kaku Gothic ProN, Arial, sans-serif]` |
| `hour_cell` | colour | `#1b2a5e` |
| `hour_cell_text` | colour | `#ffffff` |
| `row_alternate` | colour | `#eef1f8` |
| `background` | colour | `#ffffff` |
| `text` | colour | `#14171f` |
| `accent` | colour | `#1b2a5e` |

A colour must be `#` followed by three, four, six, or eight
hexadecimal digits. Anything else falls back to the default rather
than reaching the stylesheet. Font names are stripped of everything
but letters, digits, spaces, hyphens, and underscores, and are then
quoted.

The generator embeds no font file. Name families that the reader is
likely to have, or that they can install themselves; the stack ends in
a generic family so a page always renders.

A line that carries `route_color` overrides the accent for its own
panel, so a document with two lines keeps both identities.

## `corridors`

A corridor is the vertical axis of a diagram. Define one when a line
has a branch, or when a run of `diagram --line …` reports
`corridor-split`.

```yaml
corridors:
  - id: ewl-main
    line: EW
    label:
      en: East West Line
      ja: 東西線
    axis:
      - EW1
      - EW2
      - EW3
      - EW4
    branches:
      - junction: EW4
        axis:
          - CG1
          - CG2
        label:
          en: For Changi Airport
    offsets: []
```

| Key | Meaning |
|-----|---------|
| `id` | The identifier that `--corridor` selects. Must be unique. |
| `line` | The line whose trips the corridor draws. A `route_id` or a route short name. Optional; without it, every line is drawn. |
| `label` | The heading. Falls back to `labels.corridor_overrides`, then to the identifier. |
| `axis` | The stations of the main axis, in travel order. Station codes, GTFS identifiers, or names. At least two. |
| `branches` | The branches that leave the main axis. |
| `branches[].junction` | A station that must be on the main axis. |
| `branches[].axis` | The branch stations, in travel order away from the junction. |
| `offsets` | Manual vertical offsets, one per axis entry, for `station_spacing: manual`. |

A run is mapped to the corridor by matching its calls against each
path in order — the main axis, and the main axis up to each junction
followed by that branch — forwards and then backwards. A run that
matches no path is left out with a `run-off-corridor` diagnostic. It
is never bent onto the axis.

A station that appears twice on the axis, as an unrolled loop does,
gets a separate node with its own occurrence index, so the two visits
are distinct positions.

## Checking a configuration

```sh
cargo run -p mrt-schedule-cli -- timetable \
  --feed crates/mrt-gtfs/tests/fixtures/mini \
  --station TE1 --date 2025-05-05 \
  --config config/singapore.yaml --out -
```

A configuration error exits with code 2 and names the file, the line,
and the value.
