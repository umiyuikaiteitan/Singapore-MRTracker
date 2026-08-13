/* Station search on the hub.
 *
 * An enhancement only: the whole station list is in the document, so
 * a visitor without JavaScript scrolls or uses the browser's own find
 * command. This adds a filter box and a live count.
 *
 * The search key of each row is a data attribute that the generator
 * wrote — lower case, letters and digits only — so matching needs no
 * normalization table here. */
(function () {
  "use strict";

  var search = document.getElementById("station-search");
  var list = document.getElementById("station-list");
  if (!search || !list) {
    return;
  }

  var rows = Array.prototype.slice.call(list.querySelectorAll(".station-row"));
  var count = document.getElementById("station-count");
  var empty = document.getElementById("no-matches");
  var total = rows.length;

  var box = search.parentNode;
  if (box && box.hidden) {
    box.hidden = false;
  }

  function squash(value) {
    var out = "";
    var lower = value.toLowerCase();
    for (var i = 0; i < lower.length; i++) {
      var c = lower.charAt(i);
      // Keep letters, digits, and spaces; drop punctuation, so that
      // "ns-1", "NS 1", and "ns1" all match the same row.
      if (/[a-z0-9\s]/.test(c) || c.charCodeAt(0) > 127) {
        out += c;
      }
    }
    return out.replace(/\s+/g, " ").trim();
  }

  function apply() {
    var query = squash(search.value);
    var terms = query.length ? query.split(" ") : [];
    var shown = 0;
    for (var i = 0; i < rows.length; i++) {
      var key = rows[i].getAttribute("data-search") || "";
      var match = true;
      for (var t = 0; t < terms.length; t++) {
        if (key.indexOf(terms[t]) === -1) {
          match = false;
          break;
        }
      }
      rows[i].hidden = !match;
      if (match) {
        shown++;
      }
    }
    if (count) {
      count.textContent =
        shown === total
          ? total + " stations"
          : shown + " of " + total + " stations";
    }
    if (empty) {
      empty.hidden = shown !== 0;
    }
  }

  search.addEventListener("input", apply);
  search.addEventListener("search", apply);

  // Enter opens the only remaining match, so a search can end at the
  // keyboard.
  search.addEventListener("keydown", function (event) {
    if (event.key !== "Enter") {
      return;
    }
    var visible = rows.filter(function (row) {
      return !row.hidden;
    });
    if (visible.length === 1) {
      var link = visible[0].querySelector("a");
      if (link) {
        event.preventDefault();
        window.location.href = link.getAttribute("href");
      }
    }
  });

  apply();
})();
