# MRTracker UI draft

This directory contains the dependency-free GitHub Pages interface for
the `mrt-live` view models. The checked-in preview data lets
contributors review the information design without an LTA DataMall
account key. When the Pages generator supplies `stations.json`,
per-station board files, and `live.json`, the interface replaces the
preview rows with generated schedule and live data.

## Preview

Run a static file server from the repository root:

```sh
python3 -m http.server 8080
```

Then open <http://localhost:8080/ui/>.

Do not open `index.html` with a `file://` URL. Browsers block the JSON
request in that mode.

## Data contract

`data/demo.json` mirrors the concepts that the Rust workspace already
exports:

- `network_status` maps to `mrt_live::NetworkStatus`.
- Each station `board` maps to `mrt_live::LiveBoard.rows`.
- `crowd` uses `mrt_datamall::CrowdLevel` values.
- Station coordinates and line paths are presentation data. They do
  not come from the deprecated arrival API.

The `mrt-board-static` crate writes the schedule and live layers that
the interface consumes on GitHub Pages. Keep fetching and secrets
outside the browser.

## GitHub Pages

`mrt-board-static` copies this interface to the site root. It preserves
the earlier dot-matrix interface at `board.html`. The Pages workflow
generates the data on `main`; pull-request branches do not deploy.

## Visual references

The interface uses the dark passenger-information-display language in
the supplied reference image. It also uses the LTA Identity typeface
from the related `MRT-RATIS` repository. The old arrival-fetching code
from that project is not copied or called.
