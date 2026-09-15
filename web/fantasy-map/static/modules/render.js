/** Drawing the network: routes, nodes, stations, the polygon draft, and the line list. */

import { G } from "./geom.js";
import { MODE_NAMES, modeRules, lineRadius } from "./modes.js";
import {
  labelSide, labelHidden, nextLabelSide, setStationLabel, stationLabelText,
} from "./model.js";
import {
  state, activeLine, lineGeometry, lineIssues, invalidate, selectedNodeIndexes,
  clearSelection, selectNode, pushHistory, afterGeometryChange, save,
} from "./state.js";
import {
  map, routeLayer, issueLayer, nodeLayer, stationLayer, draftLayer,
} from "./map-setup.js";
import { setStatus, tooltipContent } from "./ui.js";
import { rematchAdjacentSegments } from "./snapping.js";
import { syncBranches, interlineSlot } from "./topology.js";
import { menuElement, showMenu, hideMenu, nodeMenuItems } from "./context-menu.js";
import { clickSwallowed, onRouteClick, segmentMenuItems } from "./interactions.js";
import { updateToolButtons, updateRadiusControl } from "./controls.js";

// ----------------------------------------------------------- render

export const lineVisible = (line) =>
  line.visible && (line.mode !== "Cycleway" || state.showCycleways);

/** Base stroke style for a line's mode (before interline dashes). */
export function lineStrokeStyle(line) {
  const rules = modeRules(line.mode);
  return {
    weight: rules.weight || 4.5,
    dashArray: rules.dash || null,
  };
}

export function render() {
  syncBranches();
  routeLayer.clearLayers();
  issueLayer.clearLayers();
  nodeLayer.clearLayers();
  stationLayer.clearLayers();

  for (const line of state.lines) {
    if (!lineVisible(line)) continue;
    const geometry = lineGeometry(line);
    const baseStyle = lineStrokeStyle(line);
    geometry.segmentPoints.forEach((points, index) => {
      if (points.length < 2) return;
      const latlngs = points.map(([lat, lng]) => [lat, lng]);
      L.polyline(latlngs, {
        color: "#050a13",
        weight: baseStyle.weight * 2,
        opacity: 0.9,
        interactive: false,
      }).addTo(routeLayer);
      const slot = interlineSlot(line, line.segments[index]);
      const style = {
        color: line.color,
        weight: baseStyle.weight,
        dashArray: baseStyle.dashArray,
        opacity: 1,
      };
      if (slot) {
        style.dashArray = `12 ${12 * (slot.count - 1)}`;
        style.dashOffset = `${12 * slot.index}`;
      }
      const polyline = L.polyline(latlngs, style).addTo(routeLayer);
      polyline.on("click", (event) => {
        L.DomEvent.stop(event);
        if (!menuElement.hidden) {
          hideMenu();
          return;
        }
        if (clickSwallowed()) return;
        onRouteClick(line, event.latlng);
      });
      polyline.on("contextmenu", (event) => {
        selectLine(line.id);
        showMenu(event, segmentMenuItems(line, index, event.latlng));
      });
    });
    renderStations(line, geometry);
  }
  renderIssues();
  renderNodes();
  renderDraft();
  renderLineList();
  updateToolButtons();
  setStatus();
  save();
}

// Leaflet path options take colour values, not CSS tokens: these two
// mirror `--state-error` and `--station-fill` in `static/style.css`.
const ISSUE_COLOUR = "#d64545";
const ISSUE_FILL = "#09101b";

/**
 * Curvature issues on the active line: the offending arc, and a marker
 * at its tightest vertex reading the measured radius against the line's
 * minimum. Only the active line is drawn, so the map never shows more
 * than one line-list chip accounts for. The markers carry a tooltip and
 * nothing else — clicks bubble to the map, so drawing over one works.
 */
function renderIssues() {
  const line = activeLine();
  if (!line || !lineVisible(line)) return;
  const minimum = lineRadius(line);
  for (const issue of lineIssues(line)) {
    L.polyline(issue.arc, {
      color: ISSUE_COLOUR,
      weight: 2,
      dashArray: "5 6",
      interactive: false,
    }).addTo(issueLayer);
    L.circleMarker(issue.coordinate, {
      radius: 4,
      color: ISSUE_COLOUR,
      weight: 2,
      fillColor: ISSUE_FILL,
      fillOpacity: 1,
    })
      .bindTooltip(
        tooltipContent(
          `Radius ${Math.round(issue.radiusMeters)} m · minimum ${minimum} m`,
        ),
        { direction: "top", className: "issue-tooltip" },
      )
      .addTo(issueLayer);
  }
}

