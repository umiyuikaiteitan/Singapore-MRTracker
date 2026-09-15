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

The Pages workflow runs `python3 scripts/build-fantasy-map.py site --singapore-gtfs feed.zip` after
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

The static editor supports drawing/editing, branches, interlines, radius
checks, local save/restore, GeoJSON import/export, and SVG export.

**OSM tiles** is the fast default renderer. **Overpass** mode draws only
rail lines, administrative borders, and terrain outlines (including
coastlines and multipolygon members) on a dark background. The Layers
selector remembers your choice. Both modes fetch Overpass rail alignments;
tile mode does not request border/terrain geometry. No provider account or
API key is needed. The OSM tile layer uses normal HTTP caching and referrer
headers without offline/prefetch features; see the [tile usage policy](https://operations.osmfoundation.org/policies/tiles/).

The **Follow rails** option runs locally for Mainline, Metro, Tram, and
Monorail/funicular. It follows connected compatible OSM tracks, including
mapped disused/abandoned alignments, with endpoints within 500 m of a track.
Disconnected tracks fail without inventing links. Ferry matching still
needs the optional API. Configured API deployments keep their existing
rail matching service.

The public Overpass endpoint is `https://overpass-api.de/api/interpreter`.
Queries are bounded to zoom 12 or closer and a viewport at most 0.45° by
0.65°. Panning is debounced; stale requests are cancelled; responses and
geometry are capped. Map and matching clients share request pacing and busy
cooldowns. Errors retain previous context and expose Retry. SVGs include
rail vectors in tile mode and rails/borders/terrain in Overpass mode;
raster tiles are omitted. Project export still works during an outage.

## Start with a GTFS rail network

Click **Start with Singapore MRT** to add the official LTA train network.
The Pages workflow passes its existing `feed.zip` to the editor builder,
which packages only routes, trips, stops, stop_times, and optional shapes
as `fantasy-map/singapore-mrt.zip`. The configured URL is relative, so the
button works on the custom domain and project Pages paths without a CORS
request or visitor API key. Each build validates this actual starter through
the shipped importer before deployment. It appends lines with one Undo step.
For a local build, pass `--singapore-gtfs /path/to/feed.zip` to the wrapper.

Other feeds work through **Import GTFS ZIP** or a public HTTPS feed URL. URL downloads need the feed host to allow CORS; otherwise download
the ZIP and upload it. Parsing happens in a browser worker. The importer
reads routes, trips, stops, stop_times, and optional shapes. It preserves
rail route names/colours, shapes, and named stations, collapsing repeated
trips into route/shape/stop patterns. Separate directions/branches may remain
separate editable lines. Schedules and realtime data are not imported.

Imports append to existing lines in one undoable step. Missing shapes use
straight connections between stops with a visible notice. Limits are
32 MiB zipped, 64 MiB expanded tables, 250 rail patterns, 10,000 points per
shape, 100,000 points total, and 10,000 stops across patterns. Use a regional
feed for large systems. Stored/deflated ZIP entries are supported; encrypted
and ZIP64 archives are not.

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
the editor uses it for rail/ferry matching requests. Road following uses
cached Overpass data directly. Basemap, overlay, and SVG context
requests go directly to Overpass. CORS must
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

## Road following and cached geometry

Choose **Manual**, **Follow rails**, or **Follow roads** before drawing.
Manual remains the initial default; the browser remembers your chosen option.
Road following is available for Mainline, Metro, Tram, BRT, Monorail, and
Cycleway. It fetches bounded road geometry directly from Overpass on demand,
including OSM node identities so bridges do not join roads below them just
because their coordinates cross. It does not add roads to the vector basemap.

Road queries are serialized and share public-service pacing/cooldown with
rail and map requests. A four-viewport in-memory cache lasts five minutes and
reuses containing query results for nearby segments. Responses remain bounded
by the existing byte/feature/point limits. Private/no-access ways are excluded;
cycling also excludes motorways and roads tagged bicycle=no. The result is a
bidirectional alignment for designing lines, not traffic directions: oneway
and turn restrictions are not applied. Missing/disconnected alignments fail
without changing the manual control points.


### OSM reconstruction for the Singapore starter

Before packaging the default starter, Pages reconstructs missing GTFS shapes
from active OpenStreetMap tracks downloaded through Overpass. It matches the
ordered stops to connected OSM node topology, uses matching route relations to
restrict corridors when available, and preserves supplied GTFS shapes. It never
joins disconnected OSM components. Each failed pattern stays shapeless and is
listed in the published `singapore-mrt-provenance.json`; the importer discloses
stop-to-stop fallbacks. Publication fails if any visible patterns
are still missing shapes, or if the shipped importer rejects the result.

A bounded 32 MiB Singapore query runs at most once per ISO week in hourly Pages
builds. The Actions cache records successful or failed refresh attempts with the
week, avoiding repeated retries of an immutable cache key. An outage may reuse a
snapshot under 30 days old; provenance reports its age and failed refresh.

The downloadable shapes are attributed to OpenStreetMap contributors with an
ODbL notice at `singapore-mrt-NOTICE.txt`. Original LTA data is retained. The
editor deduplicates opposite directions and contiguous short-turn copies within
each route, while retaining distinct branches and non-equivalent loops.

OSM reconstruction considers nearby parallel tracks across the complete stop
sequence, with a separate distance-bounded graph search for each candidate.
Generated shape and stop distance markers keep loops on the correct pass.
Short-turn route IDs with matching agency, line names, mode and color are
deduplicated together; physical branches remain separate.
