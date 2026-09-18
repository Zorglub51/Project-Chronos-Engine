#!/usr/bin/env python3
"""Make an existing prepared VM runtime independent of the shared repository."""
import argparse
import json
import os
from pathlib import Path
import shutil
import tempfile

import native


def install(base):
    base = Path(base).resolve()
    root = native.runtime(base / "runtime")
    native.require(not native.active(root), "Stop this test session before installing locally")
    for name in ("gl_proxy", "libMali.so", "m2hook_print.so"):
        path = base / "build" / name
        native.require(path.is_file() and not path.is_symlink(), f"Expected a local build file: {path}")
    # Materialize file links, including ROMs, so disconnecting the Mac share is
    # harmless. Never replace a link until its complete copy has succeeded.
    links = sorted(path for path in root.rglob("*") if path.is_symlink())
    for path in links:
        native.require(path.is_file(), f"Unsupported directory or broken link: {path}")
    needed = sum(path.stat().st_size for path in links)
    native.require(shutil.disk_usage(base).free > needed + 64 * 1024 * 1024,
                   "Insufficient free disk space to copy linked resources")
    for path in links:
        with tempfile.NamedTemporaryFile(dir=path.parent, prefix=".local-", delete=False) as temp:
            temporary = Path(temp.name)
        try:
            shutil.copy2(path.resolve(strict=True), temporary)
            os.replace(temporary, path)
        finally:
            temporary.unlink(missing_ok=True)
    for source, destination, mode in ((native.HERE / "native.py", base / "native.py", 0o644),
                                      (native.HERE / "chronos", base / "chronos", 0o755)):
        native.require(not destination.is_symlink(), f"Refusing a symlink destination: {destination}")
        with tempfile.NamedTemporaryFile(dir=base, prefix=".install-", delete=False) as temp:
            temporary = Path(temp.name)
        try:
            shutil.copyfile(source, temporary)
            temporary.chmod(mode)
            os.replace(temporary, destination)
        finally:
            temporary.unlink(missing_ok=True)
    # The launcher must be accessible before it elevates; saves remain private.
    base.chmod(base.stat().st_mode | 0o555)
    metadata = json.loads((root / native.MARKER).read_text())
    metadata["self_contained"] = True
    (root / native.MARKER).write_text(json.dumps(metadata, indent=2) + "\n")
    print(f"Installed {base / 'chronos'} ({len(links)} links copied, {needed} bytes)")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, help="VM directory containing build/ and runtime/")
    args = parser.parse_args()
    try:
        install(args.root)
    except (OSError, RuntimeError, ValueError) as error:
        parser.exit(1, f"Error: {error}\n")
