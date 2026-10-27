import importlib.util
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("import_prototypes", ROOT / "scripts/import-prototypes.py")
IMPORTER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(IMPORTER)


class ImportPrototypesTest(unittest.TestCase):
    def test_rejects_old_schema(self):
        current = json.loads((ROOT / "data/prototypes.json").read_text())
        dump = dict(current, schema_version=1)
        with self.assertRaisesRegex(ValueError, "schema version 2"):
            IMPORTER.merge(dump, current)
        with self.assertRaisesRegex(ValueError, "schema version 2"):
            IMPORTER.merge(current, dump)

    def test_preserves_settings_ids_and_live_metadata(self):
        current = json.loads((ROOT / "data/prototypes.json").read_text())
        dump = json.loads(json.dumps(current))
        dump.pop("physics")
        dump["source"]["game_version"] = "2.1.23"
        dump["source"]["active_mods"] = {"base": "2.1.23", "fsim-export": "0.1.0"}
        dump["vehicles"][0]["consumption"] = 123456
        for key in ("tiles", "resources"):
            dump[key].reverse()
            for entry in dump[key]:
                entry["id"] = 200
        merged = IMPORTER.merge(dump, current)
        self.assertEqual(merged["physics"], current["physics"])
        self.assertEqual(merged["source"], dump["source"])
        self.assertFalse(merged["vehicles"][0]["immune_to_all_impacts"])
        self.assertTrue(merged["vehicles"][1]["immune_to_all_impacts"])
        self.assertEqual(merged["vehicles"][0]["consumption"], 123456)
        for key in ("tiles", "resources"):
            self.assertEqual([(x["name"], x["id"]) for x in merged[key]],
                             [(x["name"], x["id"]) for x in current[key]])
        dump["tiles"].pop()
        with self.assertRaisesRegex(ValueError, "exactly the current prototype names"):
            IMPORTER.merge(dump, current)

    def test_expands_real_entity_resource_catalog_without_changing_existing_ids(self):
        current = json.loads((ROOT / "data/prototypes.json").read_text())
        dump = json.loads(json.dumps(current))
        extra = dict(dump["resources"][0], name="new-nauvis-resource", id=250)
        dump["resources"].insert(0, extra)
        dump["entities"].append(dict(dump["entities"][0], name="new-nauvis-tree"))
        merged = IMPORTER.merge(dump, current)
        self.assertEqual(merged["resources"][-1]["name"], "new-nauvis-resource")
        self.assertEqual(merged["resources"][-1]["id"], len(current["resources"]) + 1)
        self.assertEqual(merged["resources"][:-1], current["resources"])
        self.assertEqual(merged["entities"][-1]["name"], "new-nauvis-tree")
        self.assertEqual(merged["physics"], current["physics"])


if __name__ == "__main__":
    unittest.main()
