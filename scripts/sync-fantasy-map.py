#!/usr/bin/env python3
"""Refresh browser assets from an explicit OpenFantasyMap commit in a local clone."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def git(source, *args):
    return subprocess.check_output(['git', '-C', str(source), *args])


def sync(source, revision):
    commit = git(source, 'rev-parse', '--verify', revision + '^{commit}').decode().strip()
    entries = git(source, 'ls-tree', '-r', commit, '--', 'static', 'scripts/build-static.py').decode().splitlines()
    files = {}
    for entry in entries:
        metadata, path = entry.split('\t', 1)
        mode, kind, sha = metadata.split()
        if kind != 'blob' or mode not in ('100644', '100755'):
            raise ValueError('Only regular browser assets and the builder may be copied')
        files[path] = git(source, 'cat-file', 'blob', sha)
    for required in ['static/index.html', 'static/config.js', 'static/modules/config.js', 'scripts/build-static.py']:
        if required not in files:
            raise ValueError(f'This revision lacks the static hosting contract: {required}')
    destination = ROOT / 'web/fantasy-map'
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=destination.parent) as temporary:
        staging = Path(temporary) / 'snapshot'
        staging.mkdir()
        for path, content in files.items():
            target = staging / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(content)
        manifest = {
            'repository': 'https://github.com/umiyuikaiteitan/OpenFantasyMap',
            'revision': commit,
            'files': {path: hashlib.sha256(content).hexdigest() for path, content in sorted(files.items())},
        }
        (staging / 'UPSTREAM.json').write_text(json.dumps(manifest, indent=2) + '\n')
        if destination.exists():
            shutil.rmtree(destination)
        shutil.move(staging, destination)
    print(f'Pinned OpenFantasyMap {commit}: {len(files)} files')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path, help='local authenticated clone of OpenFantasyMap')
    parser.add_argument('--revision', required=True, help='explicit commit or ref to snapshot')
    args = parser.parse_args()
    sync(args.source, args.revision)
