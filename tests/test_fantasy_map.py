"""Exercise the deployed directory layout without feed credentials or Rust."""
import hashlib
import functools
import http.server
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import threading
import unittest
import urllib.request
from urllib.parse import urljoin, urlsplit

ROOT = Path(__file__).resolve().parents[1]

class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *_):
        pass

class FantasyMapBuild(unittest.TestCase):
    def build(self, output, api=''):
        subprocess.run([sys.executable, str(ROOT / 'scripts/build-fantasy-map.py'), str(output)],
                       env={**os.environ, 'OFM_API_BASE': api}, check=True, capture_output=True)

    def test_static_assets_resolve_at_root_and_project_subpaths(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for prefix in ['', 'Singapore-MRTracker']:
                site = root / prefix
                site.mkdir(exist_ok=True)
                # Packaging must preserve the existing board and schedule data.
                (site / 'index.html').write_text('board remains here')
                (site / 'timetables').mkdir()
                (site / 'timetables/index.html').write_text('timetables remain here')
                self.build(site)
                self.assertEqual((site / 'index.html').read_text(), 'board remains here')
                self.assertEqual((site / 'timetables/index.html').read_text(), 'timetables remain here')
            server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), functools.partial(QuietHandler, directory=str(root)))
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            try:
                for prefix in ['', '/Singapore-MRTracker']:
                    base = f'http://127.0.0.1:{server.server_port}{prefix}/fantasy-map/'
                    with urllib.request.urlopen(base) as response:
                        html = response.read().decode()
                    assets = re.findall(r'(?:src|href)="(static/[^\"]+)"', html)
                    self.assertGreater(len(assets), 4)
                    seen = set()
                    pending = [urljoin(base, asset) for asset in assets]
                    while pending:
                        url = pending.pop()
                        if url in seen:
                            continue
                        seen.add(url)
                        with urllib.request.urlopen(url) as response:
                            content = response.read().decode()
                            if url.endswith('.js'):
                                self.assertIn('javascript', response.headers['Content-Type'])
                        if url.endswith('.js'):
                            for module in re.findall(r'(?:from\s+|import\s+)["\'](\.[^"\']+)["\']', content):
                                pending.append(urljoin(url, module))
                    self.assertGreater(len(seen), 20)
                    config = urllib.request.urlopen(urljoin(base, 'static/config.js')).read().decode()
                    config = json.loads(config.removeprefix('window.OFM_CONFIG = ').strip().removesuffix(';'))
                    self.assertIsNone(config['apiBase'])
                    self.assertEqual(config['initialView']['center'], [1.3521, 103.8198])
                    self.assertEqual(urlsplit(urljoin(base, config['boardHref'])).path, prefix + '/')
            finally:
                server.shutdown()
                server.server_close()
                thread.join()

    def test_snapshot_matches_manifest(self):
        root = ROOT / 'web/fantasy-map'
        manifest = json.loads((root / 'UPSTREAM.json').read_text())
        actual = {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest()
                  for p in root.rglob('*') if p.is_file() and p.name != 'UPSTREAM.json'}
        self.assertEqual(actual, manifest['files'])

    def test_optional_api_configuration(self):
        with tempfile.TemporaryDirectory() as temporary:
            self.build(temporary, 'https://api.example.test/api/')
            config = (Path(temporary) / 'fantasy-map/static/config.js').read_text()
            self.assertIn('https://api.example.test/api/', config)

    def test_invalid_api_does_not_publish_a_broken_editor(self):
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaises(subprocess.CalledProcessError):
                self.build(temporary, 'http://api.example.test/api/')
            self.assertFalse((Path(temporary) / 'fantasy-map').exists())

if __name__ == '__main__':
    unittest.main()
