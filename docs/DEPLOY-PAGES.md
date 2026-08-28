# Deploy on GitHub Pages

The published site has two sections under one domain:

| Path | What it is |
|------|------------|
| `/` | The live dot-matrix departure board. |
| `/timetables/` | Browsable station timetables and train diagrams. |

GitHub Pages serves static files only. Both sections are therefore
generated ahead of time by a scheduled GitHub Actions workflow, from
one download of the GTFS Schedule feed. The board computes wait times
from the visitor's clock; train schedules are deterministic, so it
stays accurate between refreshes.

```text
GitHub Actions "pages" (hourly)
  ├─ mrt-schedule-cli fetch + account key (Actions secret)
  │    └─ downloads the GTFS Schedule feed once -> feed.zip
  ├─ mrt-board-static + feed.zip
  │    ├─ computes departures per station -> data/board/<CODE>.json
  │    ├─ reads alerts, crowd, trip updates -> data/live.json
  │    └─ writes the board at the site root
  ├─ mrt-schedule-site + feed.zip
  │    └─ writes timetables/ : a hub, one page per station and date,
  │       one diagram per line, date, and time window
  └─ deploys everything to GitHub Pages

GitHub Actions "rt-refresh" (every five minutes)
  └─ mrt-rt-snapshot + account key
       └─ force-pushes live.json to the live-data branch
          (alerts, crowd levels, per-trip delays and cancellations)

browser
  └─ index.html reads the JSON files and renders the dot-matrix
     board; wait = departure instant + live delay - Date.now().
     The page fetches the live-data snapshot on every visit and
     every 30 seconds. A delayed trip shows a red time, a trip that
     runs ahead of its schedule a yellow one; a canceled trip shows
     CANC.
```

The account key never reaches the browser. It exists only inside the
Actions runner, and the published files contain schedule and status
data only.

## One-time setup

1. **Pages availability.** GitHub Pages on a private repository
   needs a paid GitHub plan. On the free plan, make the repository
   public first, or mirror the workflow into a separate public
   repository.
2. **Enable Pages.** Repository Settings → Pages → Source:
   **GitHub Actions**.
3. **Add the secret.** Repository Settings → Secrets and variables →
   Actions → New repository secret. Name:
   `LTA_DATAMALL_ACCOUNT_KEY`. Value: your DataMall account key.
4. Push to the default branch, or run the **Deploy board to GitHub
   Pages** workflow manually from the Actions tab.

The site then appears at
`https://<owner>.github.io/<repository>/`.

## Delays and cancellations

A static page cannot call DataMall itself: the account key would be
public, and DataMall sends no CORS headers. The nearest equivalent
of a live connection is the `rt-refresh` workflow. It decodes the
GTFS-Realtime trip updates in the Actions runner and publishes a
small snapshot to the `live-data` branch, which
`raw.githubusercontent.com` serves with open CORS. The page fetches
that snapshot when a visitor opens it and every 30 seconds after,
so a delay reaches the board within minutes of LTA publishing it.

The snapshot maps trip identifiers to delays: `d` is the trip-level
delay in seconds, `c` marks a cancellation, and `s` carries
per-stop delays and skipped stops. The board files carry the trip
identifier of every departure, so the page joins the two on the
client.

The snapshot also decodes the GTFS-Realtime service alerts feed
into an `alerts` array: the display text, the effect, the active
periods, and the informed routes, stops, and trips. The board files
carry the route identifier of every departure and the platforms of
every station, so the page joins the alerts on the client too: a
no-service alert cancels the affected departures (`CANC`), reduced
service, significant delays, and a detour turn them red, and the
alert text scrolls in the ticker. A modified schedule only scrolls:
the feed uses that effect for planned adjustments that run for
months, which the timetable already carries, so flagging their
departures would leave whole lines permanently red. The page
applies the active periods against the visitor's clock, so an alert
takes effect and expires between snapshot refreshes. A no-service
alert that names a trip is also folded into the trips map, which
older cached pages understand.