/** Index of the node that drawing currently extends from. */
function extensionIndex(line) {
  return state.extendFrom === "start" ? 0 : line.nodes.length - 1;
}

function renderNodes() {
  const line = activeLine();
  if (!line || !lineVisible(line)) return;
  line.nodes.forEach((node, index) => {
    const isEndpoint = index === 0 || index === line.nodes.length - 1;
    const isBranchAnchor =
      line.branchOf && line.branchOf.branchNodeIndex === index;
    const isSelected =
      state.selected &&
      state.selected.kind === "node" &&
      state.selected.lineId === line.id &&
      state.selected.index === index;
    const classes = ["route-node"];
    if (isEndpoint) classes.push("endpoint");
    if (isBranchAnchor) classes.push("branch-anchor");
    if (state.selectedNodes.includes(index)) classes.push("multi");
    if (isSelected) classes.push("selected");
    if (
      state.tool === "draw" &&
      line.nodes.length > 1 &&
      index === extensionIndex(line)
    ) {
      classes.push("extension");
    }
    const marker = L.marker(node, {
      draggable: !isBranchAnchor,
      icon: L.divIcon({ className: classes.join(" ") }),
    }).addTo(nodeLayer);
    marker.on("click", (event) => {
      L.DomEvent.stop(event);
      const original = event.originalEvent;
      const additive = original.ctrlKey || original.metaKey;
      const range = original.shiftKey;
      // In draw mode, plain-clicking the opposite endpoint flips which
      // end new nodes extend from; modifier clicks still select.
      if (
        state.tool === "draw" &&
        isEndpoint &&
        !isBranchAnchor &&
        !additive &&
        !range
      ) {
        state.extendFrom = index === 0 ? "start" : "end";
        render();
        return;
      }
      selectNode(
        line.id,
        index,
        additive ? "toggle" : range ? "range" : "replace",
      );
      render();
    });
    marker.on("contextmenu", (event) => {
      // Keep an existing multi-selection if this node is part of it.
      if (!state.selectedNodes.includes(index)) {
        selectNode(line.id, index, "replace");
      } else {
        state.selected = { kind: "node", lineId: line.id, index };
      }
      render();
      showMenu(event, nodeMenuItems(line, index));
    });
    // Dragging one node of a multi-selection moves the whole group.
    let dragGroup = null;
    marker.on("dragstart", () => {
      pushHistory();
      const group = state.selectedNodes.includes(index)
        ? selectedNodeIndexes().filter(
            (item) =>
              !(line.branchOf && line.branchOf.branchNodeIndex === item),
          )
        : [index];
      dragGroup = {
        origin: [...line.nodes[index]],
        members: group.map((item) => ({
          index: item,
          start: [...line.nodes[item]],
        })),
      };
    });
    marker.on("drag", (event) => {
      const { lat, lng } = event.target.getLatLng();
      if (dragGroup && dragGroup.members.length > 1) {
        const dLat = lat - dragGroup.origin[0];
        const dLng = lng - dragGroup.origin[1];
        for (const member of dragGroup.members) {
          line.nodes[member.index] = [
            member.start[0] + dLat,
            member.start[1] + dLng,
          ];
        }
      } else {
        line.nodes[index] = [lat, lng];
      }
      invalidate(line.id);
      redrawRoutesOnly();
    });
    marker.on("dragend", () => {
      const moved = dragGroup
        ? dragGroup.members.map((member) => member.index)
        : [index];
      dragGroup = null;
      // Manual nodes moved by hand: re-match adjacent corridor segments
      // between the (unchanged) new endpoints.
      afterGeometryChange(line);
      rematchAdjacentSegments(line, moved);
    });
  });
}

/**
 * Cheap redraw of route polylines while a node is being dragged. The
 * curvature check is not run here: it would rerun on every pointer move
 * against geometry the drag is still changing. `dragend` re-renders.
 */
