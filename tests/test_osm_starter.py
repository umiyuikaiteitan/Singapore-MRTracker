import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile

spec = importlib.util.spec_from_file_location('osm_starter', Path(__file__).resolve().parents[1] / 'scripts/prepare-singapore-starter.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class CacheTests(unittest.TestCase):
    def test_fresh_download_then_cache_avoids_network(self):
        with tempfile.TemporaryDirectory() as temp:
            cache = Path(temp) / 'osm.json'
            data = {'elements': [{'type': 'way', 'id': 1}]}
            calls = []
            def opener(request, timeout):
                calls.append(request.full_url)
                return io.BytesIO(json.dumps(data).encode())
            self.assertEqual(module.load_osm(cache, now=100, opener=opener)[0], data)
            self.assertTrue(module.load_osm(cache, now=200, opener=opener)[1]['cached'])
            self.assertEqual(calls, [module.ENDPOINT])

    def test_failed_refresh_uses_bounded_stale_cache(self):
        with tempfile.TemporaryDirectory() as temp:
            cache = Path(temp) / 'osm.json'
            cache.write_text(json.dumps({'fetched_at': 0, 'query_sha256': hashlib.sha256(module.QUERY.encode()).hexdigest(), 'osm': {'elements': [{'id': 1}]}}))
            def failed(*args, **kwargs):
                raise OSError('offline')
            self.assertTrue(module.load_osm(cache, now=8*86400, opener=failed)[1]['refresh_failed'])
            with self.assertRaises(OSError):
                module.load_osm(cache, now=31*86400, opener=failed)

    def test_week_rollover_refreshes_even_when_younger_than_seven_days(self):
        with tempfile.TemporaryDirectory() as temp:
            cache = Path(temp) / 'osm.json'
            calls = []
            def opener(*args, **kwargs):
                calls.append(1)
                return io.BytesIO(b'{"elements":[{"id":1}]}')
            sunday = 1789343940  # 2026-09-13 23:59 UTC
            module.load_osm(cache, now=sunday, opener=opener)
            module.load_osm(cache, now=sunday+120, opener=opener)
            module.load_osm(cache, now=sunday+3600, opener=opener)
            self.assertEqual(len(calls), 2)

    def test_incomplete_response_never_poison_cache(self):
        with tempfile.TemporaryDirectory() as temp:
            cache = Path(temp) / 'osm.json'
            with self.assertRaises(ValueError):
                module.load_osm(cache, opener=lambda *a, **k: io.BytesIO(b'{"elements":[],"remark":"timeout"}'))
            self.assertFalse(cache.exists())

class PrepareTests(unittest.TestCase):
    def test_actual_packaging_and_coverage_gate(self):
        osm = {'elements': [{'type':'way','id':1,'nodes':[1,2,3], 'tags':{'railway':'subway'}, 'geometry':[{'lat':1.3,'lon':103.8},{'lat':1.31,'lon':103.81},{'lat':1.3,'lon':103.82}]}]}
        tables = {'routes.txt':'route_id,route_type\nR,1\n', 'trips.txt':'route_id,trip_id\nR,T\n', 'stops.txt':'stop_id,stop_name,stop_lat,stop_lon\nA,A,1.3,103.8\nB,B,1.3,103.82\n', 'stop_times.txt':'trip_id,stop_id,stop_sequence\nT,A,1\nT,B,2\n'}
        with tempfile.TemporaryDirectory() as temp, patch.object(module, 'load_osm', return_value=(osm, {'cached':False,'age_days':0})):
            directory=Path(temp); feed=directory/'feed.zip'; output=directory/'starter.zip'
            def write_feed():
                with zipfile.ZipFile(feed,'w',zipfile.ZIP_DEFLATED) as archive:
                    for name,value in tables.items(): archive.writestr(name,value)
            write_feed(); module.prepare(feed,output,directory/'cache.json')
            with zipfile.ZipFile(output) as archive:
                self.assertEqual(set(archive.namelist()),set(tables)|{'shapes.txt'})
                self.assertIn(b'1.31',archive.read('shapes.txt'))
            self.assertEqual(json.loads(output.with_suffix('.json').read_text())['fallback'],0)
            output.unlink()
            for n in [1,2]:
                tables['routes.txt']+=f'X{n},1\n'
                tables['trips.txt']+=f'X{n},X{n}\n'
                tables['stops.txt']+=f'C{n},C{n},1.4,104.0\n'
                tables['stop_times.txt']+=f'X{n},A,1\nX{n},C{n},2\n'
            write_feed()
            with self.assertRaisesRegex(ValueError,'without shapes'):
                module.prepare(feed,output,directory/'cache.json')
            self.assertFalse(output.exists())

if __name__ == '__main__':
    unittest.main()
