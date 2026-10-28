import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from zipfile import ZipFile

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("package_autodrive", ROOT / "scripts/package-autodrive.py")
PACKAGER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PACKAGER)


class PackageAutodriveTests(unittest.TestCase):
    def test_release_is_standalone_and_repeatable(self):
        source = ROOT / "mod/factorio-autodrive"
        info = json.loads((source / "info.json").read_text())
        prefix = f'{info["name"]}_{info["version"]}/'
        with tempfile.TemporaryDirectory() as directory:
            archive = PACKAGER.package(Path(directory))
            original = archive.read_bytes()
            self.assertEqual(archive.name, prefix[:-1] + ".zip")
            with ZipFile(archive) as release:
                self.assertIsNone(release.testzip())
                expected = {prefix + p.relative_to(source).as_posix()
                            for p in source.rglob("*") if p.is_file() and not p.name.startswith(".")}
                self.assertEqual(set(release.namelist()), expected)
                self.assertEqual(json.loads(release.read(prefix + "info.json")), info)
                self.assertFalse(any("test-runner" in p for p in release.namelist()))
                for path in expected:
                    self.assertEqual(release.read(path), (source / path.removeprefix(prefix)).read_bytes())
            PACKAGER.package(Path(directory))
            self.assertEqual(archive.read_bytes(), original)


if __name__ == "__main__":
    unittest.main()
