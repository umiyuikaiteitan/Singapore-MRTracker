#!/usr/bin/env python3
"""Append the pinned OpenFantasyMap editor to the generated Pages site."""
import argparse
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("site", nargs="?", default="site")
    parser.add_argument("--singapore-gtfs", help="official train GTFS ZIP downloaded by the Pages workflow")
    args = parser.parse_args()
    command = [sys.executable, str(ROOT / "web/fantasy-map/scripts/build-static.py"),
               str(Path(args.site) / "fantasy-map"), "--singapore", "--board-href", "../"]
    if args.singapore_gtfs:
        command += ["--singapore-gtfs", args.singapore_gtfs]
    api_base = os.environ.get("OFM_API_BASE", "").strip()
    if api_base:
        command += ["--api-base", api_base]
    subprocess.run(command, check=True)
