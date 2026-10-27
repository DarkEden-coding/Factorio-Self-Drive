import importlib.util
import json
from pathlib import Path
import struct
import sys
import tempfile
import unittest
import zlib

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
from map_format import encode_map, ENTITY
SPEC = importlib.util.spec_from_file_location("build_game_assets", SCRIPTS / "build-game-assets.py")
ASSETS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ASSETS)


class MapFormatTest(unittest.TestCase):
    def test_real_export_encoding_preserves_names_positions_health_and_gaps(self):
        export = json.loads((SCRIPTS.parent / "crates/fsim-core/tests/fixtures/map-export.json").read_text())
        encoded, entry = encode_map(export, "test")
        self.assertEqual(encoded[:8], b"FSMAP001")
        size, raw = struct.unpack_from("<II", encoded, 8)
        header = json.loads(encoded[16:16 + size])
        body = zlib.decompress(encoded[16 + size:])
        self.assertEqual(len(body), raw)
        self.assertEqual(header["entity_count"], 4)
        self.assertEqual(entry["source_offset"], [80, -16])
        self.assertEqual(header["spawn"], [0, 0])
        record = ENTITY.unpack_from(body, 2048)
        self.assertEqual(record[1:4], (70.5, -20.5, 0.125))
        self.assertEqual(record[-1], 37)
        self.assertLess(len(encoded), len(json.dumps(export)))
        export["chunks"].append(export["chunks"][0])
        with self.assertRaisesRegex(ValueError, "duplicate"):
            encode_map(export, "bad")
        export["chunks"] = []
        encoded, _ = encode_map(export, "gap")
        size = struct.unpack_from("<I", encoded, 8)[0]
        self.assertTrue(json.loads(encoded[16:16 + size])["warnings"])


class AssetExpansionTest(unittest.TestCase):
    def test_real_vehicle_stripe_boundaries_and_files_are_explicit(self):
        # Three PNG headers suffice here; the builder only needs IHDR dimensions.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ("a", "b", "sheet"):
                path = root / f"data/base/{name}.png"
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"\x89PNG\r\n\x1a\n" + b"\0\0\0\rIHDR" + struct.pack(">II", 40, 40))
            builder = ASSETS.Builder(root, root / "out")
            stripe_a = dict(filename="__base__/a.png", width_in_frames=1, height_in_frames=2)
            stripe_b = dict(filename="__base__/b.png", width_in_frames=1, height_in_frames=2)
            definition = dict(width=10, height=10, frame_count=2, direction_count=4,
                              stripes=[stripe_a, stripe_a, stripe_b, stripe_b])
            directions = [builder.sprite(definition, direction=i)[0] for i in range(4)]
            self.assertEqual([s["y"] for s in directions], [0, 10, 0, 10])
            self.assertTrue(directions[2]["image"].endswith("b.png"))
            body = dict(width=10, height=10, frame_count=2, direction_count=4,
                        stripes=[dict(filename="__base__/a.png", width_in_frames=2, height_in_frames=4)])
            self.assertEqual(builder.sprite(body, direction=3)[0]["y"], 30)
            files = dict(width=10, height=10, direction_count=4, line_length=2, lines_per_file=1,
                         filenames=["__base__/a.png", "__base__/b.png"])
            self.assertEqual(builder.sprite(files, direction=3)[0]["image"], "base/b.png")
            self.assertEqual(builder.sprite(files, direction=3)[0]["x"], 10)
            with self.assertRaisesRegex(ValueError, "outside"):
                builder.sprite(dict(filename="__base__/a.png", width=50, height=10))


if __name__ == "__main__":
    unittest.main()
