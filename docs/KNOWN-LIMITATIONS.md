# Known limitations

This document lists what the timetable and diagram generator does not
do, and why. Everything here is a deliberate boundary, not an oversight
waiting for a bug report.

## What these outputs are not

**A published timetable, not a dispatching system.** GTFS describes
passenger-facing routes, trips, stops, service dates, and stop times.
It carries no block occupancy, no signalling state, no rolling-stock
diagram, and no authoritative platform conflict. A page from this
generator says what the operator published, and nothing about whether
two trains can actually be where the drawing puts them.

**Not live.** The static outputs deliberately ignore GTFS-Realtime.
A printed page cannot follow a delay, and folding live data into one
would make it wrong the moment it left the printer. Delays,
cancellations, and alerts belong to `mrt-live` and the board.

**Not the whole network on one axis.** A diagram draws one corridor.
Putting an entire metro network on a single station axis produces a
drawing that no one can read and that implies adjacencies which do not
exist.

## Data that the generator refuses to invent

| Missing thing | What happens instead |
|---------------|----------------------|
| A train number | Nothing is printed. `trips.trip_id` is an internal key and never appears as a train number, however tempting the position on the page. |
| A platform | The platform label is omitted. It is never derived from the direction of travel. |
| A direction name | The panel is headed by the destinations it really carries, and falls back to `Direction 0`. "Northbound", 上り, and 下り come only from `labels.direction_overrides`. |
| A departure time inside a non-exact headway block | A band, `06:30–09:00  every 4 min approximately`. Minute entries appear only under `frequency_policy: expand-approximate`, and then every one carries the approximation mark. |
| A stop time in the middle of a trip | Interpolation between two known times, marked as computed. |
| A stop time before the first or after the last known one | It stays missing, and the run is drawn only in part. `missing_time_policy: interpolate-unbounded` opts in to extrapolation. |
| A station position | Distance spacing falls back to equal spacing, with a diagnostic. |

## Structural limits

**One service date per document.** A timetable covers one GTFS service
day, which by default runs `04:00` to `28:00`. A trip that started on
the *previous* service day and calls after midnight belongs to that
day's page, not to this one. This is the Japanese 発車時刻表
convention and it keeps "one page, one service pattern" true. A
station board that answers "what is next, right now" is a different
product: `mrt-board-static` already does it.

**A loop is unrolled once.** A run that goes round the loop more often
than the corridor was unrolled for cannot be drawn on that axis, and
is left out with a `run-off-corridor` diagnostic. Extend the corridor
axis if a feed really carries such a run.

**Branches need a corridor for a shared axis.** Automatic derivation
joins a pattern to the spine only when it is a subsequence of it,
forwards or backwards. Anything else gets its own panel and a
`corridor-split` warning. This is the explicit behaviour the design
calls for; it is not a failure.

**`block_id` is retained but unused.** Consecutive trips of one
vehicle are not yet joined into a single diagram run.

**Distance spacing uses station positions, not the shape.** Cumulative
`shape_dist_traveled` weights *time interpolation*, but the vertical
axis measures the great-circle distance between station positions. On
a line with long curves the two differ slightly.

## Rendering limits

**Inline styles and scripts.** The pages carry
`default-src 'none'`, which blocks every network request, but the
stylesheet and the script are inline and therefore need
`'unsafe-inline'`. A hash-based policy would be stricter; it would
also break the whole page on a one-byte mismatch. Since every value
from the feed is escaped before it reaches the markup, and the tests
prove it, the network-blocking part of the policy is the part that
earns its place.

**PNG and PDF are not produced.** The generator writes HTML and SVG.
Print from a browser, or add a headless-browser adapter.

**No server.** The command line writes files. A future HTTP server
should call the same library functions rather than reimplement the
schedule logic.

**Fonts are named, not embedded.** The pages name a font stack that
ends in a generic family, so a page always renders. A reader without
Noto Sans JP sees Japanese text in whatever their system provides.
Embedding a font would either bloat every page or borrow a licence
this project does not hold.

**Label placement is greedy.** Run labels take the longest free travel
segment first and hide when nothing fits. A denser corridor hides more
labels; the page reveals them on hover and on keyboard focus, and the
call tables list every run in full.

## The Singapore profile

`docs/SINGAPORE-GTFS-PROFILE.md` marks every unverified assumption
about the real LTA feed. The profile was written without an account
key, so the fields it lists as unverified have not been checked
against a download. Nothing in the pipeline depends on an unverified
value: it reads what the feed says, falls back to a documented
default, or emits a diagnostic.

## Testing limits

**Visual regression needs a browser.** `cargo test` checks the visual
*grammar* of both pages and pins the SVG byte for byte.
`scripts/visual-regression.sh` adds the pixel comparison, and stays
out of `cargo test` because it needs Chromium. The committed baselines
in `examples/baseline/` were rendered on one machine; font
availability changes text rendering, so regenerate them with
`--update` on the machine you compare on.

**The fixture is miniature.** `crates/mrt-gtfs/tests/fixtures/mini`
carries one of everything that matters — two platforms per station, a
short turn, an exact headway block, a non-exact one, missing
intermediate times, a pass-through call, an incompatible branch, a
loop, a trip past midnight, calendar exceptions — but it is not the
real network. Run `validate --strict` against a real feed before
trusting a claim about it.
