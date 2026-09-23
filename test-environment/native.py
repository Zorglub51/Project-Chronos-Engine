#!/usr/bin/env python3
"""Linux test harness for the original ARM32 M2 engine and Chronos folders.

Private runtime, original resources, and generated binaries are never committed.
The console hook is built unchanged and sees its real paths in a mount namespace.
"""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import struct
import subprocess
import sys
import time

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
MARKER = ".chronos-test.json"
PATCHES = ("const.nut.m", "mode_demo.nut.m", "mode_title_select.nut.m", "utils.nut.m")
KEYS = {"up": (3, 17, -1), "down": (3, 17, 1), "left": (3, 16, -1),
        "right": (3, 16, 1), "z": (1, 306, 1), "x": (1, 305, 1),
        "run": (1, 313, 1), "select": (1, 312, 1)}
SUPPORTED_BINARIES = {
    "b02848f66b82f8ac3090db523c4db9633508f9e3f53c7dc0ee3d01ce8aee8792": "stock 1006JP / 1006WW",
    "200044b9b0491302a0cac7830e6dd6ec2289b8315a5866eee952222f1de37cfb": "legacy VM platform patch",
}


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def runtime(path):
    root = Path(path).resolve()
    require((root / MARKER).is_file(), f"Not a prepared Chronos test runtime: {root}")
    return root


def prepare(args):
    """Create a new runtime. Never reuse a destination or import real saves."""
    root = Path(args.runtime).absolute()
    data = Path(args.data).resolve(strict=True)
    binary = Path(args.binary).resolve(strict=True)
    published = Path(args.published).resolve(strict=True)
    support = Path(args.support).resolve(strict=True)
    require(not root.exists() and not root.is_symlink(), f"Destination already exists: {root}")
    for source in (data, binary.parent, published, support):
        require(not root.resolve().is_relative_to(source), "Runtime must be outside input trees")
    digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    require(digest in SUPPORTED_BINARIES, f"Unsupported binary SHA-256: {digest}; hook addresses target retail 1006JP / 1006WW")
    require((data / "system/script/init.nut.m").is_file(), "Expected extracted stock resources, including init.nut.m")
    require((published / "folders/jp/_root/title_prof.psb.m").is_file(), "Missing published JP root pack")
    version = (support / "version").read_text().strip()
    require(version in ("1006JP", "1006WW"), f"Unsupported console version: {version}")
    package, motion_prefix = ("040", "jp") if version == "1006JP" else ("041", "us")
    require((data / package / "config/title_prof.psb.m").is_file(), f"Missing console {package} resources")
    packs = []
    for lineup in ("jp", "us"):
        for pack in sorted((published / "folders" / lineup).glob("*")):
            if not pack.is_dir():
                continue
            names = ("title_prof.psb.m", "title_mode_top.psb.m", f"title_{motion_prefix}_titleselect_{lineup}.psb.m")
            for name in names:
                require((pack / name).is_file(), f"Incomplete pack: {pack / name}")
            packs.append((lineup, pack, names))
    for name in ("version", "shutdown.png"):
        require((support / name).is_file(), f"Missing {support / name}")
    for name in PATCHES:
        require((REPO / "mod-assets/scripts-built" / name).is_file(), f"Missing Chronos script: {name}")

    root.mkdir(parents=True)
    game = root / "game"
    game.mkdir()
    # Copy scripts/configuration/graphics; ROMs are linked read-only in the sandbox.
    # This avoids another multi-GB ROM copy and keeps all mutable files private.
    for child in data.iterdir():
        if child.is_dir() and (child.name == "system" or child.name.isdecimal()):
            shutil.copytree(child, game / child.name,
                            ignore=lambda _, names: {"roms"} & set(names))
    roms = game / "system/roms"
    roms.mkdir(exist_ok=True)
    for rom_source in (data / "system/roms", published / "roms"):
        if rom_source.is_dir():
            for source in rom_source.iterdir():
                if source.is_file():
                    dest = roms / source.name
                    dest.unlink(missing_ok=True)
                    dest.symlink_to(source.resolve())
    shutil.copy2(binary, game / "m2engage")
    (game / "m2engage").chmod(0o755)
    for name in ("version", "shutdown.png"):
        shutil.copy2(support / name, game / name)
    for name in PATCHES:
        patch = REPO / "mod-assets/scripts-built" / name
        shutil.copy2(patch, game / "system/script" / patch.name)
    # Published Japanese font atlases contain the library's additional kanji.
    # Overlay only this known resource set; keep legacy exports usable.
    for name in ("makoto_basefont.psb.m", "makoto_basefont_18pt.psb.m",
                 "makoto_basefont_32pt.psb.m", "NotoSansCJK-OFL.txt"):
        source = published / "system/font" / name
        if source.is_file():
            (game / "system/font").mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, game / "system/font" / name)
    (game / "save").mkdir(exist_ok=True)
    for lineup, pack, names in packs:
        dest = root / "published/folders" / lineup / pack.name
        (dest / "saves").mkdir(parents=True)
        for name in names:
            shutil.copy2(pack / name, dest / name)
    (root / "published/folders/.current").write_text("jp/_root\n")
    (root / MARKER).write_text(json.dumps({
        "binary_sha256": digest, "binary_variant": SUPPORTED_BINARIES[digest],
        "data": str(data), "published": str(published), "console_package": package, "fresh_saves": True,
    }, indent=2) + "\n")
    print(f"Prepared {root} ({len(packs)} packs, fresh test saves)")


