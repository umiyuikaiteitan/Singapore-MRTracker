# OpenFantasyMap on umiyui.dev

The existing Pages workflow now packages a third section:

| Location | Application |
| --- | --- |
| `/` | Existing MRTracker departure board |
| `/timetables/` | Existing timetables and train diagrams |
| `/fantasy-map/` | OpenFantasyMap editor, initially centered on Singapore |

The board links to the editor, and the editor links back. Relative assets
and links also work at the default project URL:
`https://umiyuikaiteitan.github.io/Singapore-MRTracker/fantasy-map/`.

## Build and deploy

The Pages workflow runs `python3 scripts/build-fantasy-map.py site` after
the schedule generators. This adds `site/fantasy-map/` without changing
the generated board or timetable files. The same Pages artifact contains
all standard sections, so scheduled rebuilds retain the editor. An optional
`/map/` live schematic is built separately when `MRT_MAP_LAYOUT` names a
reviewed real-network layout.

For an editor-only local preview (no DataMall key or Rust required):

```sh
python3 scripts/build-fantasy-map.py preview
python3 -m http.server --directory preview 8000
```

Open `http://localhost:8000/fantasy-map/`. The back link needs a board at
`preview/index.html`; a full Pages build supplies it. The output
`preview/fantasy-map` must not already exist.

To publish on the requested domain, use **this repository's** Settings →
Pages → Custom domain: `umiyui.dev`. Point the domain's DNS at GitHub Pages
and enable HTTPS after verification. For an Actions-based Pages site, adding
an artifact `CNAME` file alone does not configure the custom domain.
Follow [GitHub's custom-domain instructions](https://docs.github.com/en/pages/configuring-a-custom-domain-for-your-github-pages-site/managing-a-custom-domain-for-your-github-pages-site).
This integration does not change domain settings or DNS. Confirm the board,
timetables, and `/fantasy-map/` after the Pages deployment completes.

## What works without an API

Drawing and editing lines/stations, branches and interlines, radius checks,
browser-local save/restore, GeoJSON import/export, and project-only SVG
export work in the static edition. External basemap tiles need network
access. The page explicitly disables street/corridor matching and OSM
overlays when no API is configured. Imported matched geometry is retained;
editing it in static mode does not re-query the matching service.

Projects remain in browser storage. Export GeoJSON for a portable backup.
A project saved on the github.io origin will not automatically appear on
the umiyui.dev origin. The editor is separate from the live schematic in `crates/mrt-map-web`
and `crates/mrt-map-static`; that optional `/map/` section needs a reviewed
real-network layout. See [DEPLOY-PAGES.md](DEPLOY-PAGES.md#the-map-site).

## Optional matching service

Deploy `OpenFantasyMap/main` to an HTTPS Python/container host using its
Dockerfile or existing server instructions. Its health endpoint is
`GET /api/health`. Set its environment variable:

```text
OFM_ALLOWED_ORIGINS=https://umiyui.dev,https://umiyuikaiteitan.github.io
```

In **this repository**, add an Actions **variable** named `OFM_API_BASE`
with that service's complete HTTPS API base, including `/api/`. Then rerun
the Pages workflow. For example, a service at `https://map-api.example.com`
uses `https://map-api.example.com/api/`. This is a public URL, not a secret;
no API keys belong in it. Leave the variable unset for the static edition.

The Pages build validates this URL, writes it to `static/config.js`, and
the editor uses it for every matching/overlay/export request. CORS must
permit the frontend origin for JSON POST preflights. Existing same-origin
OpenFantasyMap deployments keep working without this setting. Hosting
provider credentials and DNS changes are outside the repository build.

## Update the pinned editor

The source repository is private. Only the browser assets and its standard
library static builder are included here; Actions never needs a token to
read the private repository. `web/fantasy-map/UPSTREAM.json` records the exact
source commit and SHA-256 of every copied file. Make editor changes upstream,
then refresh this snapshot from a local authenticated clone:

```sh
python3 scripts/sync-fantasy-map.py ../OpenFantasyMap --revision <full-commit-sha>
python3 -m unittest discover -s tests -p 'test_fantasy_map.py'
```

The updater reads immutable git blobs, ignoring uncommitted checkout edits,
and replaces only `web/fantasy-map`. Review and commit that directory together
with any integration changes. Do not point the deployment at a floating
private branch or add a cross-repository secret for each hourly rebuild.

The upstream main branch preserves the history from
`claude/transit-line-mapping-gis-dxdcrd` and its descendant
`claude/fantasy-map-mrtracker-roadmaps-kvjvks`.
