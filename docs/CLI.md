# The `mrt-schedule-cli` reference

`mrt-schedule-cli` turns a GTFS Schedule feed into two printed
products: a Japanese-style station departure timetable and a
planning-style time–distance train diagram.

```sh
cargo run -p mrt-schedule-cli -- --help
```

## Commands

| Command | What it does |
|---------|--------------|
| `fetch` | Download the current train GTFS Schedule archive and cache it. |
| `timetable` | Build a departure timetable for one station. |
| `diagram` | Build a train diagram for one line, pattern, or corridor. |
| `validate` | Check a feed and print its diagnostics. |
| `stations` | List the lines and stations of a feed. |

## Examples

Fetch and cache the current LTA train schedule:

```sh
export LTA_DATAMALL_ACCOUNT_KEY=<your key>
cargo run -p mrt-schedule-cli -- fetch \
  --source datamall \
  --out cache/current.zip
```

Generate a station timetable:

```sh
cargo run -p mrt-schedule-cli -- timetable \
  --feed cache/current.zip \
  --station NS1 \
  --date 2026-08-10 \
  --line NSL \
  --config config/singapore.yaml \
  --out dist/ns1-2026-08-10.html \
  --manifest dist/ns1-2026-08-10.manifest.json
```

Generate a five-hour string diagram:

```sh
cargo run -p mrt-schedule-cli -- diagram \
  --feed cache/current.zip \
  --line EWL \
  --date 2026-08-10 \
  --from 05:00:00 \
  --until 10:00:00 \
  --config config/singapore.yaml \
  --out dist/ewl-2026-08-10.html
```

Produce the view model for another renderer:

```sh
cargo run -p mrt-schedule-cli -- diagram \
  --feed cache/current.zip \
  --line EWL \
  --date 2026-08-10 \
  --from 05:00:00 \
  --until 10:00:00 \
  --format json \
  --out dist/ewl-2026-08-10.json
```

A standalone drawing, with no page around it:

```sh
cargo run -p mrt-schedule-cli -- diagram \
  --feed cache/current.zip \
  --line TEL \
  --date 2026-08-10 \
  --format svg \
  --out dist/tel.svg
```

Find a station code, then check the feed:

```sh
cargo run -p mrt-schedule-cli -- stations --feed cache/current.zip | grep -i punggol
cargo run -p mrt-schedule-cli -- validate --feed cache/current.zip --strict
```

## Options

### Source

| Option | Meaning |
|--------|---------|
| `--feed <PATH>` | A GTFS zip archive, or a directory of feed files. |
| `--source datamall` | Download from LTA DataMall instead of reading a file. |
| `--cache-dir <PATH>` | The feed cache. Default `cache`. |
| `--allow-stale` | After a failed download, use the newest cached feed. Every generated page then says that it came from a cached feed. |
| `--account-key-env <NAME>` | The environment variable that holds the DataMall account key. Default `LTA_DATAMALL_ACCOUNT_KEY`. |

Every command except `fetch` needs either `--feed` or
`--source datamall`.

### Selection

| Option | Meaning |
|--------|---------|
| `--station <CODE>` | A station code (`NS1`, `ns-1`, `EW24`), a GTFS `stop_id`, or a station name. Required for `timetable`. |
| `--line <NAME>` | A GTFS `route_id`, a route short name, or a route long name. |
| `--pattern <INDEX>` | A stop pattern index. Use it to draw exactly one pattern. |
| `--corridor <ID>` | A corridor from the configuration. |
| `--date <YYYY-MM-DD>` | The service date. `YYYYMMDD` also works. Required for `timetable` and `diagram`. |
| `--from <HH:MM[:SS]>` | The start of a diagram window. Hours may pass 24. |
| `--until <HH:MM[:SS]>` | The exclusive end of a diagram window. |

`diagram` takes exactly one of `--line`, `--pattern`, or `--corridor`.

On a `timetable`, `--line` narrows an interchange to one line. Without
it, an interchange shows every line that serves it.

### Output