def identity(pid):
    try:
        # Field 22, after the parenthesised comm (which may itself contain spaces).
        fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
        return None if fields[0] == "Z" else fields[19]
    except (FileNotFoundError, ProcessLookupError):
        return None


def active(root):
    try:
        state = json.loads((root / "session.json").read_text())
        if identity(state["pid"]) == state["start_time"]:
            return state
    except FileNotFoundError:
        pass
    return None


def display_environment():
    env = os.environ.copy()
    env.setdefault("DISPLAY", ":0")
    if not env.get("XAUTHORITY"):
        authorities = sorted(Path("/run/user").glob("*/.mutter-Xwaylandauth.*"))
        require(len(authorities) == 1, "Set DISPLAY and XAUTHORITY for the Linux desktop session")
        env["XAUTHORITY"] = str(authorities[0])
    return env


def audio_environment(env, silent, user_runtime=Path("/run/user")):
    """Connect ARM32 OpenAL to the desktop's Pulse-compatible audio socket.

    start runs as root for mounts, so OpenAL cannot discover the user's server
    itself. PipeWire's Pulse socket works with the installed ARM32 libpulse and
    avoids depending on ARM32 PipeWire/ALSA plugins inside the VM.
    """
    env = env.copy()
    if silent:
        env["ALSOFT_DRIVERS"] = "null"
        return env
    if not env.get("PULSE_SERVER"):
        owners = []
        if env.get("XAUTHORITY"):
            try:
                owners.append(str(Path(env["XAUTHORITY"]).stat().st_uid))
            except OSError:
                pass
        if env.get("SUDO_UID"):
            owners.append(env["SUDO_UID"])
        candidates = [user_runtime / uid / "pulse/native" for uid in owners if uid.isdecimal()]
        server = next((path for path in candidates if path.is_socket()), None)
        if server is None:
            sockets = sorted(path for path in user_runtime.glob("*/pulse/native") if path.is_socket())
            require(len(sockets) <= 1, "Several desktop audio servers found; set PULSE_SERVER explicitly")
            server = sockets[0] if sockets else None
        if server is not None:
            env["PULSE_SERVER"] = "unix:" + str(server)
    if env.get("PULSE_SERVER"):
        env.setdefault("ALSOFT_DRIVERS", "pulse")
    return env


def sandbox_command(root, build, duration, silent):
    # / is read-only. /usr is reconstructed to add /usr/game without creating
    # directories on the VM host. Only the private test runtime is writable.
    cmd = ["bwrap", "--die-with-parent", "--unshare-pid", "--unshare-ipc"]
    for path in sorted(Path("/").iterdir()):
        if path.name in {"usr", "tmp", "dev", "proc", "mnt", "rootfs_data"}:
            continue
        if path.is_symlink():
            cmd += ["--symlink", os.readlink(path), str(path)]
        else:
            cmd += ["--ro-bind", str(path), str(path)]
    # Self-contained installs do not expose the shared Mac home at all. This
    # also catches accidental dependencies on it when testing a local launch.
    if json.loads((root / MARKER).read_text()).get("self_contained"):
        cmd += ["--tmpfs", "/media"]
    cmd += ["--dir", "/usr"]
    for path in sorted(Path("/usr").iterdir()):
        if path.name != "game":
            cmd += ["--ro-bind", str(path), str(path)]
    cmd += ["--bind", str(root), str(root),
            "--bind", str(root / "game"), "/usr/game",
            "--tmpfs", "/mnt", "--bind", str(root / "published"), "/mnt/usb/library/published",
            "--bind", str(root / "game/save"), "/rootfs_data",
            "--tmpfs", "/tmp", "--ro-bind", "/tmp/.X11-unix", "/tmp/.X11-unix",
            "--dev", "/dev", "--dev-bind", "/dev/dri", "/dev/dri",
            "--tmpfs", "/dev/shm", "--dir", "/dev/input",
            "--symlink", "/dev/null", "/dev/input/event4",
            "--symlink", "/dev/null", "/dev/input/event5",
            "--proc", "/proc", "--cap-add", "CAP_SYS_ADMIN",
            "--chdir", "/usr/game", sys.executable, str(HERE / "native.py"),
            "inside", "--runtime", str(root), "--build", str(build), "--duration", str(duration)]
    if silent:
        cmd.append("--silent")
    return cmd


