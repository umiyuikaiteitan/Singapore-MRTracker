/** The right-click menu element itself, and the items offered over a node. */

import { state, selectedNodeIndexes, selectNode } from "./state.js";
import { map } from "./map-setup.js";
import { selectLine } from "./render.js";
import { branchFromSelection, splitLineAt } from "./topology.js";
import { deleteSelectedNodes } from "./interactions.js";
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

export function nodeMenuItems(line, index) {
  const isEndpoint = index === 0 || index === line.nodes.length - 1;
  const isBranchAnchor =
    line.branchOf && line.branchOf.branchNodeIndex === index;
  const selected = selectedNodeIndexes();
  const multiple = selected.length > 1;
  const splittable = selected.filter(
    (item) => item > 0 && item < line.nodes.length - 1,
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
    {
      label: multiple ? `Delete ${selected.length} nodes` : "Delete node",
      action: () => deleteSelectedNodes(line, multiple ? selected : [index]),
    },
  ];
}
