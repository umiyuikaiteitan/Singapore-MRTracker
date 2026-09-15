#!/usr/bin/env python3
"""Cache active Singapore OSM tracks and enrich the editor's private build input."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import time
import urllib.parse
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
ENDPOINT = 'https://overpass-api.de/api/interpreter'
QUERY = '''[out:json][timeout:60][maxsize:134217728];
(way["railway"~"^(rail|subway|light_rail|monorail|tram)$"](1.15,103.58,1.49,104.10);
relation["type"="route"]["route"~"^(subway|light_rail|monorail|train|tram)$"](1.15,103.58,1.49,104.10););
out body geom;'''
LIMIT = 32 * 1024 * 1024
MAX_STALE = 30 * 86400

def validate(data):
    value = json.loads(data)
    if not isinstance(value, dict) or value.get('remark') or not isinstance(value.get('elements'), list):
        raise ValueError('Incomplete Overpass response')
    if not value['elements'] or len(value['elements']) > 30000:
        raise ValueError('Empty or oversized Overpass response')
    return value

def load_osm(cache, now=None, opener=urllib.request.urlopen):
    now = time.time() if now is None else now
    cache = Path(cache)
    cached = None
    week = time.strftime('%G-%V', time.gmtime(now))
    if cache.exists() and cache.stat().st_size <= LIMIT:
        try:
            candidate = json.loads(cache.read_text())
            age = now - candidate['fetched_at']
            if candidate['query_sha256'] == hashlib.sha256(QUERY.encode()).hexdigest() and 0 <= age < MAX_STALE:
                validate(json.dumps(candidate['osm']))
                cached = candidate
                if candidate.get('checked_week') == week:
                    return candidate['osm'], {'cached': True, 'age_days': round(age / 86400, 2), 'refresh_failed': candidate.get('refresh_failed', False)}
        except (ValueError, KeyError, TypeError):
            pass
    request = urllib.request.Request(ENDPOINT, data=urllib.parse.urlencode({'data': QUERY}).encode(), headers={
        'User-Agent': 'Singapore-MRTracker/1.0 (https://github.com/umiyuikaiteitan/Singapore-MRTracker)',
        'Content-Type': 'application/x-www-form-urlencoded',
    })
    # One bounded request per weekly refresh. On overload use a previously valid
    # snapshot instead of retrying the public service from every hourly build.
    try:
        with opener(request, timeout=90) as response:
            data = response.read(LIMIT + 1)
        if len(data) > LIMIT:
            raise ValueError('Overpass response exceeds 32 MiB')
        osm = validate(data)
        cache.parent.mkdir(parents=True, exist_ok=True)
        staging = cache.with_suffix('.tmp')
        staging.write_text(json.dumps({'fetched_at': now, 'checked_week': week, 'query_sha256': hashlib.sha256(QUERY.encode()).hexdigest(), 'osm': osm}))
        staging.replace(cache)
        return osm, {'cached': False, 'age_days': 0}
    except (OSError, ValueError):
        if cached is None:
            raise
        # Persist the attempted week too: an outage must not cause every hourly
        # run to retry an immutable weekly cache entry. Retry next week.
        cached.update(checked_week=week, refresh_failed=True)
        cache.write_text(json.dumps(cached))
        return cached['osm'], {'cached': True, 'age_days': round((now - cached['fetched_at']) / 86400, 2), 'refresh_failed': True}

def prepare(feed, output, cache):
    osm, cache_info = load_osm(cache)
    with tempfile.TemporaryDirectory() as temporary:
        directory = Path(temporary)
        osm_path = directory / 'osm.json'
        osm_path.write_text(json.dumps(osm))
        result_path = directory / 'result.json'
        subprocess.run(['node', str(ROOT / 'scripts/reconstruct-gtfs.mjs'), str(feed), str(osm_path), str(result_path)], check=True, timeout=180)
        result = json.loads(result_path.read_text())
    report = {**result['report'], 'cache': cache_info}
    # Never ship a total failure as a successful reconstructed starter.
    if report['fallback']:
        raise ValueError('OSM reconstruction left rail patterns without shapes')
    tables = result['files']
    if sum(len(value.encode()) for value in tables.values()) > 64 * 1024 * 1024:
        raise ValueError('Enriched GTFS exceeds importer size limit')
    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    staging = output.with_suffix('.tmp')
    with zipfile.ZipFile(staging, 'w', zipfile.ZIP_DEFLATED) as archive:
        for name, value in sorted(tables.items()):
            if name in {'routes.txt', 'trips.txt', 'stops.txt', 'stop_times.txt', 'shapes.txt'}:
                entry = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                entry.compress_type = zipfile.ZIP_DEFLATED
                archive.writestr(entry, value)
    if staging.stat().st_size > LIMIT:
        staging.unlink()
        raise ValueError('Enriched ZIP exceeds importer size limit')
    staging.replace(output)
    output.with_suffix('.json').write_text(json.dumps(report, indent=2) + '\n')
    print('OSM reconstruction:', json.dumps(report))

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('feed', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--osm-cache', type=Path, default=Path('.cache/singapore-rail.json'))
    args = parser.parse_args()
    prepare(args.feed, args.output, args.osm_cache)
