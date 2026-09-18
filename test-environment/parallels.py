#!/usr/bin/env python3
"""Control the Linux test harness from macOS through Parallels Tools.

Example: python3 test-environment/parallels.py build
Pass guest paths after `prepare`; nothing is installed automatically.
"""
import argparse
import os
from pathlib import Path
import shlex
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--vm", default="Ubuntu 24.04 ARM64")
    parser.add_argument("--guest-root", default="/home/parallels/chronos-native")
    parser.add_argument("--guest-repo", help="Repository path inside the VM; defaults to the Parallels Home share")
    parser.add_argument("command", choices=("status", "resume", "build", "prepare", "start", "stop", "log", "key", "screenshot"))
    args, extra = parser.parse_known_args()
    prlctl = os.environ.get("PRLCTL", "/usr/local/bin/prlctl")
    repo = Path(__file__).resolve().parents[1]
    guest_repo = args.guest_repo or str(Path("/media/psf/Home") / repo.relative_to(Path.home()))
    harness = str(Path(guest_repo) / "test-environment")
    base = Path(args.guest_root)
    if args.command == "resume":
        return subprocess.call([prlctl, "start", args.vm])
    if args.command == "build":
        command = ["make", "-C", harness, f"BUILD={base / 'build'}", *extra]
    elif args.command == "log":
        command = ["tail", "-n", "100", str(base / "runtime/session.log")]
    else:
        command = ["python3", harness + "/native.py", args.command, "--runtime", str(base / "runtime")]
        if args.command == "start":
            command += ["--build", str(base / "build")]
        command += extra
    # prlctl joins exec arguments into a guest shell command. Quote every token
    # explicitly: normal subprocess argument boundaries alone are insufficient.
    result = subprocess.call([prlctl, "exec", args.vm, shlex.join(command)])
    if result == 0 and args.command == "screenshot":
        out = repo / "test-environment/.work/screenshot.ppm"
        out.parent.mkdir(exist_ok=True)
        guest_out = str(Path(harness) / ".work/screenshot.ppm")
        result = subprocess.call([prlctl, "exec", args.vm, shlex.join([
            "cp", str(base / "runtime/screenshot.ppm"), guest_out])])
        if result == 0:
            png = out.with_suffix(".png")
            result = subprocess.call(["sips", "-s", "format", "png", str(out), "--out", str(png)],
                                     stdout=subprocess.DEVNULL)
            if result == 0:
                print(png)
    return result


if __name__ == "__main__":
    sys.exit(main())
