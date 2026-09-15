#!/usr/bin/env python3
"""Check actual cut-file registration and STL closed edges without CAD libraries."""
from collections import Counter
import json
from pathlib import Path
import re
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
NS = '{http://www.w3.org/2000/svg}'


def circles(path):
    root = ET.parse(ROOT/path).getroot()
    return {(float(c.attrib['cx']),float(c.attrib['cy']),float(c.attrib['r']))
            for c in root.iter(NS+'circle')}


def mesh(path):
    vertices=[tuple(float(v) for v in match) for match in re.findall(
        r'vertex\s+([-\d.e+]+)\s+([-\d.e+]+)\s+([-\d.e+]+)',(ROOT/path).read_text())]
    assert vertices and len(vertices)%3==0, f'Invalid ASCII STL: {path}'
    edges=Counter()
    for i in range(0,len(vertices),3):
        a,b,c=vertices[i:i+3]
        for e in [(a,b),(b,c),(c,a)]: edges[tuple(sorted(e))]+=1
    assert all(n==2 for n in edges.values()), f'Non-closed mesh: {path}'
    return {'file':str(path),'triangles':len(vertices)//3,
            'closed_edges':True,
            'min_mm':[min(v[i] for v in vertices) for i in range(3)],
            'max_mm':[max(v[i] for v in vertices) for i in range(3)]}


def main():
    layout=json.loads((ROOT/'layouts/singapore-mrt.json').read_text())
    board=json.loads((ROOT/'fabrication/board-model.json').read_text())
    apertures={(p['x_mm']+10,p['y_mm']+24,2.75) for p in layout['pixels']}
    front=circles('mechanical/led-front-panel-cut.svg')
    assert {p for p in front if p[2]==2.75}==apertures, 'LED aperture registration mismatch'
    back=circles('mechanical/led-back-panel-cut.svg')
    assert {(x+10,y+24,1.65) for x,y in board['mounting_holes']} <= back, 'PCB mounting hole registration mismatch'
    assert {p for p in front if p[2]==1.65} <= back, 'Front/back spacer mismatch'
    epd=circles('mechanical/epaper-front-panel-cut.svg')
    assert epd=={(x,y,1.65) for x in [5,183] for y in [5,131]}, 'E-paper boss registration mismatch'
    meshes=[mesh(Path('mechanical')/p) for p in ['led-spacers-and-cups.stl','epaper-shell.stl','epaper-front.stl','epaper-electronics-mount.stl']]
    expected={'epaper-shell.stl':[188,136,28],'epaper-front.stl':[188,136,3],
              'epaper-electronics-mount.stl':[90,48,2]}
    for m in meshes:
        name=Path(m['file']).name
        if name in expected:
            assert m['min_mm']==[0,0,0] and m['max_mm']==expected[name], f'Wrong envelope {name}'
    report=dict(result='PASS',led_apertures=len(apertures),pcb_mounting_holes=len(board['mounting_holes']),
                front_back_registration=True,epaper_boss_registration=True,meshes=meshes,
                not_verified=['Physical fit','printer shrinkage','laser kerf','purchased component tolerances'])
    (ROOT/'fabrication/mechanical-validation.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))


if __name__=='__main__': main()
