#!/usr/bin/env python3
"""Copy a map pool without demolisher territory, as if the planet had no demolishers.

usage: python3 scripts/strip-territory.py data/game/maps-vulcanus data/game/maps-vulcanus-open

Rewrites each .fsmap header without `zone_chunks` (tiles, entities and cliffs are unchanged)
and copies index.json, so the new pool has the same maps, splits and names. Used for the
long-distance Vulcanus test set: in the pool with territory, long routes detour around it.
"""
import argparse
import json
import shutil
import struct
from pathlib import Path

MAGIC_LEN = 8  # see map_format.py: MAGIC + <II header length, body length> + header + body


def strip(data: bytes) -> tuple[bytes, int]:
    header_len, body_len = struct.unpack_from("<II", data, MAGIC_LEN)
    start = MAGIC_LEN + 8
    header = json.loads(data[start:start + header_len])
    removed = len(header.pop("zone_chunks", []))
    encoded = json.dumps(header, separators=(",", ":"), ensure_ascii=False).encode()
    body = data[start + header_len:]
    return data[:MAGIC_LEN] + struct.pack("<II", len(encoded), body_len) + encoded + body, removed


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("source", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    index = json.loads((args.source / "index.json").read_text())
    args.destination.mkdir(parents=True, exist_ok=True)
    for entry in index:
        data, removed = strip((args.source / entry["file"]).read_bytes())
        (args.destination / entry["file"]).write_bytes(data)
        print(f"{entry['name']}: removed {removed} territory chunks")
    shutil.copyfile(args.source / "index.json", args.destination / "index.json")


if __name__ == "__main__":
    main()
