import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("check_mod_api", ROOT / "scripts/check-mod-api.py")
CHECKER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECKER)


def concept(name, **members):
    return {"name": name, "type": {"complex_type": "table", "parameters": [
        {"name": key, "type": value} for key, value in members.items()
    ]}}


class CheckModApiTest(unittest.TestCase):
    def audit(self, source):
        runtime = {
            "global_objects": [{"name": "game", "type": "LuaGameScript"}],
            "classes": [
                {"name": "LuaGameScript", "methods": [{"name": "create_surface"}]},
                {"name": "LuaEntityPrototype", "attributes": [{"name": "weight"}]},
            ],
            "concepts": [
                concept("MapGenSettings", width="uint32", cliff_settings="CliffPlacementSettings",
                        autoplace_settings={"complex_type": "dictionary", "key": {
                            "complex_type": "union", "options": [
                                {"complex_type": "literal", "value": "tile"}
                            ]}, "value": "AutoplaceSettings"}),
                concept("CliffPlacementSettings", richness="double"),
                concept("AutoplaceSettings", treat_missing_as_default="boolean", settings={
                    "complex_type": "dictionary", "key": "string", "value": "AutoplaceControl"}),
            ],
            "events": [],
            "defines": [{"name": "events", "values": [{"name": "on_tick"}]}],
        }
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "prototype-export.lua"
            path.write_text(source)
            audit = CHECKER.Audit(runtime, {"prototypes": [], "types": []})
            audit.file(path)
            return audit.used

    def test_checks_nested_generation_records_and_rejects_unknown_fields(self):
        source = '''game.create_surface("test", {
          width = 64, cliff_settings = {richness = 0},
          autoplace_settings = {tile = {treat_missing_as_default = false, settings = {}}}
        })'''
        used = self.audit(source)
        self.assertIn(("runtime", "CliffPlacementSettings", "richness"), used)
        self.assertIn(("runtime", "AutoplaceSettings", "treat_missing_as_default"), used)
        for old, new in [("richness", "obsolete_field"), ("tile =", "bogus ="),
                         ("create_surface", "obsolete_method")]:
            with self.subTest(new=new), self.assertRaises(ValueError):
                self.audit(source.replace(old, new))

    def test_extracts_indexed_properties_and_event_names(self):
        source = '''for _, key in ipairs({"weight"}) do v[key] = p[key] end
        script.on_event(defines.events.on_tick, handler)'''
        used = self.audit(source)
        self.assertIn(("runtime", "LuaEntityPrototype", "weight"), used)
        self.assertIn(("runtime", "defines", "events.on_tick"), used)
        for old, new in [("weight", "obsolete_property"), ("on_tick", "obsolete_event")]:
            with self.subTest(new=new), self.assertRaises(ValueError):
                self.audit(source.replace(old, new))


if __name__ == "__main__":
    unittest.main()
