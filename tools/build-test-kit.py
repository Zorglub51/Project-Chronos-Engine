#!/usr/bin/env python3
"""Build a private P7 + USB test kit from the owner's stock dump/resources.

Never connects to a console or writes a disk device. Outputs must be new.
The source P7 and the reference library are opened read-only.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import zipfile

REPO = Path(__file__).resolve().parents[1]
P7_SIZE = 104857600
STOCK_P7_SHA = "d04569a81eea5b07c2d426e93359ccadce0551b3a8155407fe8d71e0fd6f1d7e"
ENGINE_SHA = "b02848f66b82f8ac3090db523c4db9633508f9e3f53c7dc0ee3d01ce8aee8792"
FILES = {
    "mount-usb-drives": ("/bin/mount-usb-drives", 0o755),
    "gameapp": ("/etc/init.d/gameapp", 0o755),
    "S11chronos-usb": ("/etc/init.d/S11chronos-usb", 0o755),
    "S30chronos-remote": ("/etc/init.d/S30chronos-remote", 0o755),
    "pce-remote-lib.sh": ("/usr/bin/pce-remote-lib.sh", 0o755),
    "pce-usb-lib.sh": ("/usr/bin/pce-usb-lib.sh", 0o755),
    "pce-usb-attach": ("/usr/bin/pce-usb-attach", 0o755),
    "pce-usb-detach": ("/usr/bin/pce-usb-detach", 0o755),
    "99-pce-usb.rules": ("/etc/udev/rules.d/99-pce-usb.rules", 0o644),
}
TEMPLATES = (
    "040/config/title_prof.psb.m", "040/config/title_mode_top.psb.m",
    "040/motion/title_jp_titleselect_jp.psb.m", "040/motion/title_jp_titleselect_us.psb.m",
)
# Root HuCard, three folders (HuCard/SGX/CD), and two US entries.
GAMES = (
    ("jp/GAME000", "jp/GAME000"),
    ("jp/FOLDER_NAMCOT/GAME053", "jp/FOLDER_HUCARD/GAME053"),
    ("jp/FOLDER_SGX/GAME007", "jp/FOLDER_SGX/GAME007"),
    ("jp/GAME002", "jp/FOLDER_CD/GAME002"),
    ("us/GAME001", "us/GAME001"),
    ("us/GAME011", "us/GAME011"),
)


def require(ok, message):
    if not ok:
        raise RuntimeError(message)


def sha(path):
    h = hashlib.sha256()
    with Path(path).open("rb") as f:
        for block in iter(lambda: f.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def run(args, log=None):
    p = subprocess.run([str(x) for x in args], text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    if log:
        Path(log).write_text(p.stdout)
    require(p.returncode == 0, f"Command failed ({p.returncode}): {args[0]}\n{p.stdout}")
    return p.stdout


def quote(path):
    text = str(path)
    require(not any(c in text for c in '\n\r"\\'), "Unsupported path for debugfs")
    return '"' + text + '"'


def json_file(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def blank_root_password(contents):
    lines = contents.splitlines(keepends=True)
    roots = 0
    for i, line in enumerate(lines):
        fields = line.split(":")
        if fields[0] == "root":
            require(len(fields) == 9, "Invalid root shadow entry")
            fields[1] = ""
            lines[i] = ":".join(fields)
            roots += 1
    require(roots == 1, "Expected exactly one root entry in /etc/shadow")
    return "".join(lines)


def remote_files():
    root = REPO / "console-mod/remote-access/bin"
    manifest = json.loads((root / "manifest.json").read_text())
    destinations = {"dropbear": "/usr/sbin/dropbear", "dropbearkey": "/usr/bin/dropbearkey",
                    "sftp-server": "/usr/libexec/sftp-server"}
    require({f["name"] for f in manifest["files"]} == set(destinations), "Incomplete remote-access build")
    files = []
    for entry in manifest["files"]:
        path = root / entry["name"]
        require(sha(path) == entry["sha256"] and path.stat().st_size == entry["size"],
                f"Remote-access binary differs from its manifest: {path}")
        header = path.read_bytes()[:20]
        require(header[:6] == b"\x7fELF\x01\x01" and header[18:20] == b"\x28\x00", "Expected ARM32 little-endian ELF")
        files.append((path, destinations[entry["name"]], 0o755))
    files.append((root / "manifest.json", "/usr/share/chronos-remote/manifest.json", 0o644))
    for name in ("musl.txt", "dropbear.txt", "openssh.txt"):
        files.append((root / "licenses" / name, "/usr/share/chronos-remote/" + name, 0o644))
    return files, manifest


def build_p7(args, out):
    stock = args.p7.resolve()
    require(stock.stat().st_size == P7_SIZE and sha(stock) == STOCK_P7_SHA,
            "P7 must be the verified, clean JP stock image (100 MiB)")
    folder = out / "p7"
    folder.mkdir()
    image = folder / "mmcblk0p7-chronos.bin"
    shutil.copyfile(stock, image)
    run([args.fsck, "-fn", stock], folder / "fsck-stock.txt")
    original = folder / "gameapp.original"
    run([args.debugfs, "-R", f"dump /etc/init.d/gameapp {quote(original)}", stock])
    require(original.is_file() and original.stat().st_size > 0, "Cannot read stock gameapp")
    remote, remote_manifest = remote_files()
    # Read the original shadow only into a private temporary file. Ship the
    # modified copy with root's second field empty, preserving every other field.
    with tempfile.NamedTemporaryFile(dir=folder) as temp:
        run([args.debugfs, "-R", f"dump /etc/shadow {quote(temp.name)}", stock])
        original_shadow = Path(temp.name).read_text()
    shadow = folder / "shadow.chronos"
    shadow.write_text(blank_root_password(original_shadow))
    shadow.chmod(0o600)
    directories = ["/mnt/usb", "/usr/libexec", "/usr/share/chronos-remote"]
    commands = [f"mkdir {path}" for path in directories]
    replacements = [(original, "/etc/init.d/gameapp.orig", 0o755)]
    replacements += [(REPO / "console-mod" / name, dest, mode) for name, (dest, mode) in FILES.items()]
    replacements += remote + [(shadow, "/etc/shadow", 0o600)]
    for src, dest, mode in replacements:
        if dest in ("/etc/init.d/gameapp", "/etc/shadow"):
            commands.append(f"rm {dest}")
        commands += [f"write {quote(src)} {dest}",
                     f"set_inode_field {dest} mode 0{0o100000 | mode:o}",
                     f"set_inode_field {dest} uid 0", f"set_inode_field {dest} gid 0"]
    batch = folder / "debugfs.commands"
    batch.write_text("\n".join(commands) + "\n")
    run([args.debugfs, "-w", "-f", batch, image], folder / "debugfs-build.txt")
    checks = []
    verify = folder / "verified-files"
    verify.mkdir()
    for i, (src, dest, mode) in enumerate(replacements):
        dump = verify / str(i)
        run([args.debugfs, "-R", f"dump {dest} {quote(dump)}", image])
        require(dump.is_file() and sha(dump) == sha(src), f"Image file mismatch: {dest}")
        stat = run([args.debugfs, "-R", f"stat {dest}", image])
        require(f"Mode:  0{mode:o}" in stat and "User:     0   Group:     0" in stat,
                f"Wrong mode/ownership: {dest}\n{stat}")
        checks.append({"path": dest, "mode": f"{mode:04o}", "uid": 0, "gid": 0, "sha256": sha(src)})
    # rcS/inittab remain byte-for-byte stock; S11 runs after read-only remounts.
    for name in ("/etc/inittab", "/etc/init.d/rcS", "/etc/fstab", "/etc/passwd"):
        a = run([args.debugfs, "-R", f"cat {name}", stock])
        b = run([args.debugfs, "-R", f"cat {name}", image])
        require(a == b, f"Unexpected change: {name}")
    run([args.fsck, "-fn", image], folder / "fsck-chronos.txt")
    require(image.stat().st_size == P7_SIZE, "P7 output size changed")
    manifest = {"partition": 7, "device": "/dev/mmcblk0p7", "size": P7_SIZE,
                "source_sha256": STOCK_P7_SHA, "sha256": sha(image),
                "files": checks, "directories_created": directories,
                "remote_access": {"address": "169.254.13.37", "interface": "rndis0", "port": 22,
                                  "protocols": ["ssh", "sftp"], "user": "root", "root_password_empty": True,
                                  "bind_usb_address_only": True, "host_keys_generated_on_console": True,
                                  "persistent_key_directory": "/rootfs_data/chronos/ssh",
                                  "binary_bytes": sum(f["size"] for f in remote_manifest["files"])},
                "stock_rcS_inittab_fstab_unchanged": True, "fsck_readonly_passed": True,
                "kernel_partition_included": False, "flashed": False}
    json_file(folder / "manifest.json", manifest)
    print(f"P7 built and checked: {image}", flush=True)
    return image, manifest


def build_usb(args, out):
    stock_game = args.stock_game.resolve()
    require(sha(stock_game / "m2engage") == ENGINE_SHA, "Unsupported native engine; expected stock JP 1006JP")
    index_dir = out / "archive-index"
    run([sys.executable, args.psb_tool, "extract", stock_game / "alldata.psb.m", index_dir])
    entries = json.loads((index_dir / "alldata.json").read_text())["file_info"]
    usb = out / "USB"
    usb.mkdir()
    game = usb / "game"
    game.mkdir()
    total = (stock_game / "alldata.bin").stat().st_size
    with (stock_game / "alldata.bin").open("rb") as archive:
        def extract(relative, destination):
            offset, size = entries[relative]
            require(offset >= 0 and size >= 0 and offset + size <= total, f"Invalid archive range: {relative}")
            archive.seek(offset)
            data = archive.read(size)
            require(len(data) == size, f"Short archive read: {relative}")
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(data)

        for name in entries:
            path = Path(name)
            require(not path.is_absolute() and ".." not in path.parts, f"Invalid archive path: {name}")
            if name.startswith("system/roms/"):
                continue
            extract(name, game / name)
        for name in ("m2engage", "libopus.so.0", "version", "shutdown.png"):
            shutil.copyfile(stock_game / name, game / name)
        (game / "m2engage").chmod(0o755)
        (game / "system/roms").mkdir(exist_ok=True)
        (game / "save").mkdir()
        library = usb / "library"
        library.mkdir()
        for name in TEMPLATES:
            dest = library / "templates" / name
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(game / name, dest)
        chosen = []
        for source, target in GAMES:
            src = args.library / source
            dst = library / target
            dst.mkdir(parents=True)
            data = json.loads((src / "game.json").read_text())
            rom = Path(data["rom"]["rom"]).name
            # Always use the original archive's game data, never previous saves.
            extract("system/roms/" + rom, dst / rom)
            data["rom"]["rom"] = rom
            for name in ("cover.png",):
                if (src / name).is_file():
                    shutil.copyfile(src / name, dst / name)
            json_file(dst / "game.json", data)
            chosen.append({"path": target, "title": data["display"].get("name_eng") or data["display"]["name"],
                           "arch": data["rom"]["arch"], "rom": rom, "sha256": sha(dst / rom)})
    folders = [("FOLDER_HUCARD", "HuCard", "GAME053"), ("FOLDER_SGX", "SuperGrafx", "GAME007"), ("FOLDER_CD", "CD-ROM", "GAME002")]
    def gamelist(path, names):
        json_file(path / "gamelist.json", [{"folder": name, **{key: i for key in
                  ("sor_date", "sor_demo", "sor_genr", "sor_name", "sor_pnum")}} for i, name in enumerate(names)])
    gamelist(library / "jp", ["GAME000"] + [f[0] for f in folders])
    gamelist(library / "us", ["GAME001", "GAME011"])
    for ident, label, child in folders:
        folder = library / "jp" / ident
        json_file(folder / "folder.json", {"name": label})
        gamelist(folder, [child])
        shutil.copyfile(folder / child / "cover.png", folder / "cover.png")
    published = library / "published"
    run([args.publisher, library, library / "templates", published], out / "publish.txt")
    (published / "folders/.current").write_text("jp/_root\n")
    require(not list((game / "system/roms").iterdir()), "ROMs were duplicated in game/")
    require(not list((game / "save").iterdir()), "Live save mount point must be empty")
    require(not any(p.is_symlink() for p in usb.rglob("*")), "FAT32 layout contains a symlink")
    require(len(list((published / "roms").iterdir())) == len(GAMES), "Wrong published ROM count")
    for asset in (REPO / "mod-assets/scripts-built").glob("*.nut.m"):
        require(sha(asset) == sha(game / "system/script" / asset.name), "Chronos script mismatch")
    require(sha(REPO / "mod-assets/lib/m2hook_print.so") == sha(game / "lib/m2hook_print.so"), "Hook mismatch")
    manifest = {"engine_sha256": ENGINE_SHA, "games": chosen, "folder_count": 3,
                "menu_pack_count": 5, "original_resources_extracted_directly": True,
                "rom_mount_point_empty": True, "save_mount_point_empty": True,
                "user_saves_imported": False}
    json_file(out / "usb-manifest.json", manifest)
    print(f"USB example built: {usb} ({len(chosen)} games, 3 folders)", flush=True)
    return usb, manifest


def package(root, output, names=None):
    with zipfile.ZipFile(output, "x", zipfile.ZIP_DEFLATED, compresslevel=6) as z:
        paths = [root / n for n in names] if names else sorted(root.rglob("*"))
        for path in paths:
            if path.is_dir():
                z.writestr(str(path.relative_to(root)).replace("\\", "/") + "/", b"")
            else:
                z.write(path, path.relative_to(root))
    with zipfile.ZipFile(output) as z:
        require(z.testzip() is None, f"Corrupt ZIP: {output}")


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ("p7", "output", "debugfs", "fsck"):
        p.add_argument("--" + name, type=Path, required=True)
    for name in ("stock-game", "library", "psb-tool", "publisher"):
        p.add_argument("--" + name, type=Path)
    p.add_argument("--p7-only", action="store_true", help="Build P7/ZIP without rebuilding an existing USB library")
    args = p.parse_args()
    if not args.p7_only and not all((args.stock_game, args.library, args.psb_tool, args.publisher)):
        p.error("Full kit requires --stock-game, --library, --psb-tool and --publisher")
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    require(not (out / "p7").exists() and not (out / "USB").exists(), "Output already contains a kit; choose a new destination")
    image, p7 = build_p7(args, out)
    if args.p7_only:
        text = f'''Chronos — P7 avec SSH et SFTP

Restaurer UNIQUEMENT P7 (Linux) dans la version de PCE Mini Recovery prenant
les ZIP en charge : sélectionner directement chronos-p7.zip. Avec une ancienne
version, décompresser le ZIP et choisir mmcblk0p7-chronos.bin ({P7_SIZE} octets).
P6, P8 et P9 ne sont pas incluses dans cette image. La clé Chronos déjà préparée
reste compatible.

Après démarrage normal de la console :
- adresse USB RNDIS : 169.254.13.37/16 ;
- SSH et SFTP : port 22, utilisateur root, mot de passe vide ;
- SSH : ssh root@169.254.13.37 ;
- SFTP : sftp root@169.254.13.37 (également utilisable avec un client graphique).

Sur ce Mac, le pont USB RNDIS de PCE Mini Recovery doit être démarré avec
« Start bridge ». Il est utilisable en démarrage normal : ne pas cliquer sur
« Start recovery » pour accéder aux services installés dans P7.
Le port micro-USB sert à la liaison avec le Mac ; la clé utilise le port hôte.

Les jeux sont dans /mnt/usb/library/published/roms et leurs sauvegardes dans
/mnt/usb/library/published/save. Les mêmes données sont montées sous /usr/game.
Le système P7 démarre en lecture seule comme le firmware d'origine. Pour une
modification administrative via SSH/SFTP : mount -o remount,rw / ; puis terminer
avec sync et mount -o remount,ro /.

Le service écoute uniquement sur 169.254.13.37. Les clés Ed25519 sont créées au
premier démarrage dans /rootfs_data/chronos/ssh (stockage persistant sur P8) ;
aucune clé privée commune n'est contenue dans l'image. Le champ du mot de passe
root est vide dans /etc/shadow ; les autres comptes sont conservés.
Les attentes réseau et la génération de clé tournent en arrière-plan.
Journal : /tmp/chronos-remote.log.

P7 SHA-256 : {p7['sha256']}
Cette construction ne flashe pas la console. Le test matériel reste à effectuer.
'''
        (out / "LIRE-MOI.txt").write_text(text)
        shutil.copyfile(out / "LIRE-MOI.txt", out / "p7/LIRE-MOI.txt")
        package(out / "p7", out / "chronos-p7.zip", [image.name, "manifest.json", "LIRE-MOI.txt"])
        artifacts = [image, out / "chronos-p7.zip"]
        (out / "SHA256SUMS.txt").write_text("".join(f"{sha(x)}  {x.relative_to(out)}\n" for x in artifacts))
        print((out / "SHA256SUMS.txt").read_text(), flush=True)
        return
    usb, library = build_usb(args, out)
    text = f'''Chronos — premier test sur PC Engine Mini JP

1. Dans PCE Mini Recovery avec support ZIP, restaurer UNIQUEMENT P7 (Linux)
   en sélectionnant chronos-p7.zip. Une ancienne version nécessite le fichier
   mmcblk0p7-chronos.bin décompressé : {P7_SIZE} octets.
   Le kernel actuel (P6), les sauvegardes internes (P8) et les jeux (P9) restent
   en place. La sauvegarde de gameapp d'origine est conservée dans P7.
2. Copier le CONTENU du dossier USB (game et library) à la racine d'une clé
   FAT32 à une partition. Ne pas copier le dossier USB lui-même comme parent.
   Aucun lien symbolique. game/system/roms et game/save sont des points de
   montage vides ; les fichiers utilisés sont dans library/published.
3. Éteindre la console, brancher cette clé sur son port USB hôte, puis démarrer.
   Le kernel déjà installé doit prendre en charge le stockage USB/FAT32.
4. Tester The Kung Fu à la racine JP, entrer dans HuCard, SuperGrafx et CD-ROM,
   puis changer JP → US → JP. Tester RUN+SELECT, une sauvegarde et le retour
   au menu. Éteindre normalement et vérifier la persistance au redémarrage.
5. Tester aussi un démarrage sans clé : le menu stock doit fonctionner.
   Faire d'abord ces essais avant le retrait à chaud ; le retrait direct
   ne garantit pas la conservation d'une sauvegarde en cours.

La bibliothèque contient 6 jeux tirés de ton archive d'origine, et aucune
sauvegarde personnelle. Elle s'ouvre dans l'éditeur en sélectionnant le dossier
USB ou la racine de la clé. Publier met à jour les packs et les fichiers Chronos.

SSH/SFTP : root@169.254.13.37, port 22, mot de passe vide. Sur ce Mac, activer
le pont RNDIS avec Start bridge dans Recovery, sans lancer Start recovery.
Journal : /tmp/chronos-remote.log. Clé SSH propre à chaque console dans
/rootfs_data/chronos/ssh, créée au premier démarrage.

P7 SHA-256 : {p7['sha256']}
Moteur : JP 1006JP, SHA-256 {ENGINE_SHA}
Vérifications locales : taille P7, e2fsck en lecture seule, fichiers/modes/root,
scripts de démarrage, publication et absence de ROMs dupliquées dans game.
Ce kit n'a pas été flashé automatiquement. Le test matériel reste à effectuer.
'''
    (out / "LIRE-MOI.txt").write_text(text)
    shutil.copyfile(out / "LIRE-MOI.txt", out / "p7/LIRE-MOI.txt")
    shutil.copyfile(out / "LIRE-MOI.txt", usb / "LIRE-MOI.txt")
    package(out / "p7", out / "chronos-p7.zip", [image.name, "manifest.json", "LIRE-MOI.txt"])
    package(usb, out / "chronos-usb-exemple.zip")
    artifacts = [image, out / "chronos-p7.zip", out / "chronos-usb-exemple.zip"]
    (out / "SHA256SUMS.txt").write_text("".join(f"{sha(x)}  {x.relative_to(out)}\n" for x in artifacts))
    print((out / "SHA256SUMS.txt").read_text(), flush=True)


if __name__ == "__main__":
    main()
