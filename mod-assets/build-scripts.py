#!/usr/bin/env python3
"""Pack Chronos overrides using the Rust MZS CLI (no external Python packages).

The per-game save adapter is appended to utils before packing. Native boot
executes utils through Resource.exec; calling system() there would deadlock
its script loader. The adapter is compiled exactly once with utils.
"""
import argparse
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mzs", type=Path, required=True, help="Path to the m2-mzs pack/unpack executable")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent
    tool = args.mzs.resolve(strict=True)
    built = root / "scripts-built"
    built.mkdir(exist_ok=True)
    for source in sorted((root / "scripts-src").glob("*.nut")):
        if source.name in ("chronos_saves.nut", "chronos_library.nut", "chronos_covers.nut"):
            continue
        with tempfile.TemporaryDirectory(prefix="chronos-script-") as temporary:
            staged = Path(temporary) / source.name
            data = source.read_bytes()
            if source.name == "utils.nut":
                data += b"\r\n" + (root / "scripts-src/chronos_saves.nut").read_bytes()
                data += b"\r\n" + (root / "scripts-src/chronos_library.nut").read_bytes()
                covers = root / "scripts-src/chronos_covers.nut"
                if covers.is_file():
                    data += b"\r\n" + covers.read_bytes()
            staged.write_bytes(data)
            output = built / (source.name + ".m")
            if output.is_file():
                decoded = Path(temporary) / "previous.nut"
                subprocess.run([str(tool), "unpack", str(output), str(decoded)], check=True)
                if decoded.read_bytes() == data:
                    continue
            subprocess.run([str(tool), "pack", str(staged), str(output)], check=True)


if __name__ == "__main__":
    main()
