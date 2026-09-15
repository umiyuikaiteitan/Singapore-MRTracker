#!/usr/bin/env python3
"""Rebuild the passive all-station MRT board, artwork, and fabrication CAM using only Python.

The same millimetre model drives KiCad, Gerber, Excellon, SVG, and the independent
geometry/connectivity checks in validate.py. This is prototype engineering data;
manufacturer CAM review and a first article remain necessary.
"""
from __future__ import annotations

import csv
import io
import json
from pathlib import Path
import zipfile

ROOT = Path(__file__).resolve().parents[1]
WIDTH = 400.0
PITCH = 24.0
COLUMNS = 16
COLORS = {"NSL": "#d9282e", "EWL": "#008d5e", "NEL": "#8d2790",
          "CCL": "#ef9d13", "DTL": "#0077b5", "TEL": "#9d6337"}


def write(path, content):
    p = ROOT / path
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(content, encoding="utf-8")


def model():
    inventory = json.loads((ROOT / 'network-inventory.json').read_text())
    stations = inventory['stations']
    pixels = []
    for i, station in enumerate(stations):
        row, col = divmod(i, COLUMNS)
        pixels.append(dict(index=i, station=station['station'],
                           station_aliases=[c for c in station['codes'] if c != station['station']],
                           line=None, x_mm=float(20 + PITCH * (col if row % 2 == 0 else 15-col)),
                           y_mm=float(20 + PITCH * row)))
    count, rows = len(pixels), (len(pixels)+COLUMNS-1)//COLUMNS
    layout = dict(schema_version=1, layout_id="singapore-mrt-v1", led_count=count,
                  board_station=stations[0]['station'], pixels=pixels)
    board = dict(width_mm=WIDTH, height_mm=40+PITCH*(rows-1), rows=rows,
                 pad_diameter_mm=1.8, drill_mm=1.0, clearance_mm=0.25,
                 pads=[], tracks=[], mounting_holes=[])
    height = board['height_mm']
    board['mounting_holes'] = [[4,4],[WIDTH-4,4],[4,height-4],[WIDTH-4,height-4],
                              [WIDTH/2,4],[WIDTH/2,height-4]]
    def pad(ref, number, x, y, net, diameter=1.8, hole=1.0):
        board['pads'].append(dict(ref=ref, pin=number, x=round(x,4), y=round(y,4),
                                 net=net, diameter=diameter, drill=hole))
    def wire(net, layer, width, *points):
        for a,b in zip(points,points[1:]):
            if a != b:
                board['tracks'].append(dict(net=net,layer=layer,width=width,
                    start=[round(v,4) for v in a],end=[round(v,4) for v in b]))
    for p in pixels:
        i,x,y = p['index'],p['x_mm'],p['y_mm']
        row=i//COLUMNS
        direction=1 if row%2==0 else -1
        for pin,dx,net in [(1,-3.81,f"+5V_ROW{row+1}"),(2,-1.27,f"DATA{i}"),
                           (3,1.27,"GND"),(4,3.81,f"DATA{i+1}")]:
            pad(f"P{i+1}",pin,x+dx*direction,y,net)
        wire(f"+5V_ROW{row+1}","B.Cu",1.5,(x-3.81*direction,y),(x-3.81*direction,y-8))
        wire("GND","B.Cu",1.5,(x+1.27*direction,y),(x+1.27*direction,y+8))
    for row in range(rows):
        y=20+PITCH*row
        row_pixels=pixels[row*COLUMNS:(row+1)*COLUMNS]
        direction=1 if row%2==0 else -1
        min_power=min(p['x_mm']-3.81*direction for p in row_pixels)
        pad(f"J_PWR{row+1}",1,394,y-8,f"+5V_ROW{row+1}",2.6,1.3)
        pad(f"J_PWR{row+1}",2,394,y-2.92,"GND",2.6,1.3)
        wire(f"+5V_ROW{row+1}","B.Cu",1.5,(min_power,y-8),(394,y-8))
        wire("GND","B.Cu",1.5,(10,y+8),(394,y+8),(394,y-2.92))
    wire("GND","B.Cu",1.5,(10,28),(10,height-12))
    pad("J_DATA_IN",1,8,20,"DATA0")
    pad("J_DATA_IN",2,8,22.54,"GND")
    wire("GND","B.Cu",1.5,(8,22.54),(8,28),(10,28))
    wire("DATA0","F.Cu",.4,(8,20),(12,20),(12,24),(18.73,24),(18.73,20))
    for i in range(count-1):
        a,b=pixels[i],pixels[i+1]
        d=1 if (i//COLUMNS)%2==0 else -1
        x1,x2,y=a['x_mm']+3.81*d,b['x_mm']-1.27*(d if i%COLUMNS!=15 else -d),a['y_mm']
        if i%COLUMNS==15:
            fold=390 if d==1 else 6
            wire(f"DATA{i+1}","F.Cu",.4,(x1,y),(fold,y),(fold,b['y_mm']-4),
                 (x2,b['y_mm']-4),(x2,b['y_mm']))
        else:
            wire(f"DATA{i+1}","F.Cu",.4,(x1,y),(x1,y+4),(x2,y+4),(x2,y))
    last=pixels[-1]
    d=1 if ((count-1)//COLUMNS)%2==0 else -1
    endx=last['x_mm']+3.81*d
    pad("J_DATA_OUT",1,390,height-6,f"DATA{count}")
    pad("J_DATA_OUT",2,392.54,height-6,"GND")
    wire(f"DATA{count}","F.Cu",.4,(endx,last['y_mm']),(endx,height-6),(390,height-6))
    wire("GND","B.Cu",1.5,(392.54,height-6),(392.54,height-12))
    return layout,board


def kicad(board, layout):
    nets = {name: i+1 for i, name in enumerate(sorted({p["net"] for p in board["pads"]}))}
    lines = ['(kicad_pcb (version 20221018) (generator pcbnew)',
             '  (general (thickness 1.6))', '  (paper "A4")',
             '  (layers (0 "F.Cu" signal) (31 "B.Cu" signal)',
             '    (36 "B.SilkS" user "b.silkscreen") (37 "F.SilkS" user "f.silkscreen")',
             '    (38 "B.Mask" user) (39 "F.Mask" user) (44 "Edge.Cuts" user))',
             '  (setup (pad_to_mask_clearance 0.1))', '  (net 0 "")']
    lines += [f'  (net {n} "{name}")' for name, n in nets.items()]
    groups = {}
    for p in board["pads"]:
        groups.setdefault(p["ref"], []).append(p)
    for ref, pads in groups.items():
        cx, cy = pads[0]["x"], pads[0]["y"]
        lines += [f'  (footprint "MRTracker:Wired_Header_{len(pads)}" (layer "F.Cu") (at {cx} {cy})',
                  '    (attr through_hole)',
                  f'    (fp_text reference "{ref}" (at 0 -3) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))',
                  f'    (fp_text value "Wired_Header_{len(pads)}" (at 0 3) (layer "F.SilkS") hide (effects (font (size 1 1) (thickness 0.15))))']
        for p in pads:
            # Every physical pad is circular, including pin 1; its ring marker
            # and the assembly map identify it consistently in KiCad and CAM.
            lines.append(f'    (pad "{p["pin"]}" thru_hole circle (at {p["x"]-cx:.4f} {p["y"]-cy:.4f}) (size {p["diameter"]} {p["diameter"]}) (drill {p["drill"]}) (layers "*.Cu" "*.Mask") (net {nets[p["net"]]} "{p["net"]}"))')
        lines.append('  )')
    for i, (x, y) in enumerate(board["mounting_holes"]):
        lines += [f'  (footprint "MRTracker:MountingHole_3.2" (layer "F.Cu") (at {x} {y})',
                  '    (attr through_hole)',
                  f'    (fp_text reference "H{i+1}" (at 0 0) (layer "F.SilkS") hide (effects (font (size 1 1) (thickness 0.15))))',
                  '    (pad "" np_thru_hole circle (at 0 0) (size 3.2 3.2) (drill 3.2) (layers "*.Cu" "*.Mask"))', '  )']
    for t in board["tracks"]:
        a, b = t["start"], t["end"]
        lines.append(f'  (segment (start {a[0]} {a[1]}) (end {b[0]} {b[1]}) (width {t["width"]}) (layer "{t["layer"]}") (net {nets[t["net"]]}))')
    w,h=board["width_mm"],board["height_mm"]
    for a, b in [((0,0),(w,0)),((w,0),(w,h)),((w,h),(0,h)),((0,h),(0,0))]:
        lines.append(f'  (gr_line (start {a[0]} {a[1]}) (end {b[0]} {b[1]}) (stroke (width 0.05) (type default)) (layer "Edge.Cuts"))')
    for p in layout["pixels"]:
        text = f'{p["station"]} / {p["index"]:02d}'
        lines.append(f'  (gr_text "{text}" (at {p["x_mm"]} {p["y_mm"]-4}) (layer "F.SilkS") (effects (font (size 1.2 1.2) (thickness 0.15))))')
    lines.append('  (gr_text "SINGAPORE MRT V1 - 5V PER ROW - SCHEMATIC STATION MATRIX" (at 200 251) (layer "F.SilkS") (effects (font (size 1.2 1.2) (thickness 0.15))))')
    lines.append(')')
    return '\n'.join(lines) + '\n'


def gerber(board, layer):
    # RS-274X, absolute 4.6 coordinates in millimetres. CAM Y is inverted so a
    # viewer with conventional +Y up shows the same top view as KiCad and SVG.
    functions = {"F.Cu":"Copper,L1,Top", "B.Cu":"Copper,L2,Bot",
                 "F.Mask":"Soldermask,Top", "B.Mask":"Soldermask,Bot",
                 "Edge.Cuts":"Profile,NP"}
    out = ['G04 Singapore MRT v1 - generated prototype CAM*', '%FSLAX46Y46*%',
           '%MOMM*%', f'%TF.FileFunction,{functions[layer]}*%',
           '%LPD*%', '%ADD10C,1.800*%', '%ADD11C,0.400*%',
           '%ADD12C,1.500*%', '%ADD13C,2.000*%', '%ADD14C,0.050*%',
           '%ADD15C,2.600*%', '%ADD16C,2.800*%']
    def xy(x, y): return f'X{round(x*1e6):010d}Y{round((board["height_mm"]-y)*1e6):010d}'
    if layer.endswith("Cu") or layer.endswith("Mask"):
        out.append('D10*' if layer.endswith("Cu") else 'D13*')
        for p in board["pads"]:
            out.append(('D15*' if layer.endswith('Cu') else 'D16*') if p['diameter']>2 else ('D10*' if layer.endswith('Cu') else 'D13*'))
            out.append(xy(p["x"], p["y"]) + 'D03*')
    if layer.endswith("Cu"):
        for t in board["tracks"]:
            if t["layer"] == layer:
                out.extend(['D11*' if t["width"] == .4 else 'D12*',
                            xy(*t["start"]) + 'D02*', xy(*t["end"]) + 'D01*'])
    if layer == "Edge.Cuts":
        w,h=board['width_mm'],board['height_mm']
        out += ['D14*', xy(0,0)+'D02*', xy(w,0)+'D01*',
                xy(w,h)+'D01*', xy(0,h)+'D01*', xy(0,0)+'D01*']
    return '\n'.join(out + ['M02*']) + '\n'


def drill(board, plated):
    out=['M48',';TYPE='+('PLATED' if plated else 'NON_PLATED'),'METRIC']
    sizes=sorted({p['drill'] for p in board['pads']}) if plated else [3.2]
    out += [f'T{i+1:02d}C{d:.3f}' for i,d in enumerate(sizes)]
    out += ['%','G90']
    for i,size in enumerate(sizes):
        out.append(f'T{i+1:02d}')
        coords=[(p['x'],p['y']) for p in board['pads'] if p['drill']==size] if plated else board['mounting_holes']
        out += [f'X{x:.4f}Y{board["height_mm"]-y:.4f}' for x,y in coords]
    return '\n'.join(out+['M30'])+'\n'


def silkscreen(board, layout):
    """A small original line font: reproducible manufacturer-readable codes."""
    font = {
        '0':['00204042220200'], '1':['102022','0242'],
        '2':['00204041210242'], '3':['002040222040422202'],
        '4':['000222','4042'], '5':['40000222404202'],
        '6':['400002024242222002'], '7':['00402202'],
        '8':['00204042220200222240'], '9':['424000022240'],
        'N':['02004042'],'S':['40000222404202'],
        'E':['40000242','0022'],'W':['000212224240'],
        'C':['40000242'],'G':['400002424122'],
        'D':['02003040423002'],'T':['0040','2022'],
        '/':['0240'],'P':['0200402220'],'J':['40420200'],
        'I':['0040','2022','0242'],'A':['02004042','0121'],
        'O':['00204042220200'],'U':['00024240'],
        'R':['0200402220','2042'],'+':['0121','1012'],
        '-':['0121'],'V':['00224240'],
    }
    out=['G04 Singapore MRT v1 front silkscreen*','%FSLAX46Y46*%','%MOMM*%',
         '%TF.FileFunction,Legend,Top*%','%LPD*%','%ADD10C,0.150*%','D10*']
    def xy(x,y): return f'X{round(x*1e6):010d}Y{round((board["height_mm"]-y)*1e6):010d}'
    def label(text,x,y,size=1.7):
        pitch=size*.8
        start=x-(len(text)*pitch-pitch*.2)/2
        for i,char in enumerate(text):
            for path in font.get(char,[]):
                pts=[(int(path[j]),int(path[j+1])) for j in range(0,len(path),2)]
                for j,(px,py) in enumerate(pts):
                    out.append(xy(start+i*pitch+px*size*.15,y+py*size*.5)+('D02*' if j==0 else 'D01*'))
    for p in layout['pixels']:
        label('/'.join([p['station']]+p.get('station_aliases',[])),p['x_mm'],p['y_mm']-6,1.3)
        label(f'P{p["index"]+1}',p['x_mm'],p['y_mm']+10,1.4)
    for row in range(board['rows']):
        label(f'+5V {row+1}',389,20+PITCH*row-11,1.3)
    for p in board['pads']:
        if p['pin']==1:
            import math
            radius=p['diameter']/2+.45
            for i in range(25):
                angle=i*math.tau/24
                out.append(xy(p['x']+radius*math.cos(angle),p['y']+radius*math.sin(angle))+('D02*' if i==0 else 'D01*'))
    return '\n'.join(out+['M02*'])+'\n'


def svg_begin(width, height, title):
    return [f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}mm" height="{height}mm" viewBox="0 0 {width} {height}">',
            f'<title>{title}</title>', '<style>text {font-family: sans-serif}</style>']


def assembly(board, layout):
    inventory=json.loads((ROOT/'network-inventory.json').read_text())
    w,h=board['width_mm'],board['height_mm']
    out=svg_begin(w+20,h+45,'Singapore MRT all-station carrier: top assembly and copper map')
    out += [f'<rect width="{w+20}" height="{h+45}" fill="#f7faf8"/>',
            '<text x="10" y="9" font-size="5" font-weight="bold">SINGAPORE MRT · ALL STATIONS · ROUTED PCB TOP VIEW</text>',
            '<g transform="translate(10,15)">',
            f'<rect width="{w}" height="{h}" rx="1" fill="#113d31" stroke="#082a20"/>']
    for t in board['tracks']:
        a,b=t['start'],t['end']
        color='#94cceb' if t['layer']=='B.Cu' else '#e9bc64'
        out.append(f'<path d="M{a[0]},{a[1]} L{b[0]},{b[1]}" fill="none" stroke="{color}" stroke-width="{t["width"]}" opacity=".55"/>')
    for p in board['pads']:
        out.append(f'<circle cx="{p["x"]}" cy="{p["y"]}" r="{p["diameter"]/2}" fill="#dabe7e"/><circle cx="{p["x"]}" cy="{p["y"]}" r="{p["drill"]/2}" fill="#101c19"/>')
        if p['pin']==1:
            out.append(f'<circle cx="{p["x"]}" cy="{p["y"]}" r="1.6" fill="none" stroke="white" stroke-width=".15"/>')
    for x,y in board['mounting_holes']:
        out.append(f'<circle cx="{x}" cy="{y}" r="1.6" fill="#f7faf8"/>')
    for p,st in zip(layout['pixels'],inventory['stations']):
        i,x,y=p['index'],p['x_mm'],p['y_mm']
        codes='/'.join(st['codes'])
        out.append(f'<text x="{x}" y="{y-4}" fill="white" font-size="2.1" text-anchor="middle">{codes}</text>')
        out.append(f'<text x="{x}" y="{y+10.2}" fill="white" font-size="2" text-anchor="middle">P{i+1} · LED {i:03d}</text>')
    out += ['</g>',f'<text x="10" y="{h+23}" font-size="3.2">Every pixel socket: 1 = row +5V · 2 = DIN · 3 = GND · 4 = DOUT. Pin 1 ringed; odd rows reversed.</text>',
            f'<text x="10" y="{h+30}" font-size="3.2">J_PWR1–{board["rows"]}: upper pin +5V, lower GND. Individually fused row feeds. DATA headers: pin1 DATA, pin2 GND.</text>',
            f'<text x="10" y="{h+37}" font-size="3.2">Blue = rear copper · Gold = front data · Position follows inventory order; this is a schematic station matrix.</text>','</svg>']
    return '\n'.join(out)+'\n'


def panels(board,layout):
    from html import escape
    inventory=json.loads((ROOT/'network-inventory.json').read_text())
    w,h=board['width_mm']+20,board['height_mm']+42
    out=svg_begin(w,h,'Singapore MRT LED-only front panel: red cut; colored artwork is printed overlay')
    out += ['<g id="CUT" fill="none" stroke="#ff0000" stroke-width="0.01">',
            f'<rect x="0.05" y="0.05" width="{w-.1}" height="{h-.1}" rx="3"/>']
    for x,y in [(6,6),(w-6,6),(6,h-6),(w-6,h-6),(w/2,6),(w/2,h-6)]:
        out.append(f'<circle cx="{x}" cy="{y}" r="1.65"/>')
    for p in layout['pixels']:
        out.append(f'<circle cx="{p["x_mm"]+10}" cy="{p["y_mm"]+24}" r="2.75"/>')
    out += ['</g>','</svg>']
    write('mechanical/led-front-panel-cut.svg','\n'.join(out)+'\n')
    art=svg_begin(w,h,'Singapore MRT full-network station matrix face artwork')
    art += [f'<rect width="{w}" height="{h}" rx="3" fill="#111c28"/>',
            '<text x="14" y="14" fill="white" font-size="6" font-weight="bold">SINGAPORE MRT</text>',
            '<text x="407" y="13" fill="#a5b7c7" font-size="3.1" text-anchor="end">DEPARTURE ACTIVITY · SCHEMATIC MATRIX</text>']
    for p,st in zip(layout['pixels'],inventory['stations']):
        x,y=p['x_mm']+10,p['y_mm']+24
        lines=st['lines']
        for j,line in enumerate(lines):
            color=COLORS.get(line,'#ffffff')
            art.append(f'<rect x="{x-10+j*20/len(lines)}" y="{y-12}" width="{20/len(lines)}" height="1.5" fill="{color}"/>')
        art.append(f'<circle cx="{x}" cy="{y}" r="3.8" fill="none" stroke="#70879d" stroke-width=".4"/><circle cx="{x}" cy="{y}" r="2.75" fill="#fff"/>')
        art.append(f'<text x="{x}" y="{y-7.5}" fill="#ffffff" font-size="2.05" text-anchor="middle">{escape("/".join(st["codes"]))}</text>')
        # Split names at words, avoiding tiny forced single-line station labels.
        words=st['name'].split(); name_lines=[]; current=''
        for word in words:
            if len(current)+len(word)+1>17 and current:
                name_lines.append(current);current=word
            else: current=(current+' '+word).strip()
        name_lines.append(current)
        for j,txt in enumerate(name_lines):
            art.append(f'<text x="{x}" y="{y+6+j*2.6}" fill="#d7e4ee" font-size="2.35" text-anchor="middle">{escape(txt)}</text>')
    for i,(line,color) in enumerate(COLORS.items()):
        x=20+i*49
        art.append(f'<rect x="{x}" y="{h-11}" width="9" height="2" fill="{color}"/><text x="{x+12}" y="{h-8.8}" fill="white" font-size="3">{line}</text>')
    art += [f'<text x="20" y="{h-36}" fill="#ffffff" font-size="3.2" font-weight="bold">STATION DEPARTURE ACTIVITY</text>',
            f'<text x="20" y="{h-29}" fill="#a5b7c7" font-size="3">Dim: scheduled departure · Brighter: fresh prediction · Offline: lights blank</text>',
            f'<text x="20" y="{h-23}" fill="#a5b7c7" font-size="2.6">Lights show departures at stations. They do not indicate measured train positions.</text>']
    art += [f'<text x="{w-13}" y="{h-8.8}" fill="#a5b7c7" font-size="2.6" text-anchor="end">{len(layout["pixels"])} stations · LED 000–{len(layout["pixels"])-1:03d}</text>','</svg>']
    write('mechanical/led-front-artwork.svg','\n'.join(art)+'\n')
    back=svg_begin(w,h,'LED back panel: 1:1 mm red cut; blue hole labels')
    back += ['<g fill="none" stroke="#ff0000" stroke-width="0.01">',
             f'<rect x="0.05" y="0.05" width="{w-.1}" height="{h-.1}" rx="3"/>']
    for x,y in [(6,6),(w-6,6),(6,h-6),(w-6,h-6),(w/2,6),(w/2,h-6)]:
        back.append(f'<circle cx="{x}" cy="{y}" r="1.65"/>')
    for x,y in board['mounting_holes']:
        back.append(f'<circle cx="{x+10}" cy="{y+24}" r="1.65"/>')
    back += [f'<circle cx="{w-60}" cy="{h-15}" r="6"/>','</g>','</svg>']
    write('mechanical/led-back-panel-cut.svg','\n'.join(back)+'\n')
    epaper=svg_begin(188,136,'Separate e-paper bezel: red cut, units mm')
    epaper += ['<g fill="none" stroke="#ff0000" stroke-width="0.01">',
               '<rect x="0.05" y="0.05" width="187.9" height="135.9"/>',
               '<rect x="11.9" y="18.5" width="164.2" height="99"/>']
    for x in [5,183]:
        for y in [5,131]: epaper.append(f'<circle cx="{x}" cy="{y}" r="1.65"/>')
    epaper += ['</g>','</svg>']
    write('mechanical/epaper-front-panel-cut.svg','\n'.join(epaper)+'\n')



def bom(board,layout):
    rows = [
        ['PCB1',1,'Singapore MRT v1 passive LED carrier','400x256mm; FR4 1.6mm; 2 layers; 2oz copper','Generated CAM; all146 station modules mount over this carrier',''],
        ['P1-P146',layout['led_count'],'1x4 2.54mm female header OR soldered pigtail','1mm finished hole; 1.8mm pad','Pin1 row+5V;2 DIN;3 GND;4 DOUT',''],
        ['J_PWR1-J_PWR10',board['rows'],'2-position screw terminal 5.08mm pitch','1.3mm hole; >=3A/contact; <=10mm body width','Upper pin+5V; lowerGND; separate fused feed per row',''],
        ['J_DATA_IN,J_DATA_OUT',2,'1x2 2.54mm header','1mm hole','Pin1 DATA; pin2 GND; output optional',''],
        ['LED000-LED145',layout['led_count'],'Adafruit NeoPixel Mini Button PCB1612 or compatible RGB module','9.1x9.1mm nominal; 800kHz GRB; WS2812B/SK6812 RGB','30 packs of5; solder to MODULE PAD LABELS; notRGBW','https://www.adafruit.com/product/1612'],
        ['U1',1,'ESP32 DevKit with ESP32-WROOM-32','External controller; GPIO13 LED','3.3V IO; mount on insulated bracket in rear space',''],
        ['U2',1,'Texas Instruments SN74AHCT125N','PDIP14; external perfboard; 5V','1OE pin1GND;1A pin2GPIO13;1Y pin3output','https://www.ti.com/product/SN74AHCT125'],
        ['R1',1,'330ohm series resistor','0.25W; external close to first pixel','U2pin3 to J_DATA_INpin1',''],
        ['R2',1,'100kilohm pulldown','0.25W; external','U2pin2 toGND',''],
        ['C1-C10',board['rows'],'1000uF electrolytic','10V minimum; external across each J_PWR','Observe polarity; close to connector',''],
        ['C11',1,'100nF ceramic','X7R16V; external','Across U2pins14and7; short leads',''],
        ['F1-F10',board['rows'],'Inline fuse and holder','1.5A DC rated','One per row +5V wire, close to distribution block',''],
        ['F11',1,'Controller branch fuse','1A DC rated','ESP32 USB/power branch separate from LED rows',''],
        ['PSU1',1,'Regulated isolated external 5V supply','12A continuous minimum with enclosed low-voltage distribution','MaximumLED estimate8.76A; no mains wiring inside display',''],
        ['DIST1',1,'Insulated power distribution block','12A minimum; one+5V and oneGND bank','Ten paired row feeds; main16AWG, row22AWG',''],
        ['MECH1',1,'Opaque front and back sheet set','420x298x3mm; acrylic or plywood','Cut SVG paths; print separate station overlay',''],
        ['MECH2',6,'Printed 34mm panel spacers','M3 through-hole; 10mm outside diameter','M3x45 bolts and nuts through both panels',''],
        ['MECH3',layout['led_count'],'Printed pixel cups','10.2mm pocket; 12.6mm outside','Trial fit module first; foam tape to front rear',''],
        ['MECH4',6,'M3 standoffs for carrier','12mm female-female metal or printed equivalent','SixPCB mountingholes; M3x8 screws',''],
        ['MECH5',1,'Diffuser film / opaque adhesive overlay','Cut locally','Use led-front-artwork.svg printed at100percent',''],
        ['WIRE',1,'Insulated wire and strain relief','16AWG trunk;22AWG row power;26AWG data/pixels','All grounds paired and connected; short data pigtails',''],
    ]
    buff=io.StringIO(newline='')
    writer=csv.writer(buff,lineterminator='\n')
    writer.writerow(['reference','quantity','part','specification','assembly_notes','source'])
    writer.writerows(rows)
    return buff.getvalue()


def main():
    layout, board = model()
    write('layouts/singapore-mrt.json', json.dumps(layout, indent=2)+'\n')
    write('fabrication/board-model.json', json.dumps(board, indent=2)+'\n')
    write('fabrication/singapore-mrt.kicad_pcb', kicad(board, layout))
    for layer, suffix in [('F.Cu','F_Cu'),('B.Cu','B_Cu'),('F.Mask','F_Mask'),
                           ('B.Mask','B_Mask'),('Edge.Cuts','Edge_Cuts')]:
        write(f'fabrication/gerbers/singapore-mrt-{suffix}.gbr', gerber(board, layer))
    write('fabrication/gerbers/singapore-mrt-PTH.drl', drill(board, True))
    write('fabrication/gerbers/singapore-mrt-NPTH.drl', drill(board, False))
    write('fabrication/gerbers/singapore-mrt-F_Silkscreen.gbr', silkscreen(board, layout))
    write('docs/assembly-map.svg', assembly(board, layout))
    write('led-bom.csv', bom(board,layout))
    panels(board,layout)
    # Exact timestamp, order, permissions and payload make the upload archive
    # repeatable. The separate CAD/JSON sources remain editable in the repo.
    archive=ROOT/'fabrication/singapore-mrt-gerbers.zip'
    notes=(f'SINGAPORE MRT V1 - PROTOTYPE FABRICATION DATA\n'
           f'Board: {board["width_mm"]:g} x {board["height_mm"]:g} mm, FR4 1.6mm, 2 layers, 2oz copper.\n'
           'Six Gerbers: front/rear copper, front/rear mask, front legend, outline.\n'
           'Separate PTH and NPTH Excellon drills; all files share the CAM origin.\n'
           'PTH: 1.0mm and 1.3mm finished holes. NPTH: six 3.2mm mounting holes.\n'
           'Data tracks 0.4mm; power tracks 1.5mm; minimum checked copper clearance0.25mm.\n'
           'No paste layer. Hand-wired through-hole RGB module carrier.\n'
           'Geometry/connectivity checks passed; KiCad DRC and manufacturer CAM review pending.\n'
           'No physical first article has been fabricated or tested. Review before ordering.\n')
    entries={p.name:p.read_bytes() for p in (ROOT/'fabrication/gerbers').iterdir() if p.is_file()}
    entries['FABRICATION-NOTES.txt']=notes.encode('utf-8')
    with zipfile.ZipFile(archive,'w',compression=zipfile.ZIP_DEFLATED,compresslevel=9) as output:
        for name,data in sorted(entries.items()):
            info=zipfile.ZipInfo(name,date_time=(1980,1,1,0,0,0))
            info.compress_type=zipfile.ZIP_DEFLATED
            info.create_system=3
            info.external_attr=0o100644<<16
            output.writestr(info,data,compress_type=zipfile.ZIP_DEFLATED,compresslevel=9)
    print('Generated KiCad PCB, 6 Gerbers, 2 drills, layout, BOM and assembly/cut SVGs.')


if __name__ == '__main__':
    main()
