#!/usr/bin/env node
"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const http = require("node:http");
const path = require("node:path");
const { chromium } = require("playwright");

const root = path.resolve(process.argv[2] || ".interface-preview/site");
const shots = path.resolve(process.argv[3] || ".interface-screenshots");
fs.mkdirSync(shots, { recursive: true });

const mime = new Map([
  [".css", "text/css"], [".html", "text/html"], [".js", "text/javascript"],
  [".json", "application/json"], [".mjs", "text/javascript"], [".svg", "image/svg+xml"],
  [".ttf", "font/ttf"], [".zip", "application/zip"],
]);

function server() {
  return http.createServer((request, response) => {
    let pathname;
    try { pathname = decodeURIComponent(new URL(request.url, "http://localhost").pathname); }
    catch { response.writeHead(400).end(); return; }
    let file = path.resolve(root, `.${pathname}`);
    if (file !== root && !file.startsWith(root + path.sep)) { response.writeHead(403).end(); return; }
    try {
      if (fs.statSync(file).isDirectory()) file = path.join(file, "index.html");
      const body = fs.readFileSync(file);
      response.writeHead(200, { "content-type": mime.get(path.extname(file)) || "application/octet-stream" });
      response.end(body);
    } catch { response.writeHead(404).end("Not found"); }
  });
}

async function noPageOverflow(page, label) {
  const dimensions = await page.evaluate(() => ({ width: innerWidth, scroll: document.documentElement.scrollWidth }));
  assert.ok(dimensions.scroll <= dimensions.width + 1, `${label} overflows horizontally: ${JSON.stringify(dimensions)}`);
}

async function visible(page, selector, label) {
  const locator = page.locator(selector).first();
  await locator.waitFor({ state: "visible" });
  const box = await locator.boundingBox();
  assert.ok(box && box.width > 0 && box.height > 0, `${label} has no usable box`);
  return locator;
}

async function routeGeometry(page, selector, railKind) {
  return page.locator(selector).evaluateAll((links, kind) => links.map(link => {
    const dot = getComputedStyle(link, "::after");
    const width = parseFloat(dot.width);
    const height = parseFloat(dot.height);
    const transformX = dot.transform === "none" ? 0 : new DOMMatrixReadOnly(dot.transform).m41;
    const dotCenterX = parseFloat(dot.left) + transformX + width / 2;
    const dotCenterFromBottom = parseFloat(dot.bottom) + height / 2;
    let railCenterFromBottom;
    if (kind === "border") {
      railCenterFromBottom = -parseFloat(getComputedStyle(link).borderBottomWidth) / 2;
    } else {
      const rail = getComputedStyle(link, "::before");
      railCenterFromBottom = parseFloat(rail.bottom) + parseFloat(rail.height) / 2;
    }
    return {
      width,
      height,
      boxSizing: dot.boxSizing,
      horizontalError: dotCenterX - link.clientWidth / 2,
      verticalError: dotCenterFromBottom - railCenterFromBottom,
    };
  }), railKind);
}

function assertRouteGeometry(geometry, label) {
  assert.ok(geometry.length > 0, `${label} did not render any route links`);
  for (const dot of geometry) {
    assert.ok(dot.width >= 12 && dot.width <= 16 && dot.width === dot.height, `${label} dots must stay small and circular`);
    assert.equal(dot.boxSizing, "border-box", `${label} dot dimensions must include their border`);
    assert.ok(Math.abs(dot.horizontalError) <= 0.5, `${label} dot is not horizontally centered: ${dot.horizontalError}px`);
    assert.ok(Math.abs(dot.verticalError) <= 0.5, `${label} rail misses the dot center: ${dot.verticalError}px`);
  }
}

async function capture(page, name, width, test) {
  await page.setViewportSize({ width, height: width === 375 ? 760 : 900 });
  await test();
  await noPageOverflow(page, `${name} at ${width}px`);
  await page.screenshot({ path: path.join(shots, `${name}-${width}.png`), fullPage: true });
}

