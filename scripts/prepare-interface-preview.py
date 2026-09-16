#!/usr/bin/env python3
"""Build a local, fixture-only site for the browser interface checks."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import time
import zipfile


ROOT = Path(__file__).resolve().parents[1]
MINI_GTFS = ROOT / "crates/mrt-gtfs/tests/fixtures/mini"
MARKER = ".mrtracker-interface-preview"
MARKER_CONTENT = "Owned by scripts/prepare-interface-preview.py\n"


def run(*command: str, env: dict[str, str] | None = None) -> None:
    subprocess.run(command, cwd=ROOT, env=env, check=True)


def make_feed(path: Path) -> None:
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as archive:
        for table in sorted(MINI_GTFS.glob("*.txt")):
            archive.write(table, table.name)


def make_live_map(site: Path) -> None:
    target = site / "map"
    shutil.copytree(ROOT / "web/live-map", target)
    vendor = target / "vendor"
    vendor.mkdir()
    for name in ("leaflet.css", "leaflet.js"):
        shutil.copy2(ROOT / "web/fantasy-map/static/vendor" / name, vendor / name)

    now = int(time.time())
    network = {
        "version": 1,
        "generated": now,
        "validFrom": now - 3600,
        "validUntil": now + 3600,
        "routes": {"fixture": {"name": "Fixture line", "color": "#d52b1e"}},
        "lines": [{
            "name": "Fixture line",
            "color": "#d52b1e",
            "points": [[1.3331, 103.7422], [1.3854, 103.7443]],
        }],
        "stations": [
            {"id": "JUR", "name": "Jurong East", "code": "NS1 / EW24", "codes": ["NS1", "EW24"], "point": [1.3331, 103.7422]},
            {"id": "CCK", "name": "Choa Chu Kang", "code": "NS4 / BP1", "codes": ["NS4", "BP1"], "point": [1.3854, 103.7443]},
        ],
        "shapes": {"fixture-shape": [[1.3331, 103.7422, 0], [1.3854, 103.7443, 6000]]},
        "patterns": [{"shape": "fixture-shape", "stops": ["JUR", "CCK"], "distances": [0, 6000]}],
        "runs": [{
            "id": "fixture-run",
            "trip": "fixture-trip",
            "route": "fixture",
            "day": "fixture-day",
            "base": now - 300,
            "headsign": "Choa Chu Kang",
            "computed": False,
            "frequency": False,
            "pattern": 0,
            "times": [0, 0, 600, 600],
        }],
        "bands": [],
    }
    (target / "network.json").write_text(json.dumps(network), encoding="utf-8")

    data = site / "data"
    data.mkdir(exist_ok=True)
    (data / "config.json").write_text("{}\n", encoding="utf-8")
    (data / "live.json").write_text(
        json.dumps({"generated": now, "live": False}) + "\n", encoding="utf-8"
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", nargs="?", default=".interface-preview")
    args = parser.parse_args()
    output = Path(args.output).resolve()
    if output == ROOT or ROOT not in output.parents:
        raise SystemExit("output must be a directory inside the repository")
    if output.exists():
        marker = output / MARKER
        if (
            marker.is_symlink()
            or not marker.is_file()
            or marker.read_text(encoding="utf-8") != MARKER_CONTENT
        ):
            raise SystemExit(
                f"refusing to replace {output}: helper ownership marker is missing"
            )
        shutil.rmtree(output)
    output.mkdir(parents=True)
    (output / MARKER).write_text(MARKER_CONTENT, encoding="utf-8")
    feed = output / "mini-gtfs.zip"
    make_feed(feed)

    site = output / "site"
    run(
        "cargo", "run", "-p", "mrt-board-static", "--bin", "mrt-board-static", "--",
        str(site), str(feed),
    )

    timetable_env = os.environ.copy()
    timetable_env.update({"MRT_SITE_DAYS": "1", "MRT_SITE_TITLE": "Fixture rail timetables"})
    run(
        "cargo", "run", "-p", "mrt-schedule-site", "--bin", "mrt-schedule-site", "--",
        str(site / "timetables"), str(feed),
        env=timetable_env,
    )

    schematic_env = os.environ.copy()
    schematic_env["MRT_MAP_LAYOUT"] = "config/layout-mini.geojson"
    run(
        "cargo", "run", "-p", "mrt-map-static", "--bin", "mrt-map-static", "--",
        str(site / "schematic"), str(feed),
        env=schematic_env,
    )
    make_live_map(site)
    run(
        "python3", "scripts/build-fantasy-map.py", str(site),
        "--singapore-gtfs", str(feed),
    )
    print(site)


if __name__ == "__main__":
    main()
