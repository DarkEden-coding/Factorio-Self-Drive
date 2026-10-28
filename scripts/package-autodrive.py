#!/usr/bin/env python3
"""Build the standalone autodrive mod ZIP for the Factorio mod portal."""
import argparse
import json
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

ROOT = Path(__file__).resolve().parents[1]


def package(output):
    source = ROOT / "mod/factorio-autodrive"
    info = json.loads((source / "info.json").read_text())
    name = f'{info["name"]}_{info["version"]}'
    # Ship runtime Lua and player documentation only. Never include repository
    # data, training artifacts, copied game assets, or local hidden files.
    files = sorted(source.glob("*.lua"))
    files += [source / filename for filename in ("info.json", "README.md", "changelog.txt", "thumbnail.png")]
    files += sorted(source.glob("locale/*/*.cfg"))
    output.mkdir(parents=True, exist_ok=True)
    archive = output / f"{name}.zip"
    with ZipFile(archive, "w", compression=ZIP_DEFLATED) as release:
        for path in files:
            entry = ZipInfo(f"{name}/{path.relative_to(source).as_posix()}")
            entry.compress_type = ZIP_DEFLATED
            entry.external_attr = 0o100644 << 16
            release.writestr(entry, path.read_bytes())
    return archive


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "dist")
    args = parser.parse_args()
    print(package(args.output).resolve())


if __name__ == "__main__":
    main()