async function main() {
  assert.ok(fs.existsSync(path.join(root, "index.html")), `preview not found at ${root}`);
  const host = server();
  await new Promise((resolve, reject) => host.listen(0, "127.0.0.1", resolve).once("error", reject));
  const base = `http://127.0.0.1:${host.address().port}`;
  const browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ serviceWorkers: "block" });
  const page = await context.newPage();
  await page.route("https://tile.openstreetmap.org/**", route => route.abort());
  try {
    for (const width of [375, 1440]) {
      await capture(page, "board", width, async () => {
        await page.goto(`${base}/`, { waitUntil: "networkidle" });
        await visible(page, "nav[aria-label]", "board navigation");
        const station = await visible(page, "#station", "board station control");
        assert.ok(await station.locator("option").count() > 0, "fixture stations did not load");
        assertRouteGeometry(await routeGeometry(page, ".section-link", "border"), "board navigation");
      });

      let timetablePageUrl;
      await capture(page, "timetables", width, async () => {
        await page.goto(`${base}/timetables/`, { waitUntil: "networkidle" });
        const search = await visible(page, "#station-search", "station search");
        await search.fill("NS 1");
        assert.equal(await page.locator(".station-row:not([hidden])").count(), 1, "station code search should resolve punctuation-insensitively");
        await search.fill("");
        const timetableHref = await (await visible(page, ".station-row a", "timetable link")).getAttribute("href");
        assert.ok(timetableHref, "timetable link should have a destination");
        timetablePageUrl = new URL(timetableHref, page.url()).href;
      });

      await capture(page, "timetable-page", width, async () => {
        await page.goto(timetablePageUrl, { waitUntil: "networkidle" });
        await visible(page, ".site-nav", "generated-page navigation");
      });

      await capture(page, "schematic", width, async () => {
        await page.goto(`${base}/schematic/`, { waitUntil: "networkidle" });
        await visible(page, "#motion", "schematic motion control");
        await page.locator("#motion").uncheck();
        assert.equal(await page.locator("#motion").isChecked(), false);
      });

      await capture(page, "live-map", width, async () => {
        await page.goto(`${base}/map/`, { waitUntil: "networkidle" });
        await visible(page, ".route-nav", "live-map navigation");
        await visible(page, "#fit", "fit-network control");
        await page.locator("#tiles").uncheck();
        await page.locator("#stations").uncheck();
        assert.equal(await page.locator(".leaflet-control-zoom").count(), 1, "Leaflet map controls should remain available");
        assert.match(await page.locator("#count").textContent(), /estimated trains/);
        assertRouteGeometry(await routeGeometry(page, ".route-nav a", "pseudo"), "live-map navigation");
      });

      await capture(page, "editor", width, async () => {
        await page.goto(`${base}/fantasy-map/`, { waitUntil: "domcontentloaded" });
        await visible(page, "#tools", "editor tools");
        const before = await page.locator("#line-list .line-item").count();
        await page.locator("#new-line").click();
        assert.equal(await page.locator("#line-list .line-item").count(), before + 1, "New line must add an editable line");
        await page.locator('[data-tool="draw"]').click();
        assert.ok(await page.locator('[data-tool="draw"]').evaluate(el => el.classList.contains("active") || el.getAttribute("aria-pressed") === "true"), "Draw tool must expose its active state");
      });
    }

    await page.goto(`${base}/timetables/`, { waitUntil: "networkidle" });
    await page.emulateMedia({ media: "print" });
    assert.equal(await page.locator(".search").evaluate(el => getComputedStyle(el).display), "none", "station search should not print");
    assert.equal(await page.evaluate(() => getComputedStyle(document.body).backgroundColor), "rgb(255, 255, 255)", "timetable print background should be white");
    await page.screenshot({ path: path.join(shots, "timetables-print.png"), fullPage: true });
  } catch (error) {
    await page.screenshot({ path: path.join(shots, "failure.png"), fullPage: true }).catch(() => {});
    throw error;
  } finally {
    await browser.close();
    await new Promise(resolve => host.close(resolve));
  }
}

main().catch(error => { console.error(error); process.exitCode = 1; });
