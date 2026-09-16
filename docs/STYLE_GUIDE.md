# Aesthetic style guide: OpenFantasyMap and MRTracker

**Scope:** OpenFantasyMap and the MRTracker pages that host or accompany it.  
**Transferred:** 16 September 2026, at Yui's request.  
**Source:** The aesthetic guide for Yui's personal site, including the later instructions to reduce secondary copy, center and shrink station dots, and remove detached top lines.

This is the shared direction for future interface work. It applies to the standalone OpenFantasyMap editor and to Singapore-MRTracker's board at `/`, editor at `/fantasy-map/`, live map at `/map/`, and timetables and diagrams at `/timetables/`, including their nested pages and shared controls.

The transfer establishes design guidance; it does not claim that existing interfaces have already been restyled. Preserve working tools, data contracts, route geometry, and exports while making future presentation changes. Later explicit user instructions take precedence.

## Design intent

Use the clarity of railway signage, the hierarchy of transport publications, and the precise connections of PCB traces. Network SouthEast is the primary branding reference: confident Helvetica-like type, blue/red/white fields, disciplined alignment, and economical route geometry.

Prefer readable instruments and useful content. Avoid marketing slogans, oversized empty heroes, feature-card grids, floating badges, decorative statistics, and filler copy. No generated imagery, gradients, glass, backdrop blur, grain, simulated paper, decorative glow, parallax, or ornamental animation. Use square-edged panels and restrained controls; rounded bends and station circles serve a purpose.

Do not copy official insignia or imply operator affiliation. The palette below is a web interpretation, not a claim about historical specifications. Preserve OpenFantasyMap and MRTracker as their own product names. WHY04, Yui (including yui), and Cordelia refer to the same person; preserve existing artwork credits without inferring legal names or biographies.

## Typography and colour

Use a configurable local font stack for ordinary interface text:

```css
--font-body: Helvetica, "Helvetica Neue", Arial, sans-serif;
--font-heading: var(--font-body);
--blue: #003d85;
--red: #d52b1e;
--paper: #ffffff;
--ink: #131b26;
--surface: #f1f4f7;
--rule: #cbd3dc;
--focus: #ffdf45;
```

Use one documented token source per application. OpenFantasyMap currently keeps tokens in `static/style.css`; the hosted copy is in MRTracker's `web/fantasy-map/static/style.css`. Do not scatter brand values across components or vendor styles. Keep CSS, map-renderer constants, and SVG export values consistent through an explicit mapping when those renderers cannot consume CSS tokens.

White is the default for ordinary page surfaces; blue anchors navigation and red provides short accents. Use solid fills and distinct stripes. A map basemap or a specialised destination-board display may retain its functional background and display font where needed for legibility and its established rendering grammar. These exceptions do not justify unrelated dark panels or new decorative themes.

Brand colours, route identities, selection, warnings, errors, and freshness states are different roles. Preserve user-authored and imported line colours, official network identities, mode dashes, and status meanings. Do not recolour all routes blue/red or allow a selection indicator to become indistinguishable from the selected route. Pair colour with text, shapes, borders, or patterns.

Use normal and bold weights, sentence case, a small type scale, and tabular numerals for aligned times, distances, radii, coordinates, and counts. Aim for 16–18 px body text and at least 14 px ordinary controls. Do not make operational text tiny to fit a panel. Diagram annotations may follow a deliberate export scale, with readable zoom/print output. Avoid overtracked uppercase microheadings. Keep prose to roughly 60–75 characters per line.

No remote font dependency by default. Preserve necessary licensed display assets; do not add proprietary fonts without an appropriate licence. Maintain visible keyboard focus and sufficient contrast: 4.5:1 for normal text and 3:1 for large text and essential interface boundaries.

## Grid and responsive layout

- Align mastheads, toolbars, panel headings, labels, inputs, rules, and footers to deliberate shared edges.
- Use an 8 px spacing rhythm with 4 px increments for small adjustments.
- Reading pages may use a 1120–1200 px content width, 32–40 px desktop gutters, and about 20 px mobile gutters.
- Map editors and operational displays use the available workspace; do not constrain a useful map canvas to a prose column.
- Collapse or dock panels sensibly on narrow screens. Preserve access to tools, selection details, status, and attribution.
- Long names, codes, URLs, translated labels, and validation messages must wrap or scroll locally without page-level overflow.
- Keep touch targets around 44 px even when the visible marker or icon is smaller. Use text labels for unfamiliar controls.
- Keep controls visually separate from route labels and attribution. A toolbar must not cover the only way to understand or edit a selection.

## Route and station geometry

