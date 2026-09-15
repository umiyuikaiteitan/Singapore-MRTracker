# Singapore MRT physical display build package

This package contains **two separate devices**:

1. A 400 × 256 mm circuit board for **all 146 operating MRT stations**, with one addressable RGB station light per physical station and shared aliases for interchanges. It has 146 populated pixel locations, not a 16-station sample. Each station uses a small WS2812B/SK6812 **RGB module** wired to its carrier socket. The 420 × 298 mm front artwork is an intentional schematic station matrix, grouped by the first MRT line serving each station; it is not a geographic transit map. Line badges identify interchanges. LRT is excluded.
2. An independent Waveshare 7.5-inch V2 800 × 480 black/white e-paper departure board with its own ESP32 and 188 × 136 mm case. It has no LED carrier or LED power supply inside it.

**Prototype status:** routed fabrication data, mechanically dimensioned sources, and automated geometry checks are included. No PCB has been ordered or assembled. `fabrication/validation.json` records what passed and what remains unverified. KiCad's parser/ERC/DRC and a manufacturer's CAM review have not run in this environment; neither electrical performance nor enclosure fit has been measured. Review the CAM and build one first article before ordering a batch.

## What to open

| File | Purpose |
| --- | --- |
| `layouts/singapore-mrt.json` | Stable firmware/publication mapping, indices 0–145, all current and legacy interchange aliases |
| `network-inventory.json` / `NETWORK-COVERAGE.md` | Dated official station coverage and current line paths |
| `fabrication/singapore-mrt.kicad_pcb` | Editable self-contained two-layer PCB source; all footprints embedded |
| `fabrication/gerbers/` | Six Gerbers plus separate plated/non-plated Excellon drills |
| `fabrication/singapore-mrt-gerbers.zip` | Deterministic prototype fabrication upload archive with the same CAM and fabrication notes |
| `docs/assembly-map.svg` | Exact top-view PCB pad, copper, connector, station and index map |
| `docs/led-wiring.svg` | External controller, level shifting and power wiring |
| `docs/epaper-wiring.svg` | Independent e-paper wiring |
| `led-bom.csv` / `epaper-bom.csv` | Separate parts lists |
| `mechanical/led-front-panel-cut.svg` / `led-back-panel-cut.svg` | Exact-scale 3 mm sheet fabrication |
| `mechanical/led-front-artwork.svg` | Full station labels and line colors; print at 100% scale |
| `mechanical/epaper-front-panel-cut.svg` | Separate e-paper bezel |
| `mechanical/enclosures.scad` | Parametric pixel cups, spacers, e-paper shell/front and controller mount |
| `mechanical/*.stl` | Printable meshes exported from that source |

## LED circuit and immutable pinout

The carrier is deliberately a through-hole module design. It routes the 800 kHz data stream through all 146 modules; it does not assume an undocumented bare-LED package. An Adafruit 1612 pixel button is a concrete reference module. Equivalent modules must be **RGB**, 800 kHz GRB WS2812B/SK6812-compatible, and fit the printed cup. RGBW modules use a different wire protocol and do not work with this firmware.

| Connector | Pin | Signal |
| --- | --- | --- |
| P1–P146 | 1 | +5 V for that row |
| P1–P146 | 2 | Module DIN |
| P1–P146 | 3 | GND |
| P1–P146 | 4 | Module DOUT |
| J_DATA_IN | 1 / 2 | Shifted DATA0 / GND |
| J_DATA_OUT | 1 / 2 | DATA146 / GND; optional expansion, no exported power |
| J_PWR1–J_PWR10 | 1 / 2 | Row +5 V / GND; upper/lower in top view |

P1 corresponds to LED index 0. Each complete row has 16 stations; the final row has 2. Even row numbers when counted from zero run left-to-right; odd rows run right-to-left. Consequently the physical P-header pin order reverses on alternate rows. Pin 1 is ringed in the artwork. Always wire by pin number and the module's **printed pad labels**, not by wire color or a photograph. Module power and ground pad duplicates may use either electrically equivalent pair.

Each pixel button is held behind its own 5.5 mm light aperture. Four short insulated wires connect it to its P socket directly behind the aperture. The supplied design therefore makes an actual populated LED circuit-board assembly while retaining replaceable station modules. No assembly-house pick-and-place file is appropriate for this hand-wired version.

