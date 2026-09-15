/** The right-click menu element itself, and the items offered over a node. */

import {
  state, selectedNodeIndexes, selectNode, pushHistory, afterGeometryChange,
} from "./state.js";
import {
  nodeRadius, hasNodeRadiusOverride, setNodeRadius,
} from "./model.js";
import { modeRules } from "./modes.js";
import { toast } from "./ui.js";
import { map } from "./map-setup.js";
import { selectLine } from "./render.js";
import { branchFromSelection, splitLineAt } from "./topology.js";
import { deleteSelectedNodes, trimLineEnd, trimmableNodeCount } from "./interactions.js";
import { setTool } from "./controls.js";

// ------------------------------------------------------ context menu

export const menuElement = document.createElement("div");
menuElement.id = "context-menu";
menuElement.hidden = true;
document.body.append(menuElement);

/** Show a context menu at a Leaflet event's screen position. */
export function showMenu(event, items) {
  L.DomEvent.stop(event);
  event.originalEvent.preventDefault();
  menuElement.innerHTML = "";
  for (const item of items.filter(Boolean)) {
    const button = document.createElement("button");
    button.textContent = item.label;
    button.addEventListener("click", () => {
      hideMenu();
      item.action();
    });
    menuElement.append(button);
  }
  menuElement.hidden = false;
  const { clientX, clientY } = event.originalEvent;
  const rect = menuElement.getBoundingClientRect();
  menuElement.style.left =
    Math.min(clientX, window.innerWidth - rect.width - 8) + "px";
  menuElement.style.top =
    Math.min(clientY, window.innerHeight - rect.height - 8) + "px";
}

export function hideMenu() {
  menuElement.hidden = true;
}

document.addEventListener("click", hideMenu);
map.on("movestart zoomstart", hideMenu);

/** Nodes affected by a node-menu command, preserving a multi-selection. */
function menuNodeIndexes(line, index) {
  const selected = selectedNodeIndexes();
  return selected.length > 1 && selected.includes(index) ? selected : [index];
}

/** Apply a radius override after the prompt has produced a valid change. */
function setSelectedNodeRadius(line, indexes) {
  const initial = indexes.every(
    (index) => nodeRadius(line, index) === nodeRadius(line, indexes[0]),
  )
    ? nodeRadius(line, indexes[0])
    : "";
  const response = prompt("Node curve radius in metres (minimum 1)", initial);
  if (response === null || response.trim() === "") return;
  const radius = Number(response);
  if (!Number.isFinite(radius) || radius < 1) {
    toast("Enter a curve radius of at least 1 metre.");
    return;
  }
  const changed = indexes.some(
    (index) =>
      !hasNodeRadiusOverride(line, index) || nodeRadius(line, index) !== radius,
  );
  if (!changed) return;
  pushHistory();
  indexes.forEach((index) => setNodeRadius(line, index, radius));
  afterGeometryChange(line);
}

/** Remove stored overrides, including dormant endpoint/Cableway settings. */
function resetSelectedNodeRadius(line, indexes) {
  const overridden = indexes.filter((index) => hasNodeRadiusOverride(line, index));
  if (!overridden.length) return;
  pushHistory();
  overridden.forEach((index) => setNodeRadius(line, index, null));
  afterGeometryChange(line);
}

/** Ask how many consecutive nodes to remove from one end of a line. */
export function promptLineTrim(line, end = "end") {
  const maximum = trimmableNodeCount(line, end);
  if (!maximum) {
    toast("Keep at least one node and any attached branch anchor.");
    return;
  }
  const response = prompt(
    `Delete how many nodes from the ${end} of ${line.name || "this line"}? (1–${maximum})`,
    "1",
  );
  if (response === null || response.trim() === "") return;
  const count = Number(response);
  if (!Number.isInteger(count) || count < 1 || count > maximum) {
    toast(`Enter a whole number from 1 to ${maximum}.`);
    return;
  }
  trimLineEnd(line, count, end);
}

export function nodeMenuItems(line, index) {
  const isEndpoint = index === 0 || index === line.nodes.length - 1;
  const isBranchAnchor =
    line.branchOf && line.branchOf.branchNodeIndex === index;
  const selected = menuNodeIndexes(line, index);
  const multiple = selected.length > 1;
  const splittable = selected.filter(
    (item) => item > 0 && item < line.nodes.length - 1,
  );
  const radiusTargets = modeRules(line.mode).straight ? [] : splittable;
  const resetTargets = selected.filter((item) =>
    hasNodeRadiusOverride(line, item),
  );
  return [
    isEndpoint &&
      !isBranchAnchor && {
        label: `Draw from this ${index === 0 ? "start" : "end"}`,
        action: () => {
          selectLine(line.id);
          state.extendFrom = index === 0 ? "start" : "end";
          setTool("draw");
        },
      },
    !multiple && {
      label: "Branch from here",
      action: () => {
        selectNode(line.id, index, "replace");
        branchFromSelection();
      },
    },
    splittable.length > 0 && {
      label:
        splittable.length > 1
          ? `Split line at ${splittable.length} nodes`
          : "Split line here",
      action: () => splitLineAt(line, splittable),
    },
    radiusTargets.length > 0 && {
      label:
        radiusTargets.length > 1
          ? `Set curve radius for ${radiusTargets.length} nodes…`
          : "Set node curve radius…",
      action: () => setSelectedNodeRadius(line, radiusTargets),
    },
    resetTargets.length > 0 && {
      label:
        resetTargets.length > 1
          ? `Use line curve radius for ${resetTargets.length} nodes`
          : "Use line curve radius",
      action: () => resetSelectedNodeRadius(line, resetTargets),
    },
    isEndpoint && trimmableNodeCount(line, index === 0 ? "start" : "end") > 0 && {
      label: `Delete nodes from this ${index === 0 ? "start" : "end"}…`,
      action: () => promptLineTrim(line, index === 0 ? "start" : "end"),
    },
    {
      label: multiple ? `Delete ${selected.length} nodes` : "Delete node",
      action: () => deleteSelectedNodes(line, multiple ? selected : [index]),
    },
  ];
}
