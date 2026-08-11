const state = { data: null, activeLine: null, stationId: null, zoom: 1 };

const stationById = (id) => state.data.stations.find((station) => station.id === id);
const crowdColor = { Low: "#4de2c5", Moderate: "#ffc34c", High: "#ff6b61", Unknown: "#819096" };
const crowdRank = { Unknown: 0, Low: 1, Moderate: 2, High: 3 };
const escapeHtml = (value) => String(value).replace(/[&<>"']/g, (character) => ({
  "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
})[character]);

async function fetchJson(path) {
  const response = await fetch(path, { cache: "no-cache" });
  if (!response.ok) throw new Error(`${path} returned ${response.status}`);
  return response.json();
}

async function loadData() {
  state.data = await fetchJson("./data/demo.json");
  state.stationId = state.data.stations.find((station) => station.featured)?.id ?? state.data.stations[0].id;

  try {
    await hydrateFromPages();
    document.querySelector("#data-source").textContent = "GitHub Pages · generated from main";
  } catch (error) {
    console.info("Generated Pages data is unavailable; using the checked-in preview.", error);
  }
}

async function hydrateFromPages() {
  const live = await fetchJson("./data/live.json");
  const boards = await Promise.all(state.data.stations.map(async (station) => {
    try {
      return await fetchJson(`./data/board/${encodeURIComponent(station.codes[0])}.json`);
    } catch {
      return null;
    }
  }));

  let newest = live.generated || 0;
  state.data.stations.forEach((station, index) => {
    const board = boards[index];
    if (board) {
      newest = Math.max(newest, board.generated || 0);
      const now = Date.now() / 1000;
      station.board = board.rows
        .filter(([departure]) => departure >= now - 15)
        .slice(0, 10)
        .map(([departure, lineCode, destination, exact]) => ({
          line_code: lineCode,
          destination,
          departs_in_secs: Math.max(0, departure - now),
          clock_time: new Date(departure * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }),
          approximate: !exact,
          crowd: station.crowd,
        }));
    }

    const levels = station.codes.map((code) => ({ l: "Low", m: "Moderate", h: "High" })[live.crowd?.[code]] || "Unknown");
    station.crowd = levels.sort((a, b) => crowdRank[b] - crowdRank[a])[0] || "Unknown";
    station.board.forEach((row) => { row.crowd = station.crowd; });

    const affected = (live.segments || []).some((segment) =>
      (segment.stations || []).some((code) => station.codes.includes(code)));
    station.notices = live.disrupted && affected ? (live.messages || []) : [];
  });

  state.data.network_status = state.data.network_status.map((status) => {
    const affected = (live.segments || []).some((segment) =>
      segment.line?.toUpperCase().startsWith(status.line_code));
    return {
      ...status,
      status: live.disrupted && affected ? "Disrupted" : "Normal",
      message: live.disrupted && affected ? (live.messages || []).join(" ") : "",
    };
  });
  if (newest) {
    state.data.generated_at = new Date(newest * 1000).toLocaleString("en-SG", {
      timeZone: "Asia/Singapore", day: "2-digit", month: "short", hour: "2-digit", minute: "2-digit",
    }) + " SGT";
  }
}

function renderSummary() {
  const disrupted = state.data.network_status.filter((line) => line.status !== "Normal");
  document.querySelector("#line-count").textContent = state.data.lines.length;
  document.querySelector("#station-count").textContent = state.data.stations.length;
  document.querySelector("#crowd-count").textContent = state.data.stations.filter((station) => station.crowd === "High").length;
  document.querySelector("#service-label").textContent = disrupted.length ? `${disrupted.length} service notice${disrupted.length > 1 ? "s" : ""}` : "All services normal";
  document.querySelector("#service-copy").textContent = disrupted.length ? disrupted.map((line) => line.message).join(" ") : "No active disruptions across the monitored MRT and LRT lines.";
  const next = stationById(state.stationId).board[0];
  document.querySelector("#next-service").textContent = next ? `${Math.max(1, Math.round(next.departs_in_secs / 60))} min` : "—";
}

function renderFilters() {
  const host = document.querySelector("#line-filters");
  host.innerHTML = state.data.lines.map((line) => `
    <button class="line-filter${state.activeLine === line.id ? " is-active" : ""}" type="button" data-line="${line.id}" style="--line:${line.color}">
      <i></i><b>${line.code}</b><span>${line.short_name}</span>
    </button>`).join("");
  host.querySelectorAll("button").forEach((button) => button.addEventListener("click", () => {
    state.activeLine = state.activeLine === button.dataset.line ? null : button.dataset.line;
    renderFilters();
    updateMapState();
  }));
}

function polylinePoints(line) {
  return line.stations.map((stationId) => {
    const station = stationById(stationId);
    return `${station.x},${station.y}`;
  }).join(" ");
}

function renderMap() {
  const stage = document.querySelector("#map-stage");
  const lineMarkup = state.data.lines.map((line) => `
    <polyline class="map-line" data-line="${line.id}" points="${polylinePoints(line)}" style="--line:${line.color}" />`).join("");
  const stationMarkup = state.data.stations.map((station) => {
    const codes = station.codes.join(" · ");
    const labelAnchor = station.label_side === "left" ? "end" : "start";
    const dx = station.label_side === "left" ? -15 : 15;
    return `<g class="station-hit" data-station="${station.id}" data-lines="${station.lines.join(" ")}" tabindex="0" role="button" aria-label="${station.name}, ${codes}">
      <circle class="crowd-ring" cx="${station.x}" cy="${station.y}" r="13" stroke="${crowdColor[station.crowd]}" />
      <circle class="station-node" cx="${station.x}" cy="${station.y}" r="8" />
      <text class="station-label" x="${station.x + dx}" y="${station.y - 4}" text-anchor="${labelAnchor}">${station.name}</text>
      <text class="station-code-label" x="${station.x + dx}" y="${station.y + 13}" text-anchor="${labelAnchor}">${codes}</text>
    </g>`;
  }).join("");
  stage.innerHTML = lineMarkup + stationMarkup;
  stage.querySelectorAll(".station-hit").forEach((node) => {
    const select = () => { state.stationId = node.dataset.station; renderStation(); renderSummary(); updateMapState(); };
    node.addEventListener("click", select);
    node.addEventListener("keydown", (event) => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); select(); } });
  });
  updateMapState();
}

