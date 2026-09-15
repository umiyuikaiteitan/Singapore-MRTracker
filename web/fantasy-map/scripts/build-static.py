#!/usr/bin/env python3
"""Build a relocatable editor directory using only the Python standard library."""
import argparse
import io
import zipfile
import ipaddress
import json
from pathlib import Path
import re
import shutil
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parents[1]


def singapore_archive(path):
    """Package only the GTFS tables used by the editor, without credentials."""
    wanted = {"routes.txt", "trips.txt", "stops.txt", "stop_times.txt", "shapes.txt"}
    required = wanted - {"shapes.txt"}
    tables = {}
    total = 0
    with zipfile.ZipFile(path) as archive:
        for entry in archive.infolist():
            name = Path(entry.filename).name
            if name not in wanted:
                continue
            if name in tables:
                raise ValueError(f"Duplicate GTFS table: {name}")
            total += entry.file_size
            if total > 64 * 1024 * 1024:
                raise ValueError("Singapore GTFS tables exceed the 64 MiB import limit")
            tables[name] = archive.read(entry)
    if not required.issubset(tables):
        raise ValueError("Singapore GTFS is missing required tables")
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w", zipfile.ZIP_DEFLATED) as archive:
        for name, content in sorted(tables.items()):
            archive.writestr(name, content)
    data = buffer.getvalue()
    if len(data) > 32 * 1024 * 1024:
        raise ValueError("Singapore GTFS ZIP exceeds the 32 MiB import limit")
    return data


def build(output, api_base=None, board_href=None, singapore=False, singapore_gtfs=None):
    if api_base:
        # urlsplit silently removes some control characters and does not
        # validate ports until .port is accessed. Reject these before output.
        if any(char.isspace() or ord(char) < 32 or ord(char) == 127 for char in api_base) or "\\" in api_base:
            raise ValueError("api_base must not contain whitespace, control characters, or backslashes")
        url = urlsplit(api_base)
        _ = url.port
        if (url.scheme != "https" or not url.hostname or url.username or url.password
                or url.query or url.fragment):
            raise ValueError("api_base must be an absolute HTTPS API URL without credentials, query, or fragment")
        if ":" in url.hostname:
            if "%" in url.hostname:
                raise ValueError("api_base must not contain an IPv6 zone ID")
            ipaddress.IPv6Address(url.hostname)
        else:
            host = url.hostname.encode("idna").decode("ascii").removesuffix(".")
            if len(host) > 253 or any(not re.fullmatch(r"[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?", label) for label in host.split(".")):
                raise ValueError("api_base must contain a valid hostname")
            # Browsers interpret numeric final labels as IPv4, not DNS names.
            last_label = host.rsplit(".", 1)[-1]
            if last_label.isdigit() or re.fullmatch(r"0[xX][0-9A-Fa-f]+", last_label):
                ipaddress.IPv4Address(host)
    if board_href and (urlsplit(board_href).scheme or board_href.startswith("//")):
        raise ValueError("board_href must be a relative link")
    starter = singapore_archive(singapore_gtfs) if singapore_gtfs else None
    output = Path(output).resolve()
    # Refuse to overwrite an existing directory or write into the source tree.
    if output == ROOT or output in ROOT.parents or output == ROOT / "static" or ROOT / "static" in output.parents:
        raise ValueError("output overlaps the source directory")
    output.mkdir(parents=True, exist_ok=False)
    shutil.copytree(ROOT / "static", output / "static")
    shutil.copyfile(ROOT / "static/index.html", output / "index.html")
    # The page lives at the output root, not at /static/index.html.
    (output / "static/index.html").unlink()
    config = {"apiBase": api_base or None, "boardHref": board_href}
    if starter is not None:
        (output / "singapore-mrt.zip").write_bytes(starter)
        config["singaporeGtfsUrl"] = "singapore-mrt.zip"
    if singapore:
        config["initialView"] = {"center": [1.3521, 103.8198], "zoom": 12}
    (output / "static/config.js").write_text(
        "window.OFM_CONFIG = " + json.dumps(config, ensure_ascii=True) + ";\n",
        encoding="utf-8",
    )
    (output / ".nojekyll").touch()
    return output


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", help="new output directory")
    parser.add_argument("--api-base", help="HTTPS API base including /api/; omitted means static mode")
    parser.add_argument("--board-href", help="relative link back to the departure board")
    parser.add_argument("--singapore", action="store_true")
    parser.add_argument("--singapore-gtfs", help="local official train GTFS ZIP to package as the Singapore starter")
    args = parser.parse_args()
    print(build(args.output, args.api_base, args.board_href, args.singapore, args.singapore_gtfs))