Use route motifs to connect real destinations or sections. Keep interface traces orthogonal or at 45 degrees with consistent bends and stroke widths. Do not draw detached horizontal title rails or unmatched elbows above content. No ornamental network behind text.

For interface station dots, use a 12–16 px outer diameter (16 px by default), including the outline. Center the dot on the stroke, not beside it. Include pseudo-elements in border-box sizing. For a heading beside a left-border route, derive the center from the actual content gutter plus half the route width and translate by half the marker's own width. Do not use unexplained pixel offsets that drift at mobile breakpoints.

Navigation interchanges may be taller where they genuinely span two routes. Mark the current destination with text/underline and `aria-current`, not colour alone. Keep text clear of all routes.

Map features have geographic meaning: never force OSM alignments, user curves, polygons, or timetable train paths into decorative 45-degree geometry. Map station symbols must remain anchored to the route at all zoom levels; keep their visual size distinct from their hit target. Preserve endpoint, branch, selection, and interline semantics. Casing or a label halo is allowed where it distinguishes data against the basemap; it is not decorative shadow. Ensure SVG exports and on-screen station/label semantics agree.

## Copy, imagery, and data

Carry forward the personal site's preference for fewer words: remove redundant introductions, repeated labels, ornamental metadata, and vague empty-state prose. Ordinary text uses the ink colour rather than a low-contrast grey hierarchy.

Do **not** mechanically remove every currently grey string from these applications. Timetable dates, destinations, platform information, units, estimated-position notices, schedule-only labels, stale-data status, failures, import diagnostics, attribution, and licence notices are functional content. Keep them legible and factual. A concise error or empty state must explain what happened and a useful next action when one exists.

The personal site's ban on decorative map images does not ban these applications' actual maps. Existing maps, tiles, authored routes, train diagrams, and exports are the product. Preserve provider credits and data licences. Do not add stock art, generated maps, invented stations, fake activity, testimonials, or decorative image placeholders. Do not pull personal-site reference attachments into the application automatically.

## Application-specific priorities

| Surface | Apply the shared style to | Preserve |
| --- | --- | --- |
| OpenFantasyMap standalone and `/fantasy-map/` | Toolbar, line list, inspectors, menus, import/export controls, status, navigation | Editing, snapping, route topology, undo, user colours, selection, map attribution, data/export semantics |
| MRTracker board `/` | Page shell, navigation, selectors, supporting controls | Destination-board grammar, times, platform/destination data, live/stale/failure indicators |
| Live map `/map/` | Page shell, legend, selectors, status and detail panels | Train movement semantics, estimated-position labels, route identities, basemap/track context |
| `/timetables/` and nested station/diagram pages | Navigation, filters, headings and shared chrome | Hour/minute hierarchy, direction/platform grouping, annotations, service dates, print scale and train-diagram axes |

The earlier OpenFantasyMap `docs/STYLE.md` remains a record of its existing rendering grammar and implementation. This guide supersedes its conflicting aesthetic defaults (dark chrome, system-ui as an undecided font, grey uppercase section labels). Preserve the functional semantics documented there while moving ordinary interface chrome toward this shared direction.

## Maintenance across repositories

Keep this document identical at `docs/STYLE_GUIDE.md` in OpenFantasyMap and Singapore-MRTracker. Apply future shared-guide changes to both repositories and record intentional application-specific exceptions here.

MRTracker's root `AGENTS.md` applies to all page sources, including Rust-generated pages. `web/AGENTS.md` reinforces it for the editor and live-map subtrees. Do not put the only copy of agent guidance inside `web/fantasy-map/`: `scripts/sync-fantasy-map.py` replaces that directory when refreshing the pinned editor.

Make editor implementation changes in OpenFantasyMap, then refresh MRTracker's browser snapshot through the documented immutable-revision procedure. Preserve `UPSTREAM.json` hashes and existing integration additions. A documentation transfer alone does not require refreshing browser assets, changing the pinned revision, altering deployment configuration, or rewriting either backend.

## Checks before shipping a visual change

- Read this guide and the relevant application design/data documentation.
- Verify shared edges, readable labels, centered stations, and the absence of detached decorative rails.
- Check narrow screens, long content, keyboard navigation, visible focus, and 200% zoom for the changed interface.
- Verify status and selection remain understandable without colour alone.
- For renderer changes, check both on-screen output and SVG/print output; preserve hit targets and map anchors.
- Preserve attribution, freshness/estimate notices, diagnostics, user data, and real operational content.
- Run checks relevant to changed behaviour. Do not claim browser or visual testing when only source checks ran.
- Keep the shared guide and hosted-editor update documentation discoverable for future agents.