| Option | Meaning |
|--------|---------|
| `--out <PATH>` | The artifact to write. `-` writes to standard output. Without it, the artifact goes to standard output. |
| `--format html\|svg\|json` | Default `html`. A timetable has no SVG form. |
| `--manifest <PATH>` | Also write a generation manifest. |
| `--config <PATH>` | A YAML configuration file. See `docs/CONFIGURATION.md`. |
| `--language en\|ja` | Override the configured interface language. |
| `--frequency-policy <P>` | `bands`, `expand-approximate`, or `reject-non-exact`. Overrides the configuration. |
| `--strict` | Read the archive strictly (feed files at the root) and validate strictly. |
| `--warnings-as-errors` | Exit with code 4 when the run produces a warning. |
| `--quiet` | Print nothing but errors. |

Every artifact is written through a temporary file and an atomic
rename, so an interrupted run never leaves half a timetable behind.

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success. |
| 2 | Invalid command line or configuration. |
| 3 | The feed could not be fetched or read. |
| 4 | The feed is not a valid GTFS feed. |
| 5 | A station, line, pattern, or corridor could not be resolved. |
| 6 | The requested output cannot be represented under the selected policy. |
| 7 | Rendering or writing a file failed. |

Warnings do not change the exit code unless `--warnings-as-errors` is
set.

Code 6 is the honest-output code. It appears when, for example,
`--frequency-policy reject-non-exact` meets a line whose service is
described only by a headway: the generator refuses to print times that
the feed does not contain.

## The cache

`fetch`, and any command with `--source datamall`, store the archive
under its own SHA-256:

```text
cache/
  current.json            the newest object and its metadata
  objects/<sha256>.zip    the archive
  metadata/<sha256>.json  when it arrived, and what DataMall reported
```

Storing the same bytes twice reuses the object, so repeated fetches
cost one file each time the feed actually changes.

A download is validated before anything durable changes: the archive
must parse as a GTFS feed first, and only then does the cache store
the object and advance `current.json`, and only then is `--out`
written — each by an atomic rename beside its target. A corrupt
response therefore fails the run (exit 4), leaves the last good cache
entry in place as the `--allow-stale` fallback, and never overwrites
an existing `--out` file.

`--allow-stale` is the only way a cached feed stands in for a failed
download — a download that does not parse counts as failed — and
every page generated that way carries a visible "generated from a
cached feed" notice.

## The manifest

`--manifest` writes a JSON record of the run:

```json
{
  "manifest_version": "1.0",
  "generator_version": "mrt-schedule-cli 0.1.0",
  "generated_at": 1786406400,
  "command": "timetable",
  "feed_sha256": "…",
  "feed_timestamp": "2026-08-10T00:00:00+08:00",
  "feed_source": "GTFSScheduleTrain",
  "feed_from_cache": false,
  "configuration_sha256": "…",
  "configuration_path": "config/singapore.yaml",
  "service_date": "20260810",
  "timezone": "Asia/Singapore",
  "schema_version": "1.0",
  "artifacts": [
    {
      "path": "dist/ns1-2026-08-10.html",
      "kind": "timetable",
      "format": "html",
      "bytes": 20720,
      "sha256": "…"
    }
  ],
  "diagnostics": []
}
```

`generated_at` is the only value in any output that reads a clock. The
documents themselves carry no timestamp, so the same feed, date,
configuration, and generator version produce byte-identical artifacts.

`feed_source` is a path or a DataMall endpoint name. A signed download
URL never reaches it.

## Secrets

The DataMall account key is read from an environment variable and goes
nowhere else:

- It travels only in the `AccountKey` header of a request to the
  DataMall host.
- The download of a pre-signed link sends no headers at all.
- Signed query strings are redacted before any message is printed.
- The cache, the manifest, and every generated page are checked by a
  test that fails if the key appears in them.

## Diagnostics

Both documents carry the diagnostics of their run, and the pages show
the ones at warning severity in a banner. The common codes:

| Code | Meaning |
|------|---------|
| `time-interpolated` | A missing stop time was computed between two known ones. |
| `time-missing` | A call has no time and lies outside the known times, so part of a run is not drawn. |
| `frequency-zero-headway` | A `frequencies.txt` block describes no service. |
| `frequency-expanded-approximate` | Headway service was expanded on request; every entry is marked. |
| `corridor-split` | Some patterns need their own panel. Define a corridor to place them on one axis. |
| `run-off-corridor` | A run does not follow the station axis, so it was left out rather than bent onto it. |
| `distance-spacing-unavailable` | Distance spacing was asked for but the feed has no usable positions, so the axis is evenly spaced. |
| `timetable-empty` | No boardable departure at that station on that service date. |
