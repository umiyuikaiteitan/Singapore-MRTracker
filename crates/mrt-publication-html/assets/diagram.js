/* Interaction for the train diagram.
 *
 * Everything here is an enhancement. Without it the page still shows
 * the drawing, the legend, the headway bands, and a call table for
 * every run, and it still prints. The script only adds pan, zoom,
 * highlighting, filters, and the SVG download.
 *
 * The run data comes from a JSON island in the document; the script
 * never fetches anything, so the page works from a file:// URL and
 * under a Content-Security-Policy that forbids network access. */
(function () {
  "use strict";

  var island = document.getElementById("diagram-data");
  var svg = document.getElementById("diagram-svg");
  if (!island || !svg) {
    return;
  }

  var data;
  try {
    data = JSON.parse(island.textContent);
  } catch (error) {
    return;
  }

  var base = data.viewBox;
  var view = { x: base.x, y: base.y, w: base.w, h: base.h };

  function applyView() {
    svg.setAttribute(
      "viewBox",
      view.x + " " + view.y + " " + view.w + " " + view.h
    );
  }

  function clampView() {
    var minW = base.w / 40;
    view.w = Math.min(Math.max(view.w, minW), base.w);
    view.h = Math.min(Math.max(view.h, base.h / 40), base.h);
    view.x = Math.min(Math.max(view.x, base.x), base.x + base.w - view.w);
    view.y = Math.min(Math.max(view.y, base.y), base.y + base.h - view.h);
  }

  function zoomBy(factor, anchorX, anchorY) {
    var ax = typeof anchorX === "number" ? anchorX : view.x + view.w / 2;
    var ay = typeof anchorY === "number" ? anchorY : view.y + view.h / 2;
    var newW = view.w * factor;
    var newH = data.zoomVertical ? view.h * factor : view.h;
    view.x = ax - (ax - view.x) * (newW / view.w);
    view.y = ay - (ay - view.y) * (newH / view.h);
    view.w = newW;
    view.h = newH;
    clampView();
    applyView();
  }

  function resetView() {
    view = { x: base.x, y: base.y, w: base.w, h: base.h };
    applyView();
  }

  /* ---------------------------------------------------------------- */
  /* Controls                                                          */
  /* ---------------------------------------------------------------- */

  /* The controls carry `needs-script`, which the stylesheet hides.
   * Removing the class is what reveals them; clearing an inline style
   * would leave the stylesheet rule in force. */
  var controls = document.querySelectorAll(".needs-script");
  for (var i = 0; i < controls.length; i++) {
    controls[i].classList.remove("needs-script");
  }

  function on(id, handler) {
    var node = document.getElementById(id);
    if (node) {
      node.addEventListener("click", handler);
    }
  }

  on("zoom-in", function () {
    zoomBy(1 / 1.4);
  });
  on("zoom-out", function () {
    zoomBy(1.4);
  });
  on("reset-view", resetView);
  on("print-page", function () {
    window.print();
  });
  on("toggle-mono", function () {
    document.documentElement.classList.toggle("monochrome");
  });

  var download = document.getElementById("download-svg");
  if (download) {
    download.addEventListener("click", function () {
      var clone = svg.cloneNode(true);
      clone.setAttribute("viewBox", base.x + " " + base.y + " " + base.w + " " + base.h);
      var text =
        '<?xml version="1.0" encoding="UTF-8"?>\n' +
        new XMLSerializer().serializeToString(clone);
      var blob = new Blob([text], { type: "image/svg+xml" });
      var url = URL.createObjectURL(blob);
      var link = document.createElement("a");
      link.href = url;
      link.download = data.fileName || "diagram.svg";
      document.body.appendChild(link);
      link.click();
      document.body.removeChild(link);
      setTimeout(function () {
        URL.revokeObjectURL(url);
      }, 0);
    });
  }

  /* ---------------------------------------------------------------- */
  /* Pan and wheel zoom                                                */
  /* ---------------------------------------------------------------- */

  var dragging = null;

  function svgPoint(event) {
    var rect = svg.getBoundingClientRect();
    if (!rect.width || !rect.height) {
      return { x: view.x, y: view.y };
    }
    return {
      x: view.x + ((event.clientX - rect.left) / rect.width) * view.w,
      y: view.y + ((event.clientY - rect.top) / rect.height) * view.h
    };
  }

  svg.addEventListener("pointerdown", function (event) {
    if (event.button !== 0) {
      return;
    }
    dragging = { start: svgPoint(event), x: view.x, y: view.y };
    svg.setPointerCapture(event.pointerId);
  });

  svg.addEventListener("pointermove", function (event) {
    if (!dragging) {
      return;
    }
    var rect = svg.getBoundingClientRect();
    if (!rect.width || !rect.height) {
      return;
    }
    var dx = ((event.clientX - rect.left) / rect.width) * view.w;
    var dy = ((event.clientY - rect.top) / rect.height) * view.h;
    view.x = dragging.x + (dragging.start.x - view.x - dx);
    view.y = dragging.y + (dragging.start.y - view.y - dy);
    clampView();
    applyView();
  });

  function endDrag(event) {
    if (dragging) {
      dragging = null;
      if (svg.hasPointerCapture && svg.hasPointerCapture(event.pointerId)) {
        svg.releasePointerCapture(event.pointerId);
      }
    }
  }

  svg.addEventListener("pointerup", endDrag);
  svg.addEventListener("pointercancel", endDrag);

  svg.addEventListener(
    "wheel",
    function (event) {
      if (!event.ctrlKey && !event.metaKey) {
        return;
      }
      event.preventDefault();
      var anchor = svgPoint(event);
      zoomBy(event.deltaY > 0 ? 1.15 : 1 / 1.15, anchor.x, anchor.y);
    },
    { passive: false }
  );

  /* Keyboard panning, so the diagram is usable without a pointer. */
  svg.addEventListener("keydown", function (event) {
    var step = view.w / 12;
    var handled = true;
    switch (event.key) {
      case "ArrowLeft":
        view.x -= step;
        break;
      case "ArrowRight":
        view.x += step;
        break;
      case "ArrowUp":
        view.y -= view.h / 12;
        break;
      case "ArrowDown":
        view.y += view.h / 12;
        break;
      case "+":
      case "=":
        zoomBy(1 / 1.4);
        return;
      case "-":
        zoomBy(1.4);
        return;
      case "0":
        resetView();
        return;
      default:
        handled = false;
    }
    if (handled) {
      event.preventDefault();
      clampView();
      applyView();
    }
  });

  /* ---------------------------------------------------------------- */
  /* Highlighting and details                                          */
  /* ---------------------------------------------------------------- */

  var details = document.getElementById("run-details-body");
  var runsById = {};
  for (var r = 0; r < data.runs.length; r++) {
    runsById[data.runs[r].id] = data.runs[r];
  }

  function textCell(value) {
    var cell = document.createElement("td");
    cell.textContent = value === null || value === undefined ? "—" : value;
    return cell;
  }

  function showRun(id) {
    var run = runsById[id];
    if (!run || !details) {
      return;
    }
    while (details.firstChild) {
      details.removeChild(details.firstChild);
    }
    var list = document.createElement("dl");
    var rows = [
      [data.labels.line, run.line],
      [data.labels.destination, run.destination],
      [data.labels.direction, run.direction],
      [data.labels.exactness, run.exactness]
    ];
    if (run.label) {
      rows.unshift([data.labels.train, run.label]);
    }
    if (run.tripId) {
      rows.push([data.labels.tripId, run.tripId]);
    }
    for (var i2 = 0; i2 < rows.length; i2++) {
      var term = document.createElement("dt");
      term.textContent = rows[i2][0];
      var value = document.createElement("dd");
      value.textContent = rows[i2][1];
      list.appendChild(term);
      list.appendChild(value);
    }
    details.appendChild(list);

    var table = document.createElement("table");
    var head = document.createElement("tr");
    var headings = [
      data.labels.station,
      data.labels.arrival,
      data.labels.departure,
      data.labels.platform
    ];
    for (var h = 0; h < headings.length; h++) {
      var th = document.createElement("th");
      th.textContent = headings[h];
      head.appendChild(th);
    }
    table.appendChild(head);
    for (var c = 0; c < run.calls.length; c++) {
      var call = run.calls[c];
      var row = document.createElement("tr");
      row.appendChild(textCell(call.station));
      row.appendChild(textCell(call.arrival));
      row.appendChild(textCell(call.departure));
      row.appendChild(textCell(call.platform));
      table.appendChild(row);
    }
    details.appendChild(table);
  }

  function setHighlight(id) {
    if (id) {
      svg.setAttribute("data-highlight", id);
      showRun(id);
    } else {
      svg.removeAttribute("data-highlight");
    }
  }

  var paths = svg.querySelectorAll("[data-run]");
  for (var p = 0; p < paths.length; p++) {
    (function (node) {
      var id = node.getAttribute("data-run");
      node.addEventListener("pointerenter", function () {
        setHighlight(id);
      });
      node.addEventListener("focus", function () {
        setHighlight(id);
      });
      node.addEventListener("pointerleave", function () {
        setHighlight(null);
      });
      node.addEventListener("blur", function () {
        setHighlight(null);
      });
    })(paths[p]);
  }

  /* ---------------------------------------------------------------- */
  /* Filters                                                           */
  /* ---------------------------------------------------------------- */

  function applyFilters() {
    var checked = {};
    var boxes = document.querySelectorAll("[data-filter]");
    for (var b = 0; b < boxes.length; b++) {
      var kind = boxes[b].getAttribute("data-filter");
      if (!checked[kind]) {
        checked[kind] = {};
      }
      checked[kind][boxes[b].value] = boxes[b].checked;
    }
    var groups = svg.querySelectorAll("[data-run]");
    for (var g = 0; g < groups.length; g++) {
      var node = groups[g];
      var visible = true;
      var keys = ["line", "direction", "destination", "exactness", "panel"];
      for (var k = 0; k < keys.length; k++) {
        var set = checked[keys[k]];
        if (!set) {
          continue;
        }
        var value = node.getAttribute("data-" + keys[k]);
        if (value !== null && set[value] === false) {
          visible = false;
        }
      }
      node.style.display = visible ? "" : "none";
    }
  }

  var filterBoxes = document.querySelectorAll("[data-filter]");
  for (var f = 0; f < filterBoxes.length; f++) {
    filterBoxes[f].addEventListener("change", applyFilters);
  }
  applyFilters();
})();
