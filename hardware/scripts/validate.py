#!/usr/bin/env python3
"""Independent connectivity/clearance and CAM checks for this routed prototype.

No external dependencies. This checks circular PTH pads and round straight
tracks, the complete geometry used by generate.py. It does not claim KiCad DRC,
electrical simulation, thermal certification, or a physical first-article test.
"""
import json
import math
import re
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def point_segment(p, a, b):
    dx, dy = b[0]-a[0], b[1]-a[1]
    den = dx*dx+dy*dy
    t = max(0, min(1, ((p[0]-a[0])*dx+(p[1]-a[1])*dy)/den)) if den else 0
    return math.hypot(p[0]-a[0]-t*dx, p[1]-a[1]-t*dy)


def cross(a, b, c):
    return (b[0]-a[0])*(c[1]-a[1])-(b[1]-a[1])*(c[0]-a[0])


def segment_distance(a, b, c, d):
    # Bounding-box check is essential: collinear disjoint segments do not cross.
    if (max(min(a[0],b[0]),min(c[0],d[0])) <= min(max(a[0],b[0]),max(c[0],d[0]))+1e-9
        and max(min(a[1],b[1]),min(c[1],d[1])) <= min(max(a[1],b[1]),max(c[1],d[1]))+1e-9
        and cross(a,b,c)*cross(a,b,d) <= 0 and cross(c,d,a)*cross(c,d,b) <= 0):
        return 0
    return min(point_segment(a,c,d),point_segment(b,c,d),
               point_segment(c,a,b),point_segment(d,a,b))


