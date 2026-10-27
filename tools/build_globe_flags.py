#!/usr/bin/env python3
"""Compile GlobeSwitch's MIT pixel-art corpus into embedded lossless PNGs."""
from pathlib import Path
import struct
import zlib

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "assets/globe-switch"


def png(pixels):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    scanlines = b"".join(b"\0" + pixels[row * 88:(row + 1) * 88] for row in range(14))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 22, 14, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(scanlines, 9)) + chunk(b"IEND", b""))


def compile_flags():
    blocks = {}
    for block in (ASSETS / "flags.txt").read_text().strip().split("\n\n"):
        code = block[:2]
        assert code not in blocks, f"duplicate region: {code}"
        blocks[code] = block.splitlines()

    def pixels(code, seen=()):
        assert code not in seen, f"cyclic alias: {code}"
        lines = blocks[code]
        if " = " in lines[0]:
            return pixels(lines[0].split(" = ")[1], (*seen, code))
        assert len(lines) == 16, code
        palette = {pair[0]: bytes.fromhex(pair[2:]) + b"\xff" for pair in lines[1].split()}
        palette["."] = b"\0\0\0\0"
        assert all(len(row) == 22 for row in lines[2:]), code
        return b"".join(palette[pixel] for row in lines[2:] for pixel in row)

    output = bytearray(b"UPYRFLAGS1")
    for code in sorted(blocks):
        data = png(pixels(code))
        output.extend(code.encode("ascii") + struct.pack("<I", len(data)) + data)
    (ASSETS / "flags.bin").write_bytes(output)
    print(f"Compiled {len(blocks)} pixel flags into {len(output)} bytes")


if __name__ == "__main__":
    compile_flags()