def start(args):
    require(os.geteuid() == 0, "Run start as root inside the VM (sudo); mounts stay in its private namespace")
    root = runtime(args.runtime)
    build = Path(args.build).resolve()
    for name in ("gl_proxy", "libMali.so", "m2hook_print.so"):
        require((build / name).is_file(), f"Missing {build / name}; run make first")
    require(shutil.which("qemu-arm-static") and shutil.which("bwrap"), "Install qemu-user-static and bubblewrap")
    with (root / "session.lock").open("w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        require(not active(root), "This runtime already has a running session")
        # A console menu restart from a BACK pseudo-title currently crashes in
        # the native engine. Boot at the JP root unless reproducing that bug.
        if not args.resume_pack:
            (root / "published/folders/.current").write_text("jp/_root\n")
        env = audio_environment(display_environment(), args.silent)
        if args.debug:
            env.update(LIBMALI_DEBUG="1", GL_PROXY_DEBUG="1", M2HOOK_DEBUG="1")
        (root / "request.json").unlink(missing_ok=True)
        (root / "exit.json").unlink(missing_ok=True)
        with (root / "session.log").open("w") as log:
            command = [sys.executable, str(HERE / "native.py"), "supervise",
                       "--runtime", str(root), "--build", str(build), "--duration", str(args.duration)]
            if args.silent:
                command.append("--silent")
            child = subprocess.Popen(command,
                                     stdin=subprocess.DEVNULL, stdout=log, stderr=log,
                                     env=env, start_new_session=True)
        state = {"pid": child.pid, "start_time": identity(child.pid)}
        (root / "session.json").write_text(json.dumps(state) + "\n")
        time.sleep(1)
        require(child.poll() is None, f"Startup failed; read {root / 'session.log'}")
    print(f"Started test session {child.pid}; log: {root / 'session.log'}")


def supervise(args):
    root = runtime(args.runtime)
    result = subprocess.run(sandbox_command(root, Path(args.build).resolve(), args.duration, args.silent))
    (root / "exit.json").write_text(json.dumps({"exit_code": result.returncode}) + "\n")
    return result.returncode


def control(args):
    root = runtime(args.runtime)
    require(active(root), "No running test session")
    request = root / "request.json"
    require(not request.exists(), "A control request is already pending")
    if args.command == "screenshot":
        (root / "screenshot.ppm").unlink(missing_ok=True)
    temp = root / "request.tmp"
    temp.write_text(json.dumps({"command": args.command, "keys": getattr(args, "key", None),
                               "hold": getattr(args, "hold", 0.15)}))
    temp.replace(request)
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        ready = (root / "screenshot.ppm").exists() if args.command == "screenshot" else not request.exists()
        if ready:
            print(str(root / "screenshot.ppm") if args.command == "screenshot" else "Key sent")
            return
        require(active(root), "Session exited while processing the request")
        time.sleep(0.1)
    raise RuntimeError("Control request timed out; inspect session.log")


def stop(args):
    root = runtime(args.runtime)
    state = active(root)
    if not state:
        print("No running test session")
        return
    os.killpg(state["pid"], signal.SIGTERM)
    deadline = time.monotonic() + 5
    while identity(state["pid"]) == state["start_time"] and time.monotonic() < deadline:
        time.sleep(0.1)
    if identity(state["pid"]) == state["start_time"]:
        os.killpg(state["pid"], signal.SIGKILL)
    print("Stopped this test session; test saves kept")


def inside(args):
    import resource
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    root = runtime(args.runtime)
    build = Path(args.build).resolve()
    children = []
    stopping = False

    def terminate(signum, frame):
        nonlocal stopping
        stopping = True

    signal.signal(signal.SIGTERM, terminate)
    signal.signal(signal.SIGINT, terminate)
    try:
        proxy_env = os.environ.copy()
        proxy_env["CHRONOS_SCREENSHOT"] = str(root / "screenshot.ppm")
        proxy = subprocess.Popen([str(build / "gl_proxy")], env=proxy_env)
        children.append(proxy)
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline and not stopping:
            require(proxy.poll() is None, "Graphics proxy exited during startup")
            try:
                with open("/dev/shm/m2e_gl", "rb") as shm:
                    if shm.read(4) == struct.pack("<I", 1):
                        break
            except FileNotFoundError:
                pass
            time.sleep(0.05)
        else:
            raise RuntimeError("Graphics proxy did not become ready")
        Path("/tmp/sunxi_dump").touch()
        env = os.environ.copy()
        if args.silent:
            env["ALSOFT_DRIVERS"] = "null"
        print(f"[audio] driver={env.get('ALSOFT_DRIVERS', 'automatic')}, "
              f"server={env.get('PULSE_SERVER', 'automatic')}", flush=True)
        engine = subprocess.Popen([
            "qemu-arm-static", "-L", "/usr/arm-linux-gnueabihf",
            "-E", f"LD_LIBRARY_PATH={build}:/usr/lib/arm-linux-gnueabihf",
            "-E", f"LD_PRELOAD={build}/libMali.so:{build}/m2hook_print.so",
            "./m2engage"], env=env)
        children.append(engine)
        end = time.monotonic() + args.duration if args.duration else float("inf")
        while not stopping and time.monotonic() < end:
            if engine.poll() is not None:
                print(f"Engine exited: {engine.returncode}", flush=True)
                return engine.returncode
            if proxy.poll() is not None:
                raise RuntimeError("Graphics proxy exited; stopping engine")
            request = root / "request.json"
            if request.exists():
                action = json.loads(request.read_text())
                if action["command"] == "screenshot":
                    proxy.send_signal(signal.SIGUSR1)
                elif action["command"] == "key":
                    fd = os.open("/tmp/m2e_input0", os.O_WRONLY | os.O_NONBLOCK)
                    with os.fdopen(fd, "wb", buffering=0) as fifo:
                        for pressed in (True, False):
                            for key in action["keys"]:
                                event_type, code, value = KEYS[key]
                                fifo.write(struct.pack("<IIHHi", 0, 0, event_type, code, value if pressed else 0))
                            fifo.write(struct.pack("<IIHHi", 0, 0, 0, 0, 0))
                            time.sleep(action["hold"] if pressed else 0.15)
                request.unlink()
            time.sleep(0.1)
        return 0
    finally:
        for child in reversed(children):
            if child.poll() is None:
                child.terminate()
        for child in children:
            try:
                child.wait(timeout=3)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    p = commands.add_parser("prepare")
    for name in ("runtime", "data", "binary", "support", "published"):
        p.add_argument("--" + name, required=True)
    for command in ("start", "inside", "supervise", "stop", "status", "key", "screenshot"):
        p = commands.add_parser(command)
        p.add_argument("--runtime", required=True)
        if command == "key":
            p.add_argument("--key", required=True, choices=KEYS, action="append", help="Repeat to hold a chord")
            p.add_argument("--hold", type=float, default=0.15, help="Hold for 0.05 to 5 seconds")
        if command in ("start", "inside", "supervise"):
            p.add_argument("--build", default=str(HERE / "build"))
            p.add_argument("--duration", type=int, default=0, help="Stop after N seconds; 0 = interactive")
            p.add_argument("--silent", action="store_true", help="Use OpenAL's null output for automated tests")
        if command == "start":
            p.add_argument("--debug", action="store_true")
            p.add_argument("--resume-pack", action="store_true", help="Diagnostic: keep .current instead of booting JP root")
    args = parser.parse_args()
    if hasattr(args, "duration"):
        require(args.duration >= 0, "Duration must be nonnegative")
    if args.command == "key":
        require(0.05 <= args.hold <= 5, "Key hold must be between 0.05 and 5 seconds")
    if args.command in ("key", "screenshot"):
        return control(args)
    if args.command == "status":
        root = runtime(args.runtime)
        print(json.dumps({"running": bool(active(root)), "runtime": str(root),
                          "pack": (root / "published/folders/.current").read_text().strip()}, indent=2))
        return 0
    return globals()[args.command](args) or 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (RuntimeError, OSError, ValueError) as error:
        print(f"Error: {error}", file=sys.stderr)
        sys.exit(1)
