#!/usr/bin/env python3
"""Merge a live fsim dump with simulator settings and stable palette ids."""
import argparse
import json
from pathlib import Path


def merge(dump, current):
    if dump.get("schema_version") != 2 or current.get("schema_version") != 2:
        raise ValueError("both files must use prototype schema version 2")
    result = dict(dump)
    result["physics"] = current["physics"]
    for key in ("vehicles", "fuels", "tiles", "entities", "resources"):
        entries = {entry["name"]: dict(entry) for entry in dump[key]}
        names = [entry["name"] for entry in current[key]]
        existing = set(names)
        extras = sorted(set(entries) - existing)
        if (len(entries) != len(dump[key]) or not existing <= set(entries) or
                extras and key not in ("tiles", "entities", "resources")):
            raise ValueError(f"{key}: dump must contain exactly the current prototype names, with new tiles/entities/resources allowed")
        result[key] = [entries[name] for name in names + extras]
        if key in ("tiles", "resources"):
            for entry, old in zip(result[key], current[key]):
                entry["id"] = old["id"]
            # Tile ids start at 0, resource ids at 1 (0 means none).
            first = len(names) + (key == "resources")
            for i, entry in enumerate(result[key][len(names):], start=first):
                entry["id"] = i
    if result["cliffs"]["name"] != current["cliffs"]["name"]:
        raise ValueError("cliff prototype name changed")
    # A base-only dump has no planet cliffs; keep the ones already in the snapshot.
    if not result.get("other_cliffs"):
        result["other_cliffs"] = current.get("other_cliffs", [])
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dump", type=Path)
    parser.add_argument("current", type=Path, help="current data/prototypes.json")
    parser.add_argument("-o", "--output", type=Path, help="defaults to replacing current atomically")
    args = parser.parse_args()
    try:
        result = merge(json.loads(args.dump.read_text()), json.loads(args.current.read_text()))
        output = args.output or args.current
        temporary = output.with_name(output.name + ".tmp")
        temporary.write_text(json.dumps(result, indent=2, allow_nan=False) + "\n")
        temporary.replace(output)
    except (ValueError, KeyError, TypeError, OSError) as error:
        parser.exit(1, f"import-prototypes: {error}\n")


if __name__ == "__main__":
    main()
