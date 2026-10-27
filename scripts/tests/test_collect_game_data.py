import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
SPEC = importlib.util.spec_from_file_location("collect_game_data", ROOT / "scripts/collect-game-data.py")
COLLECT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COLLECT)


class EngineLifecycleTest(unittest.TestCase):
    def exercise(self, source, timeout=3):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            command = [sys.executable, "-c", source, str(output)]
            return COLLECT.run_engine(command, output / "engine.log", timeout, output)

    def test_completion_marker_stops_and_reaps_engine(self):
        marker = self.exercise('''import json, pathlib, sys, time
p = pathlib.Path(sys.argv[1])
(p / "done.json").write_text(json.dumps({"completed": ["test"]}))
time.sleep(30)
''')
        self.assertEqual(marker["completed"], ["test"])

    def test_error_marker_and_process_exit_fail_loudly(self):
        with self.assertRaisesRegex(RuntimeError, "broken export"):
            self.exercise('''import pathlib, sys, time
(pathlib.Path(sys.argv[1]) / "error.json").write_text('{"error":"broken export"}')
time.sleep(30)
''')
        with self.assertRaisesRegex(RuntimeError, "exited 4 before completion"):
            self.exercise("import sys; sys.exit(4)")

    def test_timeout_stops_engine(self):
        with self.assertRaisesRegex(RuntimeError, "timed out"):
            self.exercise("import time; time.sleep(30)", timeout=0.15)


class SplitTest(unittest.TestCase):
    def test_last_seeds_are_holdout_and_one_map_stays_train(self):
        entries = COLLECT.assign_splits([{"seed": i} for i in range(10)], 8)
        self.assertEqual([e["split"] for e in entries], ["train"] * 2 + ["holdout"] * 8)
        self.assertEqual(COLLECT.assign_splits([{"seed": 1}], 8)[0]["split"], "train")


if __name__ == "__main__":
    unittest.main()
