# Singapore MRT display firmware

Two independent ESP32 devices consume the display service's `GET /v1/frame`:

* **`led`** drives the full MRT station PCB: 146 individually addressable pixels,
  one per physical station, including shared interchange stations. This is the
  default build. It has no e-paper driver.
* **`epaper`** drives a separate Waveshare 7.5-inch black/white V2, 800 × 480
  arrival-board device. It has no LED driver. Point it at a service configured
  for the station whose arrivals you want to display.

The devices share a protocol, not a physical enclosure. The default mapping is
[`singapore-mrt-v1`](../hardware/layouts/singapore-mrt.json). `LED_COUNT` is 146
on **both** devices so a frame from another physical mapping fails validation.
The e-paper device validates pixel metadata but allocates no NeoPixel driver or
pixel output task. The selected build does not require the other device to exist.

## Build and flash

Install Python and [PlatformIO Core](https://docs.platformio.org/en/latest/core/installation/index.html),
then run from the repository root:

```sh
python3 -m pip install platformio==6.1.18
cp firmware/include/config.example.h firmware/include/config.h
```

Edit `config.h` with the Wi-Fi credentials, trusted-LAN service URL (including
`/v1/frame`), and a reachable NTP server. The URL's default port is 8787.
`config.h` is gitignored; do not commit credentials. The upstream LTA/DataMall
account key belongs on the service and is never sent to either device.

```sh
# LED station PCB controller
pio run -d firmware -e led
pio run -d firmware -e led -t upload --upload-port /dev/ttyUSB0

# Separate arrival board; use its own ESP32 and USB port
pio run -d firmware -e epaper
pio run -d firmware -e epaper -t upload --upload-port /dev/ttyUSB1

pio device monitor --baud 115200 --port /dev/ttyUSB0
```

Both environments target `esp32dev` and pin the platform and library versions
in `platformio.ini`. Without `config.h`, builds use placeholder settings from
`config.example.h`; they will not join a real Wi-Fi network. Flash each device
with its own edited configuration as appropriate. USB port names vary by OS.

To exercise the four-pixel demo layout, set `EXPECTED_LAYOUT_ID` to
`fixture-mini-v1` and `LED_COUNT` to `4`, and run the service with its matching
fixture layout. Restore both values for the full station PCB. Never reuse an
LED index order without also updating the layout ID on service and firmware.

## Wiring

The LED controller uses GPIO 13 through an external **SN74AHCT125** level
shifter and series resistor into the board's first DIN. Power pixels from the
board's external, regulated, fused 5 V distribution with a common ground. The
full board has ten independently fused power banks of at most 16 pixels.
Do not carry the board's LED current through the ESP32 or its USB connector.
See the [hardware design](../hardware/README.md) for supply, fuse, and assembly
details, including the level shifter's enable and boot-state wiring.

The separate e-paper ESP32 connects to the **adapter/HAT**, using 3.3 V logic:

| Adapter signal | ESP32 GPIO |
| --- | ---: |
| CLK / SCK | 18 |
| DIN / MOSI | 23 |
| CS | 5 |
| DC | 17 |
| RST | 16 |
| BUSY | 4 |
| GND | GND |

Power the adapter according to its exact revision's specification; this
firmware does not connect directly to a bare panel FPC. It selects
[`GxEPD2_750_T7`](https://github.com/ZinggJM/GxEPD2/blob/1.6.4/src/epd/GxEPD2_750_T7.h)
for the GDEW075T7 / GD7965 800 × 480 panel. Verify the controller marking when
buying: a different 7.5-inch variant can need another driver class despite
having the same dimensions. The two-millisecond reset pulse accommodates
Waveshare adapter boards with the reset-controlled power circuit.

## Display behavior

The service is polled every 15 seconds. A valid frame replaces the entire LED
map; omitted pixels become dark. Schedule-only positions remain estimates.
The e-paper board labels `REALTIME SNAPSHOT` or `SCHEDULE ESTIMATE`, and every
departure includes `RT` or `SCH`. The source notice explains the data's origin.
This does not create train-position telemetry that the upstream feed lacks.

E-paper uses full refresh only, defaults to a conservative 180-second minimum
between completed refreshes, and hibernates the panel afterward. The firmware
rejects configurations below 60 seconds; follow the purchased panel's actual
refresh guidance before changing the default. It shows snapshot and expiry
timestamps in **SGT (UTC+8)** and explicitly labels minutes as values **at the
snapshot**. They do not count down on the retained display. The built-in font
supports ASCII; other UTF-8 characters become one `?` per code point.

A failed request, wrong layout, malformed frame, expired source, or unavailable
source switches LEDs off. An independent task checks LED expiry every 25 ms,
including while networking or an e-paper refresh is blocked. The e-paper
device marks the offline state on its next permitted full refresh, which can
be roughly three minutes later plus network/refresh time. A powerless e-paper
screen cannot update; the always-visible expiry and retention warning remain
the way to judge its age. The display never presents a retained frame as an
unqualified live countdown.

LED expiry requires a running ESP32. Addressable pixels can retain their last
color if the controller loses power or crashes while the LED supply stays on;
a hardware watchdog that disconnects LED power is needed if that failure must
also force darkness. Booting firmware clears the chain before joining Wi-Fi.

## Frame acceptance and limits

* Schema 1, exact layout ID, UNIX-second timestamps, known source state,
  complete board text, and all pixel and row fields must pass before any
  output changes. Pixel indices must be unique and below `LED_COUNT`.
* JSON input is capped at 48 KiB and six nesting levels; pixel count is capped
  at the configured count (at most 512). Board limits are eight rows,
  80-byte station names, 12-byte line labels, 96-byte destinations, and
  240-byte notices. Embedded NULs and control characters are rejected.
* Usable NTP time is required before accepting data. The service and NTP
  server must agree within five seconds. Expired frames, lifetimes over
  300 seconds, and older generations are rejected. Repeated generations
  cannot renew the local lease. One second is subtracted from the timestamp
  lease to account for clock resolution. Lease checks survive `millis()`
  wraparound and expire independently of later wall-clock changes.
* `stale` and `unavailable` frames leave LEDs dark, even if they contain
  pixels. The e-paper device shows safe timetable fallback rows from a fresh
  `stale` frame under `STALE LIVE INPUT - SCHEDULE SNAPSHOT`, with the source
  notice. `schedule` is a valid estimated map. No frame or credentials are
  logged over serial.
* HTTP is intentionally for a trusted LAN/VPN only. Responses must carry a
  bounded `Content-Length`; redirects, chunking, compression, HTTP errors,
  and incomplete bodies fail closed. There is no TLS, server authentication,
  signed-frame verification, OTA updater, or captive configuration portal.
  NTP and HTTP are not authentication mechanisms; keep the network trusted.

`LED_BRIGHTNESS` defaults to 32/255. After that scaling, a second limiter caps
the **estimated** LED current to 1,000 mA using 20 mA per full color channel
plus 1 mA idle per installed pixel. This keeps the default 146-pixel build
conservative. It is not a current sensor and does not replace the board's
fuses, suitable wiring, thermal checks, or correctly rated supply. Increase
the budget only after validating the installed hardware's power distribution.

## Verification

The policy tests run without an Arduino installation:

```sh
c++ -std=c++17 -Wall -Wextra -Werror -Ifirmware/include \
  firmware/test/policy_test.cpp -o /tmp/mrt-policy-test
/tmp/mrt-policy-test
```

They exercise expiry, replay and duplicate handling, future timestamps,
unsynchronized clocks, rollover, pixel bounds/duplicates, and current limits.
Run both `pio run` commands above to compile the actual ESP32 targets.

Once the LED build has installed its pinned ArduinoJson dependency, run the
same JSON decoder used by both devices on the host:

```sh
c++ -std=c++17 -Wall -Wextra -Werror \
  -Ifirmware/include -Ifirmware/.pio/libdeps/led/ArduinoJson/src \
  firmware/test/parser_test.cpp -o /tmp/mrt-parser-test
/tmp/mrt-parser-test
```

These tests cover the Rust schema fixture, invalid types, wrong layouts,
duplicate and out-of-range pixels, malformed JSON, text limits/control
characters, and an actual 512-pixel document at the decoder's memory limit.

On a bench, first disconnect LED power and verify GPIO/level-shifter behavior;
then power one bank at the default limit. Check the service's station-index
mapping against the fabricated board. Disconnect Wi-Fi, stop the service,
send an expired frame and a wrong-layout frame, and observe that lit pixels
clear. Power-cycle the e-paper device, verify panel orientation and BUSY
behavior, and wait through at least two refresh intervals. Target compilation
and bench validation are separate gates; a passing host test does not verify
the chosen panel, ESP32 electrical connections, or power distribution.