For true per-request freshness, put a small proxy with the key in
front of DataMall (for example a Cloudflare Worker) and set its URL
as `MRT_DELAYS_URL` in `pages.yml` — the page follows whatever URL
`data/config.json` names.

## How the workflows run

- The workflow file is `.github/workflows/pages.yml`.
- It runs twice per hour, on every push to the default branch, and
  on manual dispatch. Scheduled runs fire from the default branch
  only.
- Each run rebuilds the generator (fast with the cargo cache),
  regenerates all data, and redeploys the site.
- Without the secret, the run fails at the generate step with a
  clear message.

## The timetable section

`mrt-schedule-site` writes a browsable section beside the board:

```text
timetables/
  index.html                   the hub for today
  day-<YYYYMMDD>.html          the hub for another service date
  t/<code>-<date>.html         one station departure timetable
  d/<line>-<date>-<window>.html  one train diagram
  d/<line>-<date>-<window>.svg   the same drawing on its own
  data/index.json              the machine-readable index
```

The hub lists every line and every station. The station list is in
the document, so it works without JavaScript; the search box is an
enhancement that appears only once the script runs, and it matches a
name or any code in any spelling (`ns-1`, `NS 1`, `EW24`).

Each generated page is the same self-contained document that
`mrt-schedule-cli` writes, plus a navigation block that switches the
service date, the line, and the diagram window. Every link is
relative, because a project site lives under `/<repository>/`. The
navigation carries the `no-print` class, so a printed timetable looks
exactly like one generated on its own.

The workflow controls the section through the environment:

| Variable | Meaning | Default |
|----------|---------|---------|
| `MRT_SITE_DAYS` | How many service dates to cover, from today in Singapore time | `3` |
| `MRT_SITE_CONFIG` | A publication configuration file | none |
| `MRT_SITE_TITLE` | The name in the masthead | `Singapore rail timetables` |
| `MRT_SITE_BOARD_HREF` | Relative link back to the board | `../index.html` |
| `MRT_SITE_LINES` | Only these route identifiers, comma separated | all |
| `MRT_SITE_ALLOW_PARTIAL` | `1` accepts a build with failed pages | unset: fail |

A page that cannot be built or written is dropped from every hub —
the site never links to a file that does not exist — and the missing
pages are listed on standard error and under `missing` in
`data/index.json`. By default the
generator then exits non-zero, so an incomplete site cannot deploy
silently. Setting `MRT_SITE_ALLOW_PARTIAL=1` accepts such a partial
site: the hubs still omit the missing pages, only the exit code
changes. The Pages workflow sets it, on the reasoning that one bad
station page should not hold back the whole hourly board deploy —
the next run fills the gap. A build that produced no pages at all
fails regardless.

Raising `MRT_SITE_DAYS` multiplies the page count and the build time:
the section holds `stations x days` timetables and
`lines x days x windows` diagrams. Three days covers today, tomorrow,
and the day after, which answers "now", "tonight", and "the weekend"
for most of the week. Seven covers every service pattern at roughly
twice the size.

If a run gets slow or the artifact gets large, lower `MRT_SITE_DAYS`
first, or narrow `MRT_SITE_LINES`.

## Data files

| File | Content |
|------|---------|
| `data/stations.json` | All stations with their codes. |
| `data/board/<CODE>.json` | Departures for one station: `[posix_seconds, line, destination, exact, trip_id, route_id]` rows for the next 26 hours, with the station's platforms and route identifiers. An interchange has one alias file per code. |
| `data/live.json` | Alerts (legacy and GTFS-Realtime), crowd levels, per-trip delays, and the generation time. The `live-data` branch carries the same shape, refreshed every five minutes. |
| `timetables/data/index.json` | Every station, line, service date, and diagram window that the section covers, with the feed fingerprint. |

The page refetches the station file every 10 minutes and the live
snapshot every 30 seconds.

## Station aliases in the URL

