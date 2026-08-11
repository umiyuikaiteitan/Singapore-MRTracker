# Deploy the board on GitHub Pages

GitHub Pages serves static files only. The board therefore runs in a
different shape than the server deployment: a scheduled GitHub
Actions workflow generates the site, and the browser computes the
wait times from the visitor's clock. Train schedules are
deterministic, so the board stays accurate between refreshes.

```text
GitHub Actions (twice per hour)
  └─ mrt-board-static + account key (Actions secret)
       ├─ downloads the GTFS Schedule feed
       ├─ computes departures per station -> data/board/<CODE>.json
       ├─ reads alerts and crowd levels  -> data/live.json
       └─ deploys the network visualisation and classic board

browser
  ├─ index.html renders the interactive network visualisation
  └─ board.html renders the classic dot-matrix destination board
     wait = departure instant - Date.now()
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

The network visualisation is the landing page. The earlier dot-matrix
board remains available at
`https://<owner>.github.io/<repository>/board.html`.

## How the workflow runs

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
| `data/stations.json` | All stations with their codes. |
| `data/board/<CODE>.json` | Departures for one station: `[posix_seconds, line, destination, exact]` rows for the next 26 hours. An interchange has one alias file per code. |
| `data/live.json` | Alerts, per-station crowd levels, and the generation time. |

The page refetches the station file every 10 minutes and
`data/live.json` every minute.

## Freshness compared with the server deployment

| Aspect | Server (SRCF) | GitHub Pages |
|--------|---------------|--------------|
| Scheduled departures | live, per request | exact between refreshes |
| Service alerts | at most 20 s old | up to ~30 min old |
| Crowd levels | at most 20 s old | up to ~30 min old |
| Trip update delays | per request | not included |
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