The controller connection is `ESP32 GPIO13 → SN74AHCT125 gate 1 → 330 Ω → J_DATA_IN pin 1`. U2 runs at 5 V; the ESP32 stays at 3.3 V logic. Supply U2 pin 14 with 5 V, pin 7 with GND, pin 1 with GND, pin 2 with GPIO13, and pin 3 with the resistor. Fit a 100 nF ceramic at pins 14/7 and a 100 kΩ pulldown at pin 2. Disable unused gates: pins 4/10/13 to +5 V, pins 5/9/12 to GND, leave outputs 6/8/11 unconnected. Keep the shifted data lead short, with an adjacent ground wire.

## Power distribution

Do not feed this entire board through a DevKit USB connector or one pixel header. Worst-case legacy RGB modules can use 60 mA each: **146 × 60 mA = 8.76 A at 5 V** before the controller. Use an enclosed isolated external regulated 5 V supply rated at least 12 A, with a suitable low-voltage distribution block. No mains wiring is part of this build.

Each row has its **own +5 V copper net** and its own paired supply and return wires. Fit one 1.5 A DC-rated fuse close to the distribution block for each of the ten row feeds. Fit a 1000 µF, ≥10 V capacitor across each J_PWR connector near the PCB. The negative lead goes to GND. Use approximately 16 AWG for the short supply trunk and 22 AWG paired row feeds; all terminals and the trunk must be rated for the total load. The 1.5 mm, 2 oz PCB row traces are intended for a maximum 16-pixel row, not the aggregate network load.

Connect **all ten ground returns**, including the final partial row, before powering any row. Grounds share a reference on the PCB, but the inter-row reference trace must not become the substitute return path for missing row wires. Power the ESP32 and AHCT from their own fused branch of the same supply, sharing ground. Use the DevKit's supported input path. Avoid connecting two powered USB/5 V sources simultaneously; development-board power-isolation arrangements vary.

Firmware defaults to a conservative brightness and a 1000 mA **whole-display estimate**. This is intentionally dim. Raise its budget only after measuring total current and the most distant row voltage under the intended pattern. The software limit is not a fuse and does not account precisely for every module revision. At first power-up use current limiting, verify each row's polarity, run a low-brightness single-pixel chase through all 146 indices, then test a moderate static pattern while checking connectors, rail voltage and temperature. A missing or reversed module interrupts all later pixels.

## Fabrication and assembly

Order one 400 × 256 mm, 1.6 mm FR4, two-layer board with 2 oz copper, solder mask both sides and front silkscreen. The CAM uses circular 1.8 mm pads / 1.0 mm finished drills for the pixel/data headers, 2.6 mm pads / 1.3 mm finished drills for power terminals, and six 3.2 mm non-plated mounting holes. Track widths are 0.4 mm data and 1.5 mm power/ground; the checked clearance is 0.25 mm. Large-board pricing and panel availability need a manufacturer quote. Gerber and drill origins match, with +Y up in CAM. The SVG and KiCad views use +Y down. No solder paste layer is needed.

The editable KiCad board and generated CAM share the same copper model. The front CAM uses a simple stroke font for station aliases and socket numbers; it is intentionally independent of installed system fonts. Open all layers and drills together in a CAM viewer before fabrication, check the 400 × 256 mm outline and hole registration, and have the board shop review plating and clearance requirements. KiCad source regeneration overwrites manual edits; change `scripts/generate.py` if you want reproducible routing changes.

Cut the LED front/back SVGs from 3 mm sheet at 100% scale. Red paths are cuts; artwork is a separate print, not a cut layer. No laser kerf compensation is embedded: the shop should apply its measured kerf outward for external edges and inward for holes. Check a 100 mm printed ruler against the machine's scale before cutting. Panel outline is 420 × 298 mm. Six 34 mm spacers plus M3×45 bolts and nuts make a 40 mm total-depth open-sided sandwich. Six 12 mm PCB standoffs mount the carrier to the rear sheet; PCB origin is 10 mm right and 24 mm down from the panel origin. The aligned six PCB holes are present in the back sheet.

Print cups and spacers in PETG at 0.2 mm layers; trial-fit **one** cup to the purchased pixel module and aperture first. Print cup copies as needed, attach their flat optical faces to the back of the front panel with foam tape, and use a translucent diffuser over each aperture. Insert the pixels from the rear; retain them without stressing their solder joints. Route short pigtails to the corresponding sockets and provide strain relief for the main cable through the 12 mm rear opening. The modular printed parts fit a normal desktop printer; neither a 420 mm printer nor a combined e-paper enclosure is required. The open-sided format permits inspection and cooling; add an independently designed side guard if the final installation requires one.

