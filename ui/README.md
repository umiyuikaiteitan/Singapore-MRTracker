# MRTracker UI draft

This directory contains a dependency-free web prototype for the
`mrt-live` view models. It does not call an API. The checked-in preview
data lets contributors review the information design without an LTA
DataMall account key.

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

The next implementation step is a small adapter that writes the
serialized Rust view models into this shape. Keep fetching and secrets
outside the browser.

## Visual references

The interface uses the dark passenger-information-display language in
the supplied reference image. It also uses the LTA Identity typeface
from the related `MRT-RATIS` repository. The old arrival-fetching code
from that project is not copied or called.