function updateMapState() {
  document.querySelectorAll(".map-line").forEach((line) => {
    line.classList.toggle("is-dimmed", Boolean(state.activeLine) && line.dataset.line !== state.activeLine);
    line.classList.toggle("is-highlighted", line.dataset.line === state.activeLine);
  });
  document.querySelectorAll(".station-hit").forEach((station) => {
    const servesActive = !state.activeLine || station.dataset.lines.split(" ").includes(state.activeLine);
    station.classList.toggle("is-dimmed", !servesActive);
    station.classList.toggle("is-selected", station.dataset.station === state.stationId);
  });
}

function codePills(station) {
  return station.codes.map((code) => {
    const prefix = code.replace(/[0-9]/g, "");
    const line = state.data.lines.find((item) => item.code === prefix || item.code.startsWith(prefix));
    return `<span class="code-pill" style="--line:${line?.color ?? "#59646a"};--text:${line?.text_color ?? "#fff"}">${code}</span>`;
  }).join("");
}

function departureMarkup(row, full = false, index = 0) {
  const line = state.data.lines.find((item) => item.code === row.line_code);
  const minutes = Math.max(1, Math.round(row.departs_in_secs / 60));
  if (full) return `<article class="full-row">
    <div class="platform">Platform ${index % 2 ? "B" : "A"}<br /><span class="code-pill" style="--line:${line?.color};--text:${line?.text_color}">${escapeHtml(row.line_code)}</span></div>
    <h3><span class="via">${row.approximate ? "Scheduled" : "Live estimate"}</span>${escapeHtml(row.destination)}</h3>
    <div class="eta"><strong>${minutes}</strong><span>mins</span></div>
  </article>`;
  return `<article class="departure">
    <div class="departure-main">
      <span class="line-name">${escapeHtml(line?.name ?? row.line_code)}</span>
      <h3>${escapeHtml(row.destination)}</h3>
      <div class="meta"><span>${row.clock_time}</span><span>·</span><span class="crowd-tag" style="--crowd:${crowdColor[row.crowd]}">${row.crowd} crowd</span></div>
    </div>
    <div class="eta"><strong>${minutes}</strong><span>mins</span></div>
  </article>`;
}

function renderStation() {
  const station = stationById(state.stationId);
  document.querySelector("#station-codes").innerHTML = codePills(station);
  document.querySelector("#station-name").textContent = station.name;
  document.querySelector("#station-crowd").textContent = `${station.crowd} crowd level · ${station.lines.length > 1 ? "Interchange" : "Single line"}`;
  document.querySelector("#departures").innerHTML = station.board.length
    ? station.board.slice(0, 3).map((row) => departureMarkup(row)).join("")
    : '<p class="empty-state">No upcoming departures in the generated schedule.</p>';
  const notice = document.querySelector("#station-notice");
  notice.hidden = !station.notices.length;
  notice.textContent = station.notices.join(" ");
  renderFullBoard();
}

function renderBoardPicker() {
  const picker = document.querySelector("#board-station");
  picker.innerHTML = state.data.stations.map((station) => `<option value="${station.id}">${station.name} · ${station.codes.join(" / ")}</option>`).join("");
  picker.value = state.stationId;
  picker.addEventListener("change", () => { state.stationId = picker.value; renderStation(); renderSummary(); updateMapState(); });
}

function renderFullBoard() {
  const station = stationById(state.stationId);
  document.querySelector("#board-station").value = state.stationId;
  document.querySelector("#full-board").innerHTML = station.board.length
    ? station.board.map((row, index) => departureMarkup(row, true, index)).join("")
    : '<p class="empty-state">No upcoming departures in the generated schedule.</p>';
}

function bindControls() {
  document.querySelector("#reset-lines").addEventListener("click", () => { state.activeLine = null; renderFilters(); updateMapState(); });
  document.querySelectorAll(".tab").forEach((tab) => tab.addEventListener("click", () => {
    document.querySelectorAll(".tab").forEach((item) => item.classList.toggle("is-active", item === tab));
    document.querySelectorAll("[data-panel]").forEach((panel) => { panel.hidden = panel.dataset.panel !== tab.dataset.view; });
  }));
  const updateZoom = (delta) => {
    state.zoom = Math.min(1.4, Math.max(.75, state.zoom + delta));
    document.querySelector("#network-map").style.transform = `scale(${state.zoom})`;
    document.querySelector("#zoom-level").value = `${Math.round(state.zoom * 100)}%`;
  };
  document.querySelector("#zoom-in").addEventListener("click", () => updateZoom(.1));
  document.querySelector("#zoom-out").addEventListener("click", () => updateZoom(-.1));
}

async function init() {
  try {
    await loadData();
    renderSummary(); renderFilters(); renderMap(); renderBoardPicker(); renderStation(); bindControls();
    document.querySelector("#last-updated").textContent = `Snapshot ${state.data.generated_at}`;
  } catch (error) {
    document.querySelector("#main").innerHTML = `<p class="station-notice">${error.message}. Serve this directory over HTTP; browsers block JSON loading from file:// URLs.</p>`;
  }
}

init();
