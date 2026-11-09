"""FSMAP001 encoder. Engine JSON is a transient interchange, never a pool file."""
import json
import math
import struct
import zlib

MAGIC = b"FSMAP001"
ENTITY = struct.Struct("<HdddHf")


def encode_map(export, name):
    if export.get("schema_version") != 1:
        raise ValueError("unsupported map export schema")
    (x0, y0), (x1, y1) = export["area"]
    width, height = x1 - x0, y1 - y0
    if (not 2 <= width <= 8192 or not 2 <= height <= 8192 or
            width * height > 16777216 or any(v % 32 for v in (x0, y0, x1, y1))):
        raise ValueError("invalid map area")
    tiles = bytearray(width * height)
    resources = bytearray(width * height)
    tile_names, resource_names, entity_names, cliff_names = ["deepwater"], ["none"], [], []
    warnings, records, seen, zone_chunks = [], [], set(), []

    def intern(table, value, maximum):
        if value not in table:
            table.append(value)
        index = table.index(value)
        if index > maximum:
            raise ValueError("name table exceeds format capacity")
        return index

    for chunk in export["chunks"]:
        cx, cy = chunk["position"]
        if ((cx, cy) in seen or not x0 <= cx * 32 < x1 or not y0 <= cy * 32 < y1
                or len(chunk["tiles"]) != 1024):
            raise ValueError("duplicate, out-of-area, or incomplete chunk")
        seen.add((cx, cy))
        if chunk.get("territory"):
            zone_chunks.append([cx, cy])
        for i, tile in enumerate(chunk["tiles"]):
            index = (cy * 32 - y0 + i // 32) * width + cx * 32 - x0 + i % 32
            tiles[index] = intern(tile_names, tile, 255)
        for resource in chunk["resources"]:
            x, y = resource["position"]
            if x // 32 != cx or y // 32 != cy:
                raise ValueError("resource outside its chunk")
            resources[(y - y0) * width + x - x0] = intern(resource_names, resource["name"], 255)
        for entity in chunk["entities"]:
            x, y = entity["position"]
            orientation, health = entity.get("orientation", 0), entity.get("health")
            if (not all(math.isfinite(v) for v in (x, y, orientation)) or
                    math.floor(x / 32) != cx or math.floor(y / 32) != cy or
                    health is not None and (not math.isfinite(health) or health < 0)):
                raise ValueError("invalid entity state or chunk")
            cliff = entity.get("cliff_orientation")
            records.append(ENTITY.pack(intern(entity_names, [entity["name"], entity["kind"]], 65535),
                                       x, y, orientation,
                                       intern(cliff_names, cliff, 65534) if cliff else 65535,
                                       health if health is not None else math.nan))
    if len(seen) != width // 32 * (height // 32):
        warnings.append("ungenerated chunks are blocked with deepwater")
    metadata = dict(name=name, seed=export.get("seed", 0), area=export["area"], spawn=[0, 0],
                    source=export.get("source", {}), tile_names=tile_names, resource_names=resource_names,
                    entity_names=entity_names, cliff_names=cliff_names, entity_count=len(records), warnings=warnings)
    if zone_chunks:
        # Source chunk positions of demolisher territory; every tile in them is a zone tile.
        metadata["zone_chunks"] = zone_chunks
    header = json.dumps(metadata, separators=(",", ":"), ensure_ascii=False).encode()
    body = bytes(tiles + resources) + b"".join(records)
    encoded = MAGIC + struct.pack("<II", len(header), len(body)) + header + zlib.compress(body, 6)
    entry = dict(name=name, file=name + ".fsmap", seed=metadata["seed"], width=width, height=height,
                 x_min=-width // 2, y_min=-height // 2,
                 source_offset=[x0 + width // 2, y0 + height // 2], entity_count=len(records))
    return encoded, entry
