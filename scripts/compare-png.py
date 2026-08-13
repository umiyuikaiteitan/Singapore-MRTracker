#!/usr/bin/env python3
"""Compare two PNG files within a tolerance.

The script decodes PNG itself, so the visual regression check needs
nothing beyond the standard library. It handles the subset that a
headless Chromium writes: 8-bit RGB or RGBA, non-interlaced, with the
five standard filter types.

Exit status is 0 when the images match within the tolerance and 1 when
they do not.
"""

from __future__ import annotations

import sys
import zlib

# The mean absolute difference per channel, out of 255, that still
# counts as a match. Text antialiasing moves single pixels a long way,
# so a small mean is the useful measure.
MEAN_TOLERANCE = 2.0
# The share of pixels that may differ strongly, and what "strongly"
# means on a 0-255 channel.
OUTLIER_SHARE = 0.01
OUTLIER_DELTA = 32


def read_png(path: str) -> tuple[int, int, int, bytes]:
    """Return (width, height, channels, pixel bytes) of a PNG file."""
    with open(path, "rb") as handle:
        data = handle.read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError(f"{path} is not a PNG file")

    offset = 8
    header = None
    idat = bytearray()
    while offset < len(data):
        length = int.from_bytes(data[offset : offset + 4], "big")
        kind = data[offset + 4 : offset + 8]
        body = data[offset + 8 : offset + 8 + length]
        offset += 12 + length
        if kind == b"IHDR":
            header = body
        elif kind == b"IDAT":
            idat.extend(body)
        elif kind == b"IEND":
            break

    if header is None:
        raise ValueError(f"{path} has no header")
    width = int.from_bytes(header[0:4], "big")
    height = int.from_bytes(header[4:8], "big")
    depth = header[8]
    colour = header[9]
    interlace = header[12]
    if depth != 8 or interlace != 0 or colour not in (2, 6):
        raise ValueError(
            f"{path} uses an unsupported PNG form "
            f"(depth {depth}, colour {colour}, interlace {interlace})"
        )
    channels = 3 if colour == 2 else 4

    raw = zlib.decompress(bytes(idat))
    stride = width * channels
    out = bytearray(height * stride)
    previous = bytearray(stride)
    position = 0
    for row in range(height):
        filter_type = raw[position]
        position += 1
        line = bytearray(raw[position : position + stride])
        position += stride
        if filter_type == 1:  # Sub
            for i in range(channels, stride):
                line[i] = (line[i] + line[i - channels]) & 0xFF
        elif filter_type == 2:  # Up
            for i in range(stride):
                line[i] = (line[i] + previous[i]) & 0xFF
        elif filter_type == 3:  # Average
            for i in range(stride):
                left = line[i - channels] if i >= channels else 0
                line[i] = (line[i] + ((left + previous[i]) >> 1)) & 0xFF
        elif filter_type == 4:  # Paeth
            for i in range(stride):
                left = line[i - channels] if i >= channels else 0
                up = previous[i]
                up_left = previous[i - channels] if i >= channels else 0
                estimate = left + up - up_left
                da = abs(estimate - left)
                db = abs(estimate - up)
                dc = abs(estimate - up_left)
                if da <= db and da <= dc:
                    predictor = left
                elif db <= dc:
                    predictor = up
                else:
                    predictor = up_left
                line[i] = (line[i] + predictor) & 0xFF
        elif filter_type != 0:
            raise ValueError(f"{path} uses filter type {filter_type}")
        out[row * stride : (row + 1) * stride] = line
        previous = line
    return width, height, channels, bytes(out)


def compare(expected_path: str, actual_path: str, name: str) -> bool:
    ew, eh, ec, expected = read_png(expected_path)
    aw, ah, ac, actual = read_png(actual_path)
    if (ew, eh) != (aw, ah):
        print(f"{name}: size changed from {ew}x{eh} to {aw}x{ah}")
        return False

    total = 0
    outliers = 0
    samples = 0
    for index in range(0, min(len(expected), len(actual))):
        # Skip the alpha channel: it carries no visible difference.
        if ec == 4 and index % 4 == 3:
            continue
        if ac == 4 and index % 4 == 3:
            continue
        delta = abs(expected[index] - actual[index])
        total += delta
        samples += 1
        if delta > OUTLIER_DELTA:
            outliers += 1

    mean = total / samples if samples else 0.0
    share = outliers / samples if samples else 0.0
    ok = mean <= MEAN_TOLERANCE and share <= OUTLIER_SHARE
    verdict = "ok" if ok else "CHANGED"
    print(
        f"{name}: {verdict} (mean difference {mean:.3f}/255, "
        f"{share * 100:.3f}% of channels beyond {OUTLIER_DELTA})"
    )
    return ok


def main() -> int:
    if len(sys.argv) != 4:
        print("usage: compare-png.py <expected.png> <actual.png> <name>", file=sys.stderr)
        return 2
    return 0 if compare(sys.argv[1], sys.argv[2], sys.argv[3]) else 1


if __name__ == "__main__":
    sys.exit(main())
