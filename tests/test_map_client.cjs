const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { test } = require('node:test');

// Execute the production inline client with a small DOM and controlled clock.
// No duplicated positioning or freshness logic lives in the test harness.
const html = fs.readFileSync(path.join(__dirname, '../crates/mrt-map-web/assets/map.html'), 'utf8');
const source = html.slice(html.lastIndexOf('<script>') + '<script>'.length, html.lastIndexOf('</script>'));

function element() {
  return {
    attributes: {}, children: [], textContent: '', checked: true,
    classList: { remove() {}, toggle() {} },
    setAttribute(name, value) { this.attributes[name] = value; },
    getAttribute(name) { return this.attributes[name]; },
    removeAttribute(name) { delete this.attributes[name]; },
    appendChild(child) { this.children.push(child); },
    removeChild(child) { this.children.splice(this.children.indexOf(child), 1); },
    get firstChild() { return this.children[0]; },
  };
}

function browser(nowSeconds = 1000) {
  let now = nowSeconds * 1000;
  let response;
  const elements = Object.fromEntries(['map-geometry', 'map-trains', 'map-bands', 'lamp', 'statusText', 'motion'].map(id => [id, element()]));
  elements['map-geometry'].textContent = JSON.stringify({
    lines: [{ color: '#ff0000' }], sections: { '0-0-1': [[0, 0], [100, 0]] },
    stations: { 0: [0, 0] }, bands: {}, pollSecs: 15,
  });
  const context = vm.createContext({
    document: {
      getElementById: id => elements[id], querySelectorAll: () => [],
      createElementNS: () => element(), body: { getAttribute: () => 'data/map.json' },
    },
    Date: { now: () => now }, setInterval() {},
    fetch: () => {
      if (!response) return new Promise(() => {}); // Initial automatic poll.
      const value = response;
      return value instanceof Error ? Promise.reject(value) : Promise.resolve({ ok: true, json: () => Promise.resolve(value) });
    },
  });
  vm.runInContext(source, context);
  return {
    context, elements,
    advance(seconds) { now += seconds * 1000; context.positionTrains(); context.updateStatus(); },
    async poll(body) { response = body; await context.refresh(); },
    train(index = 0) { return elements['map-trains'].children[index]; },
    status() { return elements.statusText.textContent; },
  };
}

function documentAt(generated = 1000, qualities = ['schedule-only'], freshness = {}) {
  return {
    generated,
    snapshot: {
      freshness: { state: 'live', age_secs: 0, ageing_secs: 60, staleness_secs: 120, ...freshness },
      bands: [],
      trains: qualities.map(quality => ({
        line: 0, destination: 'Terminus', quality, delay_secs: quality === 'schedule-only' ? null : 60,
        location: { kind: 'on-edge', from: 0, to: 1 }, progress: 0.2, edge_secs: 100,
      })),
    },
  };
}

function x(train) { return Number(train.attributes.transform.match(/^translate\(([-\d.]+)/)[1]); }

test('identical polls retain train identity, continuous progress, and arrival clamp', async () => {
  const page = browser();
  const body = documentAt();
  await page.poll(body);
  const original = page.train();
  page.advance(15);
  assert.equal(x(original), 35);
  await page.poll(JSON.parse(JSON.stringify(body)));
  assert.equal(page.train(), original);
  assert.equal(x(page.train()), 35);
  page.advance(100);
  assert.equal(x(page.train()), 100);
  assert.match(page.train().attributes.class, /overdue/);
  await page.poll(body);
  assert.equal(x(page.train()), 100);
  assert.match(page.train().attributes.class, /overdue/);
});

test('first load accounts for an old document before advancing any train', async () => {
  const page = browser(1100);
  await page.poll(documentAt(1000));
  assert.equal(x(page.train()), 100);
  assert.match(page.train().attributes.class, /overdue/);
});

test('expired predictions disappear without inventing a scheduled position', async () => {
  const page = browser();
  const body = documentAt(1000, ['at-station', 'interpolated-realtime', 'interpolated-schedule', 'schedule-only']);
  body.snapshot.trains[0].location = { kind: 'at-station', station: 0 };
  await page.poll(body);
  page.advance(121);
  for (let i = 0; i < 3; i++) assert.equal(page.train(i).attributes.display, 'none');
  assert.equal(page.train(3).attributes.display, undefined);
  assert.match(page.status(), /realtime stale.*3 expired prediction\(s\) hidden.*1 trains/);
  assert.doesNotMatch(page.status(), /schedule only/);
  await page.poll(body);
  for (let i = 0; i < 3; i++) assert.equal(page.train(i).attributes.display, 'none');
  const fresh = JSON.parse(JSON.stringify(body));
  fresh.generated = 1121;
  await page.poll(fresh);
  for (let i = 0; i < 3; i++) assert.equal(page.train(i).attributes.display, undefined);
  assert.doesNotMatch(page.status(), /expired/);
});

test('a failed fetch hides predictions immediately and a fresh recovery may restore them', async () => {
  const page = browser();
  const body = documentAt(1000, ['interpolated-realtime']);
  await page.poll(body);
  page.advance(10);
  await page.poll(new Error('offline'));
  assert.equal(page.train().attributes.display, 'none');
  assert.match(page.status(), /connection lost/);
  await page.poll(body);
  assert.equal(page.train().attributes.display, undefined);
  assert.equal(x(page.train()), 30);
});

test('an older response cannot overwrite a newer accepted snapshot', async () => {
  const page = browser(1020);
  await page.poll(documentAt(1020));
  const train = page.train();
  page.advance(10);
  await page.poll(documentAt(1000));
  assert.equal(page.context.generated, 1020);
  assert.equal(page.train(), train);
  assert.equal(x(page.train()), 30);
});

test('pausing motion does not prevent prediction expiry', async () => {
  const page = browser();
  page.elements.motion.checked = false;
  await page.poll(documentAt(1000, ['interpolated-realtime']));
  page.advance(30);
  assert.equal(x(page.train()), 20);
  page.advance(91);
  assert.equal(page.train().attributes.display, 'none');
});

test('a server clock ahead of the visitor still ages during repeated polls', async () => {
  const page = browser(900);
  const body = documentAt(1000, ['interpolated-realtime']);
  await page.poll(body);
  page.advance(121);
  await page.poll(body);
  assert.equal(page.train().attributes.display, 'none');
  assert.match(page.status(), /realtime stale/);
});