function redrawRoutesOnly() {
  routeLayer.clearLayers();
  issueLayer.clearLayers();
  stationLayer.clearLayers();
  for (const line of state.lines) {
    if (!lineVisible(line)) continue;
    const geometry = lineGeometry(line);
    const baseStyle = lineStrokeStyle(line);
    geometry.segmentPoints.forEach((points) => {
      if (points.length < 2) return;
      L.polyline(points, {
        color: line.color,
        weight: baseStyle.weight,
        dashArray: baseStyle.dashArray,
        interactive: false,
      }).addTo(routeLayer);
    });
    renderStations(line, geometry);
  }
}

function renderStations(line, geometry) {
  for (const station of line.stations) {
    const anchor = G.pointAtFraction(geometry.points, station.t);
    if (!anchor) continue;
    // `coordinate` is the committed physical anchor used for
    // re-projection after route edits — only initialise it here.
    if (!station.coordinate) station.coordinate = anchor;
    const isSelected =
      state.selected &&
      state.selected.kind === "station" &&
      state.selected.stationId === station.id;
    if (station.polygon && station.polygon.length >= 3) {
      const ring = station.polygon.map((offset) =>
        G.offsetToCoordinate(anchor, offset),
      );
      L.polygon(ring, {
        color: line.color,
        weight: 2,
        fillColor: line.color,
        fillOpacity: 0.22,
        interactive: false,
      }).addTo(stationLayer);
    }
    const marker = L.marker(anchor, {
      draggable: state.tool === "select",
      icon: L.divIcon({
        className: `station-marker${isSelected ? " selected" : ""}`,
      }),
      title: stationLabelText(station),
    }).addTo(stationLayer);
    marker.getElement()?.style.setProperty("color", line.color);
    // Placement follows the station's own label field; a hidden label
    // binds no tooltip at all, leaving the marker's hover title.
    if (!labelHidden(station)) {
      marker.bindTooltip(tooltipContent(stationLabelText(station)), {
        permanent: false,
        direction: labelSide(station),
        className: "station-label",
      });
    }
    marker.on("click", (event) => {
      L.DomEvent.stop(event);
      state.selectedNodes = [];
      state.selected = { kind: "station", lineId: line.id, stationId: station.id };
      render();
    });
    marker.on("contextmenu", (event) => {
      state.selectedNodes = [];
      state.selected = { kind: "station", lineId: line.id, stationId: station.id };
      render();
      showMenu(event, [
        {
          label: "Rename station",
          action: () => {
            const name = prompt("Station name", station.name);
            if (name && name.trim()) {
              pushHistory();
              station.name = name.trim();
              render();
            }
          },
        },
        {
          label: "Set station code",
          action: () => {
            const code = prompt("Station code", station.code || "");
            if (code === null) return;
            pushHistory();
            // An empty answer clears the code rather than storing "".
            if (code.trim()) station.code = code.trim();
            else delete station.code;
            render();
          },
        },
        // One cycling item rather than four side items: the menu is a
        // flat list of buttons and four of them would be most of it.
        !labelHidden(station) && {
          label: `Move label to ${nextLabelSide(labelSide(station))}`,
          action: () => {
            pushHistory();
            setStationLabel(station, { side: nextLabelSide(labelSide(station)) });
            render();
          },
        },
        {
          label: labelHidden(station) ? "Show label" : "Hide label",
          action: () => {
            pushHistory();
            setStationLabel(station, { hidden: !labelHidden(station) });
            render();
          },
        },
        {
          label: "Delete station",
          action: () => {
            pushHistory();
            line.stations = line.stations.filter(
              (item) => item.id !== station.id,
            );
            clearSelection();
            render();
          },
        },
      ]);
    });
    // Wrapped: passed directly, Leaflet would hand `pushHistory` the
    // drag event as the snapshot to record.
    marker.on("dragstart", () => pushHistory());
    // Stations only exist on the route: dragging slides them along it.
    marker.on("drag", (event) => {
      const { lat, lng } = event.target.getLatLng();
      const projection = G.projectToPolyline([lat, lng], geometry.points);
      if (projection) {
        station.t = projection.t;
        event.target.setLatLng(projection.coordinate);
      }
    });
    marker.on("dragend", () => {
      // Commit the new physical anchor after an intentional slide.
      station.coordinate = G.pointAtFraction(geometry.points, station.t);
      render();
    });
  }
}