The e-paper case uses a separate 188 × 136 × 28 mm printed shell and 3 mm front bezel. Its 164.2 × 99 mm opening is slightly larger than the panel's specified 163.2 × 97.92 mm active area. The actual glass outline is 170.2 × 111.2 mm. Align the active image to the opening before applying low-pressure foam tape to the glass perimeter; the shell deliberately does not assume symmetric active-area margins or a particular HAT mounting-hole pattern. Keep the FPC unstressed. Use the universal perforated mount, insulating tape and cable ties for the DevKit and matching driver HAT. The exposed glass and cable still require a physical fit trial.

## E-paper electrical contract

Use the **matching driver HAT/module**, not the panel's bare 24-pin FPC. This device uses a separate ESP32-WROOM-32 DevKit; ESP32-WROVER boards may reserve GPIO16/17 for PSRAM. Power the DevKit through USB and the HAT from its 3.3 V rail so its SPI interface shares the ESP32 logic level. Confirm the purchased HAT revision supports this supply and its manual's reset/timing settings.

| HAT signal | ESP32 |
| --- | --- |
| VCC / GND | 3V3 / GND |
| DIN / CLK | GPIO23 / GPIO18 |
| CS / DC | GPIO5 / GPIO17 |
| RST / BUSY | GPIO16 / GPIO4 |

No MISO connection is used. Never connect a 5 V signal to an ESP32 input. GPIO5 is a strapping pin; the specified driver should not force its level at reset. Use short leads and check a cold boot with the actual module. The intended firmware driver is GxEPD2_750_T7 for the V2 black/white 800 × 480 panel; similarly named 640 × 384, three-color and other controller revisions are not interchangeable.

## Reproduce and validate

```sh
python3 hardware/scripts/generate.py
python3 hardware/scripts/validate.py
openscad -D 'part="led_parts"' -o hardware/mechanical/led-spacers-and-cups.stl hardware/mechanical/enclosures.scad
openscad -D 'part="epaper_shell"' -o hardware/mechanical/epaper-shell.stl hardware/mechanical/enclosures.scad
openscad -D 'part="epaper_front"' -o hardware/mechanical/epaper-front.stl hardware/mechanical/enclosures.scad
openscad -D 'part="epaper_mount"' -o hardware/mechanical/epaper-electronics-mount.stl hardware/mechanical/enclosures.scad
python3 hardware/scripts/validate_mechanical.py
```

Optional PNG previews use `node hardware/scripts/render-previews.js` with the `sharp` package installed. The SVG sources are the exact-scale originals. `fabrication/mechanical-validation.json` checks the 146 aperture positions, six PCB mounting holes, front/back registration, e-paper boss registration, closed STL edges and the exported enclosure envelopes. Printer shrinkage, laser kerf and purchased-part fit remain physical checks.

The Python validator independently checks copper clearance, annular rings, edge/hole spacing, electrical connectivity of every routed net, per-pixel pinout, station/alias coverage, contiguous indices and exact generated CAM parity. It does not implement full KiCad DRC, electrical rules checking, impedance analysis or thermal analysis. Its explicit limitations are retained in the report. E-paper SPI timings, power sequencing and display refresh intervals require the purchased module and a bench test.

The default matrix can be replaced by a more geographic schematic in a future board revision, but the topology data is already present in `network-inventory.json`. Preserve LED indices or deliberately version `layout_id` when moving station assignments. Do not relabel the physical artwork while leaving an old mapping on the device.

## Primary references

- [Adafruit NeoPixel Mini Button 1612](https://www.adafruit.com/product/1612): reference RGB module, dimensions, protocol and conservative current requirement. Availability and module revisions vary.
- [Texas Instruments SN74AHCT125](https://www.ti.com/product/SN74AHCT125) and [datasheet](https://www.ti.com/lit/ds/symlink/sn74ahct125.pdf): 5 V TTL-compatible input buffer and pin connections.
- [Waveshare 7.5-inch V2 specification](https://files.waveshare.com/upload/6/60/7.5inch_e-Paper_V2_Specification.pdf): 800 × 480 resolution, glass/active-area dimensions and panel interface. The matching HAT adds the power/driver circuitry; its revision-specific instructions still apply.
- [LTA Thomson–East Coast Line](https://www.lta.gov.sg/content/ltagov/en/upcoming_projects/rail_expansion/thomson_east_coast_line.html) plus the dated full-network sources in `NETWORK-COVERAGE.md` establish station coverage.