def validate():
    board=json.loads((ROOT/'fabrication/board-model.json').read_text())
    layout=json.loads((ROOT/'layouts/singapore-mrt.json').read_text())
    inventory=json.loads((ROOT/'network-inventory.json').read_text())
    errors=[]
    def check(ok,msg):
        if not ok: errors.append(msg)
    count=layout['led_count']
    check(count==len(inventory['stations'])==len(layout['pixels']), 'Station inventory/layout size mismatch')
    check([p['index'] for p in layout['pixels']]==list(range(count)), 'Non-contiguous LED indices')
    for p,st in zip(layout['pixels'],inventory['stations']):
        check(set([p['station']]+p.get('station_aliases',[]))==set(st['codes']), f'Alias coverage mismatch at {p["index"]}')
    # Each primitive has its own node. Same-net intersecting copper unites them;
    # each through-hole pad belongs to both copper layers and bridges them.
    primitives=[]
    for p in board['pads']:
        xy=(p['x'],p['y'])
        primitives.append(dict(a=xy,b=xy,r=p['diameter']/2,net=p['net'],
                               layers={'F.Cu','B.Cu'},label=f'{p["ref"]}.{p["pin"]}'))
        check((p['diameter']-p['drill'])/2 >= .25, f'Annular ring too small: {p["ref"]}')
    for i,t in enumerate(board['tracks']):
        primitives.append(dict(a=t['start'],b=t['end'],r=t['width']/2,net=t['net'],
                               layers={t['layer']},label=f'track{i}'))
    parents=list(range(len(primitives)))
    def find(i):
        while parents[i]!=i:
            parents[i]=parents[parents[i]];i=parents[i]
        return i
    minimum=float('inf')
    for i,a in enumerate(primitives):
        for pt in [a['a'],a['b']]:
            check(min(pt[0],pt[1],board['width_mm']-pt[0],board['height_mm']-pt[1])-a['r'] >= .3,
                  f'Copper too close to outline: {a["label"]}')
        for hole in board['mounting_holes']:
            check(point_segment(hole,a['a'],a['b'])-a['r']-1.6 >= .25,
                  f'Copper too close to mounting hole: {a["label"]}')
        for j in range(i):
            b=primitives[j]
            if not a['layers'] & b['layers']: continue
            # Cheap bounding-box separation avoids almost all quadratic geometry.
            padding=a['r']+b['r']+board['clearance_mm']
            if (max(a['a'][0],a['b'][0])+padding < min(b['a'][0],b['b'][0])
                or max(b['a'][0],b['b'][0])+padding < min(a['a'][0],a['b'][0])
                or max(a['a'][1],a['b'][1])+padding < min(b['a'][1],b['b'][1])
                or max(b['a'][1],b['b'][1])+padding < min(a['a'][1],a['b'][1])): continue
            distance=segment_distance(a['a'],a['b'],b['a'],b['b'])-a['r']-b['r']
            if a['net']==b['net']:
                if distance<=1e-6: parents[find(i)]=find(j)
            else:
                minimum=min(minimum,distance)
                check(distance >= board['clearance_mm']-1e-6,
                      f'Clearance {distance:.3f}mm: {a["label"]}({a["net"]}) / {b["label"]}({b["net"]})')
    roots={}
    for i,p in enumerate(primitives): roots.setdefault(p['net'],set()).add(find(i))
    for net,components in roots.items(): check(len(components)==1, f'Unrouted {net}: {len(components)} islands')
    pad_by_ref={(p['ref'],p['pin']):p for p in board['pads']}
    for i in range(count):
        expected=[f'+5V_ROW{i//16+1}',f'DATA{i}','GND',f'DATA{i+1}']
        check([pad_by_ref[(f'P{i+1}',p)]['net'] for p in range(1,5)]==expected, f'Pixel {i} wrong pinout')
    # Verify source regeneration exactly matches committed production files.
    import generate
    generated_layout,generated_board=generate.model()
    check(board==generated_board and layout==generated_layout, 'Generated design differs from source/inventory')
    check((ROOT/'fabrication/singapore-mrt.kicad_pcb').read_text()==generate.kicad(board,layout), 'KiCad source mismatch')
    for layer,suffix in [('F.Cu','F_Cu'),('B.Cu','B_Cu'),('F.Mask','F_Mask'),('B.Mask','B_Mask'),('Edge.Cuts','Edge_Cuts')]:
        content=(ROOT/f'fabrication/gerbers/singapore-mrt-{suffix}.gbr').read_text()
        check(content==generate.gerber(board,layer), f'Gerber mismatch {layer}')
        check(content.startswith('G04') and content.rstrip().endswith('M02*'),f'Incomplete Gerber {layer}')
        flashes=re.findall(r'X(\d+)Y(\d+)D03\*',content)
        if layer.endswith('Cu') or layer.endswith('Mask'):
            expected=sorted((round(p['x']*1e6),round((board['height_mm']-p['y'])*1e6)) for p in board['pads'])
            check(sorted((int(x),int(y)) for x,y in flashes)==expected,f'Wrong Gerber pad flashes {layer}')
    for plated,suffix in [(True,'PTH'),(False,'NPTH')]:
        check((ROOT/f'fabrication/gerbers/singapore-mrt-{suffix}.drl').read_text()==generate.drill(board,plated),f'Drill mismatch {suffix}')
    check((ROOT/'fabrication/gerbers/singapore-mrt-F_Silkscreen.gbr').read_text()==generate.silkscreen(board,layout),'Silkscreen mismatch')
    with zipfile.ZipFile(ROOT/'fabrication/singapore-mrt-gerbers.zip') as archive:
        expected={p.name:p.read_bytes() for p in (ROOT/'fabrication/gerbers').iterdir() if p.is_file()}
        check(set(archive.namelist())==set(expected)|{'FABRICATION-NOTES.txt'},'Unexpected fabrication ZIP entries')
        for name,payload in expected.items(): check(archive.read(name)==payload,f'ZIP payload mismatch {name}')
        check(all(i.date_time==(1980,1,1,0,0,0) for i in archive.infolist()),'Non-deterministic ZIP timestamp')
    report=dict(result='FAIL' if errors else 'PASS',stations=count,rows=board['rows'],
                pcb_mm=[board['width_mm'],board['height_mm']],pads=len(board['pads']),
                tracks=len(board['tracks']),connected_nets=len(roots),
                clearance_rule_mm=board['clearance_mm'],
                checks=['inventory alias coverage','contiguous pixels','pad annular rings','copper-to-edge',
                        'copper-to-mounting-hole','same-layer copper clearance','all net connectivity',
                        'pixel daisy-chain pinout','deterministic KiCad/Gerber/drill parity','Gerber pad flashes','fabrication ZIP parity'],
                not_verified=['KiCad parser/ERC/DRC (kicad-cli unavailable)','manufacturer CAM acceptance',
                              'physical assembly/fit','power/thermal measurements','signal integrity'],errors=errors)
    (ROOT/'fabrication/validation.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))
    return 1 if errors else 0


if __name__=='__main__':
    sys.exit(validate())
