#!/usr/bin/env python3
"""Build a render-ready manifest and copy referenced PNGs from a local Factorio install."""
import argparse
import json
from pathlib import Path
import re
import shutil
import struct

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_CONTENTS = Path.home() / "Library/Application Support/Steam/steamapps/common/Factorio/factorio.app/Contents"


def pair(value, default=0):
    if isinstance(value, dict):
        return [value.get("x", default), value.get("y", default)]
    return list(value) if isinstance(value, (list, tuple)) else [value or default] * 2


def color(value):
    if isinstance(value, dict):
        result = [value.get(k, 1 if k == "a" else 0) for k in "rgba"]
    else:
        result = list(value)
        result += [1] * (4 - len(result))
    if max(result[:3]) > 1:
        result[:3] = [v / 255 for v in result[:3]]
        if result[3] > 1:
            result[3] /= 255
    return result


class Builder:
    def __init__(self, contents, output):
        self.contents, self.output = contents, output
        self.images = {}
        self.rect_count = 0

    def image(self, filename):
        match = re.fullmatch(r"__([\w-]+)__/(.+\.png)", filename)
        if not match or ".." in Path(match[2]).parts:
            raise ValueError(f"unsupported graphics path: {filename}")
        module, relative = match.groups()
        source = self.contents / "data" / module / relative
        image = f"{module}/{relative}"
        if image not in self.images:
            with source.open("rb") as stream:
                header = stream.read(24)
            if header[:8] != b"\x89PNG\r\n\x1a\n" or header[12:16] != b"IHDR":
                raise ValueError(f"invalid PNG: {source}")
            width, height = struct.unpack(">II", header[16:24])
            destination = self.output / image
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, destination)
            self.images[image] = dict(width=width, height=height, bytes=source.stat().st_size)
        return image

    def sprite(self, definition, frame=0, direction=0, variation=0, shadow=False, tint=None):
        if "layers" in definition:
            return [sprite for layer in definition["layers"] for sprite in
                    self.sprite(layer, frame, direction, variation, shadow, tint)]
        if "sheet" in definition:
            return self.sprite(definition["sheet"], frame, direction, variation, shadow, tint)
        if "sheets" in definition:
            return [s for sheet in definition["sheets"] for s in
                    self.sprite(sheet, frame, direction, variation, shadow, tint)]
        size = pair(definition.get("size", 0))
        width, height = definition.get("width", size[0]), definition.get("height", size[1])
        if not width or not height:
            raise ValueError(f"sprite missing dimensions: {definition}")
        frames = definition.get("frame_count", 1)
        line = definition.get("line_length") or frames
        x, y = definition.get("x", 0), definition.get("y", 0)
        if "stripes" in definition:
            # Rotated vehicle stripes store frames across columns and directions
            # down rows. multiplystripes repeats an identical stripe for repeated
            # animation frames; select the first frame of those copies.
            stripes = []
            for stripe in definition["stripes"]:
                if not stripes or stripe != stripes[-1]:
                    stripes.append(stripe)
            index = direction
            for stripe in stripes:
                columns = stripe["width_in_frames"]
                rows = stripe.get("height_in_frames", definition.get("direction_count", 1))
                directions = rows * max(1, columns // frames)
                if index < directions:
                    column = (index * frames + frame) % columns
                    row = (index * frames + frame) // columns if columns >= frames else index
                    filename = stripe["filename"]
                    x += stripe.get("x", 0) + column * width
                    y += stripe.get("y", 0) + row * height
                    break
                index -= directions
            else:
                raise ValueError("direction outside sprite stripes")
        else:
            # Animation frames run horizontally; variations/directions begin on
            # the next row when no explicit line_length is present.
            index = (direction + variation) * frames + frame
            row, column = divmod(index, line)
            if "filenames" in definition:
                file_index, row = divmod(row, definition["lines_per_file"])
                filename = definition["filenames"][file_index]
            else:
                filename = definition["filename"]
            x += column * width
            y += row * height
        image = self.image(filename)
        dimensions = self.images[image]
        if x < 0 or y < 0 or x + width > dimensions["width"] or y + height > dimensions["height"]:
            raise ValueError(f"sprite rectangle outside {image}: {x},{y} {width}x{height}, PNG {dimensions}")
        result = dict(image=image, x=x, y=y, width=width, height=height,
                      scale=definition.get("scale", 1), shift=pair(definition.get("shift", [0, 0])),
                      draw_as_shadow=bool(shadow or definition.get("draw_as_shadow", False)))
        if definition.get("tint") is not None or tint is not None:
            result["tint"] = color(definition.get("tint", tint))
        if definition.get("apply_runtime_tint"):
            result["tint"] = [0.9, 0.5, 0.2, 1]
        self.rect_count += 1
        return [result]

    @staticmethod
    def paint(sprites):
        return sorted(sprites, key=lambda s: not s["draw_as_shadow"])

    def variations(self, pictures):
        if isinstance(pictures, list):
            return [self.paint(self.sprite(picture)) for picture in pictures]
        sheet = pictures.get("sheet", pictures)
        count = sheet.get("variation_count", 1)
        return [self.paint(self.sprite(pictures, variation=i)) for i in range(count)]

    def build(self, raw):
        manifest = dict(schema_version=1, tile_pixels=32, tiles={}, entities={}, cliffs={}, resources={}, vehicles={})
        for name, tile in sorted(raw["tiles"].items()):
            # Choose the 1x1 terrain arrangement so each map tile is independent.
            main = next(p for p in tile["variants"]["main"] if p["size"] == 1)
            px = round(32 / main.get("scale", 1))
            definition = dict(filename=main["picture"], width=px, height=px, scale=main.get("scale", 1),
                              x=main.get("x", 0), y=main.get("y", 0),
                              line_length=main.get("line_length") or main["count"])
            manifest["tiles"][name] = dict(size_tiles=1, variations=[
                self.sprite(definition, frame=i) for i in range(main["count"])])
        for name, entity in sorted(raw["entities"].items()):
            if entity.get("variations"):
                variations = []
                for i, tree in enumerate(entity["variations"]):
                    layers = []
                    for layer in ("shadow", "trunk", "leaves"):
                        if tree.get(layer):
                            tint = None
                            if layer == "leaves" and entity.get("colors"):
                                tint = entity["colors"][i % len(entity["colors"])]
                            layers.extend(self.sprite(tree[layer], shadow=layer == "shadow", tint=tint))
                    variations.append(layers)
            elif entity.get("animations"):
                variations = [self.paint(self.sprite(animation)) for animation in entity["animations"]]
            else:
                pictures = entity.get("pictures", entity.get("picture"))
                if not pictures:
                    raise ValueError(f"no drawable graphics for entity {name}")
                variations = self.variations(pictures)
            manifest["entities"][name] = dict(variations=variations)
        for orientation, definition in sorted(raw["cliffs"]["cliff"].items()):
            manifest["cliffs"][orientation.replace("_", "-")] = dict(
                variations=self.variations(definition["pictures"]))
        for name, resource in sorted(raw["resources"].items()):
            stages = resource["stages"]
            sheet = stages.get("sheet", stages.get("sheets", [stages])[0])
            count = sheet.get("variation_count", 1)
            # Richest ore stage, one static frame per shape variation.
            manifest["resources"][name] = dict(stage=0, variations=[
                self.paint(self.sprite(stages, frame=0, variation=i)) for i in range(count)])
        for name, vehicle in sorted(raw["vehicles"].items()):
            animation = vehicle["animation"]
            count = animation.get("direction_count", animation.get("layers", [{}])[0].get("direction_count", 1))
            directions = []
            for direction in range(count):
                body = self.sprite(animation, direction=direction)
                turret = self.sprite(vehicle["turret_animation"], direction=direction)
                # Shadow layers compose first; turret tracks body orientation.
                layers = body + turret
                directions.append(sorted(layers, key=lambda s: not s["draw_as_shadow"]))
            manifest["vehicles"][name] = dict(direction_count=count, directions=directions)
        manifest["images"] = self.images
        manifest["summary"] = dict(image_count=len(self.images), sprite_count=self.rect_count,
                                   image_bytes=sum(i["bytes"] for i in self.images.values()))
        return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--factorio", type=Path, default=DEFAULT_CONTENTS, help="Factorio Contents directory")
    parser.add_argument("--input", type=Path, default=ROOT / "data/game/graphics.json")
    parser.add_argument("--output", type=Path, default=ROOT / "app/public/game-assets")
    args = parser.parse_args()
    try:
        raw = json.loads(args.input.read_text())
        if raw.get("schema_version") != 1:
            raise ValueError("unsupported graphics schema")
        builder = Builder(args.factorio.expanduser(), args.output)
        manifest = builder.build(raw)
        # Remove only stale generated PNGs; no unrelated files are touched.
        for path in args.output.rglob("*.png"):
            if path.relative_to(args.output).as_posix() not in builder.images:
                path.unlink()
        (args.output / "manifest.json").write_text(json.dumps(manifest, separators=(",", ":")) + "\n")
        print(f"Game assets verified: {len(manifest['tiles'])} tiles, {len(manifest['entities'])} entities, "
              f"{len(manifest['cliffs'])} cliff orientations, {len(manifest['resources'])} resources, "
              f"{len(manifest['vehicles'])} vehicles; {builder.rect_count} rectangles, "
              f"{len(builder.images)} PNGs, {manifest['summary']['image_bytes']:,} bytes copied")
    except (OSError, ValueError, KeyError, IndexError) as error:
        parser.exit(1, f"build-game-assets: {error}\n")


if __name__ == "__main__":
    main()
