# Deploy the board on GitHub Pages

GitHub Pages serves static files only. The board therefore runs in a
different shape than the server deployment: a scheduled GitHub
Actions workflow generates the site, and the browser computes the
wait times from the visitor's clock. Train schedules are
deterministic, so the board stays accurate between refreshes.

```text
GitHub Actions "pages" (twice per hour)
  └─ mrt-board-static + account key (Actions secret)
       ├─ downloads the GTFS Schedule feed
       ├─ computes departures per station -> data/board/<CODE>.json
       ├─ reads alerts, crowd, trip updates -> data/live.json
       └─ deploys everything to GitHub Pages

GitHub Actions "rt-refresh" (every five minutes)
  └─ mrt-rt-snapshot + account key
       └─ force-pushes live.json to the live-data branch
          (alerts, crowd levels, per-trip delays and cancellations)

browser
  └─ index.html reads the JSON files and renders the dot-matrix
     board; wait = departure instant + live delay - Date.now().
     The page fetches the live-data snapshot on every visit and
     every 30 seconds. A delayed trip shows a red time; a canceled
     trip shows CANC.
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

## Data files

| File | Content |
|------|---------|
| `data/stations.json` | All stations with their codes and their slugs. |
| `data/aliases.json` | The URL alias table: a normalized alias to the station code that names its board file. |
| `data/board/<CODE>.json` | Departures for one station: `[posix_seconds, line, destination, exact, trip_id]` rows for the next 26 hours. An interchange has one alias file per code. |
| `data/live.json` | Alerts, crowd levels, per-trip delays, and the generation time. The `live-data` branch carries the same shape, refreshed every five minutes. |

The page refetches the station file every 10 minutes and the live
snapshot every 30 seconds.

## Station aliases in the URL

The `station` query parameter takes any alias of a station, so a link
can read the way a person would say it:

```text
?station=NS1           the station code
?station=ns-1          any spelling of the code
?station=EW24          any other code of the same interchange
?station=jurong-east   the station name as a slug
?station=Jurong+East   the station name as it appears on the board
```

The generator writes `data/aliases.json`, and the page resolves the
parameter against that table. The keys are normalized the same way on
both sides: lower case, letters and digits only. An alias that names
no station falls back to the default station. Picking a station from
the dropdown writes the readable slug into the address bar.

## Times and the freshness lamp

The board shows Singapore time (UTC+8) wherever a visitor opens it:
the panel clock, the snapshot time in the status line, and the
tooltip all read SGT. Wait times need no timezone at all, because the
board files carry POSIX instants and the page subtracts the visitor's
clock.

The status line ends with a lamp and the age of the live data:

| Lamp | Meaning |
|------|---------|
| Green | The last check reached a live source, and the snapshot is current. |
| Amber | The poll is falling behind (no answer for two minutes), the snapshot is more than 15 minutes old, or the deployment carries no live layer. |
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
cargo run --release -p mrt-board-static -- site
python3 -m http.server --directory site 8000
```

Then open <http://localhost:8000>.
