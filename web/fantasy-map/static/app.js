/**
 * OpenFantasyMap editor.
 *
 * State lives in `state.lines`; displayed geometry is derived on every
 * render via OFMGeometry.buildRouteGeometry, so manual nodes stay the
 * single source of truth. Stations are stored as an arc-length fraction
 * along their line and therefore follow every route edit.
 *
 * This file is only the entry point. The editor lives in the modules
 * under `static/modules/`, each named for one of the seams this file
 * used to mark with a comment rule; they wire their own map and DOM
 * handlers as they load, and this file boots what they set up.
 */
import { refreshBasemap } from "./modules/basemap.js";
import { apiEnabled, config } from "./modules/config.js";
import { state, load } from "./modules/state.js";
import { render } from "./modules/render.js";
import {
  cyclewayToggle,
  updateToolButtons,
  updateRadiusControl,
} from "./modules/controls.js";
// Imported for their map and document handlers, which they register as
// they load; nothing here calls into them directly.
import "./modules/interactions.js";
import "./modules/overlay.js";
import "./modules/gtfs-import.js";

// ------------------------------------------------------------ boot

load();
if (!apiEnabled) state.snapMode = "manual";
document.getElementById("hosting-notice").hidden = apiEnabled;
const boardLink = document.getElementById("board-link");
if (config.boardHref) {
  boardLink.href = config.boardHref;
  boardLink.hidden = false;
}
cyclewayToggle.checked = state.showCycleways;
updateToolButtons();
updateRadiusControl();
render();

refreshBasemap();