The `station` query parameter takes any code of a station, in any
spelling:

```text
?station=NS1    the station code
?station=ns1    any case
?station=ns-1   any punctuation or spacing
?station=EW24   any other code of the same interchange
```

The page normalizes the parameter the way the library does — lower
case, letters and digits only — and matches it against the codes in
`data/stations.json`. An alias that names no station falls back to
the default station instead of leaving a blank board. Picking a
station from the dropdown writes its first code into the address bar.

Station names are not aliases. The official feed carries names that
two stations share, for example `Bukit Panjang` on the Downtown Line
and on the Bukit Panjang LRT, so a name in a link would open an
arbitrary one of them.

## Times and the freshness lamp

The board shows Singapore time (UTC+8) wherever a visitor opens it:
the panel clock, the snapshot time in the status line, and the
tooltip. The panel clock carries the `SGT` label, and the other times
follow it without repeating it. Wait times need no timezone at all,
because the board files carry POSIX instants and the page subtracts
the visitor's clock.

The status line ends with a lamp and the age of the live data:

| Lamp | Meaning |
|------|---------|
| Green | The last check reached a live source, and the snapshot is current. |
| Amber | The poll is falling behind (no answer for two minutes), the snapshot is more than 15 minutes old, the deployment carries no live layer, or the last snapshot reached no live source (`live: false`). |
| Red | The last check reached no source at all. The board keeps the last snapshot on screen and says how long ago it arrived. |

The tooltip on the status line names the exact fetch time, the source
that answered, and the time the snapshot was built.

## Freshness compared with the server deployment

| Aspect | Server (SRCF) | GitHub Pages |
|--------|---------------|--------------|
| Scheduled departures | live, per request | exact between refreshes |
| Service alerts | at most 20 s old | ~5 min old |
| Crowd levels | at most 20 s old | ~5 min old |
| Trip update delays | per request | ~5 min old, fetched on access |
| Needs a server | yes | no |

Raise the cron frequency in `pages.yml` for fresher live data. Each
run costs a few Actions minutes; a public repository has free
Actions minutes.

## Generate the site locally

```sh
export LTA_DATAMALL_ACCOUNT_KEY=<your key>

# One download, both sections.
cargo run --release -p mrt-schedule-cli -- fetch --source datamall --out feed.zip
cargo run --release -p mrt-board-static -- site feed.zip
MRT_SITE_DAYS=2 MRT_SITE_CONFIG=config/singapore.yaml \
  cargo run --release -p mrt-schedule-site -- site/timetables feed.zip

python3 -m http.server --directory site 8000
```

Then open <http://localhost:8000> for the board and
<http://localhost:8000/timetables/> for the timetables.

Without an account key, pass any local GTFS archive instead of
`feed.zip`. The miniature test feed works for a quick look:

```sh
cd crates/mrt-gtfs/tests/fixtures/mini && zip -q /tmp/mini.zip *.txt && cd -
cargo run -p mrt-schedule-site -- site/timetables /tmp/mini.zip
```

## The map site

The live schematic map is generated by `mrt-map-static` into its own
output directory (`index.html`, `.nojekyll`, `data/map.json`). It
never modifies a board page; the two share nothing but the feed.

The deployment in use builds it as a section of this site: the Pages
workflow runs `cargo run --release -p mrt-map-static -- site/map
feed.zip` after the board and timetable steps, so the map serves at
`/map/` beside them. The step is marked `continue-on-error`, so a
map failure cannot block a board deploy. The page's asset and data
URLs are relative, so the same output also works as its own site: to
give the map its own subdomain or repository instead, publish the
generated directory to a separate Pages site with the same command.

The bundled `data/map.json` is only as fresh as the last rebuild, so
a Pages deployment is a schedule animation with a delay hint, and the
page says so; for second-level freshness, run `mrt-map-web` on a
server instead, or set `MRT_MAP_SNAPSHOT_URL` at build time to a
snapshot that a faster workflow refreshes.
