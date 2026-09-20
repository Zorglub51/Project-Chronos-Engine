#!/usr/bin/env python3
"""Cross-build minimal static ARMv7 SSH/SFTP utilities on Linux.

Requires gcc-arm-linux-gnueabihf, make, Python 3, and the three pinned release
archives below in --sources. Build in a new directory on a Linux filesystem.
No system install or console access. Only the server, key tool and SFTP helper
are installed into --output, together with their licences and build manifest.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile

SOURCES = {
    'musl-1.2.6.tar.gz': ('d585fd3b613c66151fc3249e8ed44f77020cb5e6c1e635a616d3f9f82460512a', 'https://www.musl-libc.org/releases/musl-1.2.6.tar.gz'),
    'dropbear-2026.94.tar.bz2': ('e098034a843699200c8c977a991fff73159735bf795d5f72ef672c41a6b1ae81', 'https://dropbear.nl/mirror/releases/dropbear-2026.94.tar.bz2'),
    'openssh-10.5p1.tar.gz': ('d44d28a839ea9daf969cc69150fde59910b2b39361dad81a3bd6cbd19218db11', 'https://cdn.openbsd.org/pub/OpenBSD/OpenSSH/portable/openssh-10.5p1.tar.gz'),
}
OPTIONS = '''/* Chronos A33: SSH shell + SFTP, no persistent auxiliary services. */
#define DROPBEAR_SVR_AGENTFWD 0
#define DROPBEAR_X11FWD 0
#define DROPBEAR_SVR_LOCALTCPFWD 0
#define DROPBEAR_SVR_REMOTETCPFWD 0
#define DROPBEAR_SFTPSERVER 1
#define SFTPSERVER_PATH "/usr/libexec/sftp-server"
#define DROPBEAR_RSA 0
#define DROPBEAR_ECDSA 0
#define DROPBEAR_ED25519 1
#define DROPBEAR_DH_GROUP14_SHA256 0
#define DROPBEAR_DH_GROUP16 0
#define DROPBEAR_ECDH 0
#define DROPBEAR_CURVE25519 1
'''


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--sources', type=Path, required=True)
    parser.add_argument('--work', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--jobs', type=int, default=4)
    args = parser.parse_args()
    work = args.work.resolve(); output = args.output.resolve()
    work.mkdir(parents=True, exist_ok=False)
    output.mkdir(parents=True, exist_ok=True)
    logs = work / 'logs'; logs.mkdir()

    def run(command, directory, label, env=None):
        print(label, flush=True)
        with (logs / (label + '.log')).open('w') as stream:
            p = subprocess.run([str(x) for x in command], cwd=directory, env=env,
                               stdout=stream, stderr=subprocess.STDOUT)
        if p.returncode:
            raise RuntimeError((logs / (label + '.log')).read_text()[-6000:])

    for name, (digest, _) in SOURCES.items():
        archive = args.sources / name
        if sha(archive) != digest:
            raise RuntimeError('Source checksum mismatch: ' + name)
        with tarfile.open(archive) as tar:
            tar.extractall(work, filter='data')
    jobs = '-j' + str(args.jobs)
    common = dict(os.environ, CFLAGS='-Os -march=armv7-a -mfpu=vfpv3-d16 -mfloat-abi=hard -ffunction-sections -fdata-sections',
                  AR='arm-linux-gnueabihf-ar', RANLIB='arm-linux-gnueabihf-ranlib')
    musl = work / 'musl-1.2.6'; prefix = work / 'musl-toolchain'
    env = dict(common, CC='arm-linux-gnueabihf-gcc')
    run(['./configure', '--target=arm-linux-gnueabihf', '--disable-shared', '--prefix=' + str(prefix)], musl, 'musl-configure', env)
    run(['make', jobs], musl, 'musl-build', env)
    run(['make', 'install'], musl, 'musl-install', env)
    env = dict(common, CC=str(prefix / 'bin/musl-gcc'), LDFLAGS='-static -Wl,--gc-sections')
    dropbear = work / 'dropbear-2026.94'
    (dropbear / 'localoptions.h').write_text(OPTIONS)
    run(['./configure', '--host=arm-linux-gnueabihf', '--disable-zlib', '--disable-pam', '--enable-static'], dropbear, 'dropbear-configure', env)
    run(['make', jobs, 'PROGRAMS=dropbear dropbearkey'], dropbear, 'dropbear-build', env)
    openssh = work / 'openssh-10.5p1'
    run(['./configure', '--host=arm-linux-gnueabihf', '--without-openssl', '--without-zlib', '--without-pam',
         '--without-selinux', '--without-kerberos5', '--without-libedit', '--without-security-key-builtin',
         '--disable-utmp', '--disable-wtmp', '--disable-lastlog'], openssh, 'sftp-configure', env)
    run(['make', jobs, 'sftp-server'], openssh, 'sftp-build', env)
    files = []
    for source, name in [(dropbear / 'dropbear', 'dropbear'), (dropbear / 'dropbearkey', 'dropbearkey'),
                         (openssh / 'sftp-server', 'sftp-server')]:
        dest = output / name
        shutil.copyfile(source, dest); dest.chmod(0o755)
        run(['arm-linux-gnueabihf-strip', dest], work, 'strip-' + name)
        headers = subprocess.check_output(['arm-linux-gnueabihf-readelf', '-h', '-l', str(dest)], text=True)
        if 'INTERP' in headers or 'Machine:                           ARM' not in headers:
            raise RuntimeError('Expected static ARM binary: ' + name)
        files.append({'name': name, 'size': dest.stat().st_size, 'sha256': sha(dest)})
    licences = output / 'licenses'; licences.mkdir(exist_ok=True)
    for source, name in [(musl / 'COPYRIGHT', 'musl.txt'), (dropbear / 'LICENSE', 'dropbear.txt'), (openssh / 'LICENCE', 'openssh.txt')]:
        shutil.copyfile(source, licences / name)
    (output / 'manifest.json').write_text(json.dumps({'architecture': 'ARMv7 hard-float, static musl',
        'sources': {name: {'sha256': digest, 'url': url} for name, (digest, url) in SOURCES.items()},
        'files': files, 'dropbear_localoptions': OPTIONS}, indent=2) + '\n')
    print(json.dumps(files, indent=2), flush=True)


if __name__ == '__main__':
    main()