export function renderDraft() {
  draftLayer.clearLayers();
  if (state.tool === "polygon" && state.polygonDraft.length) {
    L.polyline([...state.polygonDraft, state.polygonDraft[0]], {
      color: "#ffd75e",
      weight: 2,
      dashArray: "4 4",
    }).addTo(draftLayer);
    state.polygonDraft.forEach((vertex) =>
      L.circleMarker(vertex, {
        radius: 3,
        color: "#ffd75e",
        fillOpacity: 1,
      }).addTo(draftLayer),
    );
  }
}

/**
 * Curvature indicator for one line-list row: "N issues" or "Valid".
 * Hidden lines are not checked, so the chip only ever reports on what is
 * drawn. A chip with issues selects its line and frames the first one.
 */
function issueChip(line) {
  if (!lineVisible(line)) return document.createDocumentFragment();
  const issues = lineIssues(line);
  const minimum = lineRadius(line);
  const chip = document.createElement("span");
  chip.className = `line-issues${issues.length ? " has-issues" : ""}`;
  chip.textContent = issues.length
    ? `${issues.length} issue${issues.length === 1 ? "" : "s"}`
    : "Valid";
  if (modeRules(line.mode).straight) {
    chip.title = "Straight spans — no curve minimum applies";
  } else if (issues.length) {
    chip.title = `Tighter than the ${minimum} m minimum — click to show`;
  } else {
    chip.title = `Every curve meets the ${minimum} m minimum`;
  }
  if (issues.length) {
    chip.addEventListener("click", (event) => {
      event.stopPropagation();
      selectLine(line.id);
      map.setView(issues[0].coordinate, Math.max(map.getZoom(), 16));
    });
  }
  return chip;
}

function renderLineList() {
  const container = document.getElementById("line-list");
  container.innerHTML = "";
  for (const line of state.lines) {
    const item = document.createElement("div");
    item.className = `line-item${line.id === state.activeLineId ? " active" : ""}`;

    const swatch = document.createElement("input");
    swatch.type = "color";
    swatch.className = "line-swatch";
    swatch.value = line.color;
    swatch.title = "Line colour";
    swatch.addEventListener("input", () => {
      line.color = swatch.value;
      render();
    });

    const name = document.createElement("input");
    name.className = "line-name";
    name.value = line.name;
    name.title = line.branchOf ? "Branch line" : "Line name";
    name.addEventListener("change", () => {
      line.name = name.value.trim() || line.name;
      render();
    });
    name.addEventListener("focus", () => selectLine(line.id));

    const meta = document.createElement("select");
    meta.className = "line-mode";
    meta.title = line.branchOf
      ? "Branch line — transit mode"
      : "Transit mode (sets snapping and curve rules)";
    for (const modeName of MODE_NAMES) {
      const option = document.createElement("option");
      option.value = modeName;
      option.textContent = modeName;
      option.selected = modeName === line.mode;
      meta.append(option);
    }
    meta.addEventListener("click", (event) => event.stopPropagation());
    meta.addEventListener("change", () => {
      pushHistory();
      line.mode = meta.value;
      // A new mode brings its own sensible default curve radius.
      line.minRadius = modeRules(line.mode).radius;
      invalidate(line.id);
      updateRadiusControl();
      render();
    });

    const visibility = document.createElement("button");
    visibility.className = "icon-button";
    visibility.textContent = line.visible ? "👁" : "🚫";
    visibility.title = "Toggle visibility";
    visibility.addEventListener("click", (event) => {
      event.stopPropagation();
      line.visible = !line.visible;
      render();
    });

    const remove = document.createElement("button");
    remove.className = "icon-button";
    remove.textContent = "✕";
    remove.title = "Delete line";
    remove.addEventListener("click", (event) => {
      event.stopPropagation();
      if (!confirm(`Delete ${line.name}?`)) return;
      pushHistory();
      state.lines = state.lines.filter((item2) => item2.id !== line.id);
      state.lines.forEach((other) => {
        if (other.branchOf && other.branchOf.lineId === line.id) {
          other.branchOf = null;
        }
      });
      if (state.activeLineId === line.id) {
        state.activeLineId = state.lines.length ? state.lines[0].id : null;
      }
      clearSelection();
      invalidate();
      render();
    });

    item.append(swatch, name, meta, issueChip(line), visibility, remove);
    item.addEventListener("click", () => selectLine(line.id));
    container.append(item);
  }
}

export function selectLine(id) {
  if (state.activeLineId === id) return;
  state.activeLineId = id;
  clearSelection();
  state.extendFrom = "end";
  updateRadiusControl();
  render();
}
