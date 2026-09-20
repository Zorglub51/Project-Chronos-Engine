#!/usr/bin/env python3
"""Validate actual P7 SSH/SFTP binaries in private Linux namespaces.

No physical console, USB or block device is accessed. ARM execution registration
and 169.254.13.37 exist only in a new user/network/mount/PID namespace. The actual
P7 root is read-only; fake sysfs models the gadget and a dummy link models RNDIS.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time


def run(args, **kwargs):
    p = subprocess.run([str(a) for a in args], text=True, capture_output=True, timeout=30, **kwargs)
    if p.returncode:
        raise RuntimeError(f'{args[0]} failed ({p.returncode}):\n{p.stdout}\n{p.stderr}')
    return p.stdout + p.stderr


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def check(kit):
    result = {'passed': False, 'physical_console_tested': False, 'kernel': os.uname().release}
    with tempfile.TemporaryDirectory(prefix='chronos-remote-check-') as tmp:
        work = Path(tmp)
        root = work / 'root'; root.mkdir()
        print('Extracting P7', flush=True)
        run(['debugfs', '-R', f'rdump / {root}', kit / 'p7/mmcblk0p7-chronos.bin'])
        for path in ['run', 'tmp', 'data', 'sys', 'binfmt']:
            (work / path).mkdir()
        os.chmod(work / 'tmp', 0o1777)
        print('Preparing private ARM execution and network', flush=True)
        # Isolated binfmt_misc instance in this new user namespace only.
        run(['mount', '-t', 'binfmt_misc', 'none', work / 'binfmt'])
        qemu = Path(shutil.which('qemu-arm-static')).resolve()
        magic = r'\x7fELF\x01\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x02\x00\x28\x00'
        mask = r'\xff\xff\xff\xff\xff\xff\xff\x00\xff\xff\xff\xff\xff\xff\xff\xff\xfe\xff\xff\xff'
        (work / 'binfmt/register').write_text(f':chronos-arm:M::{magic}:{mask}:{qemu}:F\n')
        gadget = work / 'sys/devices/virtual/android_usb/android0'
        # Match the real A33 3.4.113 gadget: no writable USB string attributes.
        for name in ('enable', 'f_rndis/manufacturer',
                     'f_rndis/vendorID', 'f_rndis/wceis', 'idVendor', 'idProduct', 'functions', 'bDeviceClass'):
            p = gadget / name; p.parent.mkdir(parents=True, exist_ok=True); p.write_text('0\n')
        run(['ip', 'link', 'add', 'rndis0', 'type', 'dummy'])
        run(['ip', 'link', 'set', 'lo', 'up'])
        # New P7, but only writable runtime/P8/gadget stand-ins in the test.
        command = ['bwrap', '--die-with-parent', '--ro-bind', root, '/', '--dev', '/dev', '--proc', '/proc',
                   '--bind', work / 'sys', '/sys', '--bind', work / 'run', '/run',
                   '--bind', work / 'tmp', '/tmp', '--bind', work / 'data', '/rootfs_data',
                   '--cap-add', 'CAP_NET_ADMIN', '--cap-add', 'CAP_SETUID', '--cap-add', 'CAP_SETGID',
                   '--cap-add', 'CAP_CHOWN', '--cap-add', 'CAP_FOWNER',
                   '--cap-add', 'CAP_SYS_CHROOT', '--setenv', 'PATH', '/bin:/sbin:/usr/bin:/usr/sbin']
        p7_command = [str(a) for a in command]
        # Keep the parent namespace process alive while Dropbear daemonises.
        print('Starting actual P7 init script', flush=True)
        keeper_log = (work / 'keeper.log').open('w')
        keeper = subprocess.Popen(p7_command + ['/bin/sh', '-c',
            '/etc/init.d/S30chronos-remote start; while :; do sleep 1; done'],
            stdout=keeper_log, stderr=subprocess.STDOUT, text=True)
        try:
            for attempt in range(100):
                if keeper.poll() is not None:
                    raise RuntimeError('P7 boot test exited: ' + (work / 'keeper.log').read_text())
                try:
                    with socket.create_connection(('169.254.13.37', 22), timeout=.2) as sock:
                        banner = sock.makefile('rb').readline(256).decode('ascii')
                    break
                except OSError:
                    time.sleep(.1)
            else:
                raise RuntimeError('SSH did not start: ' + (work / 'tmp/chronos-remote.log').read_text())
            print('SSH banner received', flush=True)
            assert 'dropbear_2026.94' in banner, banner
            options = ['-o', 'BatchMode=yes', '-o', 'PreferredAuthentications=none',
                       '-o', 'StrictHostKeyChecking=accept-new', '-o', f'UserKnownHostsFile={work / "known_hosts"}',
                       '-o', 'ConnectTimeout=5', '-o', 'LogLevel=ERROR']
            def ssh(shell):
                return run(['ssh', *options, 'root@169.254.13.37', shell])
            print('Testing passwordless root shell', flush=True)
            output = ssh('id -u; printf CHRONOS_SSH_OK; test -r /etc/shadow')
            assert '0\nCHRONOS_SSH_OK' in output, output
            result['passwordless_root_ssh'] = True
            # Keep client stdin open until the short command exits. An inherited
            # /dev/null sends immediate EOF and hangs up the allocated terminal.
            with subprocess.Popen(['ssh', '-tt', *options, 'root@169.254.13.37',
                                   'test -t 0 && tty && printf CHRONOS_PTY_OK'],
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.STDOUT, text=True) as client:
                try:
                    client.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    client.kill()
                    raise
                terminal = client.communicate(timeout=3)[0]
                assert client.returncode == 0, terminal
            assert '/dev/pts/' in terminal and 'CHRONOS_PTY_OK' in terminal, terminal
            result['ssh_terminal'] = True
            # Prove that the exact image's BusyBox shell and SFTP helper execute.
            assert ssh('readlink /bin/sh').strip() in ('busybox', '/bin/busybox'), 'unexpected shell'
            listing = run(['ss', '-ltn'])
            assert '169.254.13.37:22' in listing and '0.0.0.0:22' not in listing and '*:22' not in listing, listing
            result['listen_usb_address_only'] = True
            before = (work / 'run/chronos-dropbear.pid').read_text()
            ssh('/etc/init.d/S30chronos-remote start')
            assert (work / 'run/chronos-dropbear.pid').read_text() == before
            result['idempotent_start'] = True
            key = work / 'data/chronos/ssh/dropbear_ed25519_host_key'
            key_hash = digest(key)
            assert key.stat().st_mode & 0o777 == 0o600
            payload = work / 'upload.bin'; payload.write_bytes(bytes(range(256)) * 8192)
            download = work / 'download.bin'
            batch = work / 'sftp.batch'
            batch.write_text(f'put {payload} /rootfs_data/sftp-upload.bin\n'
                             'rename /rootfs_data/sftp-upload.bin /rootfs_data/sftp-renamed.bin\n'
                             f'get /rootfs_data/sftp-renamed.bin {download}\n'
                             'rm /rootfs_data/sftp-renamed.bin\n')
            print('Testing SFTP transfer', flush=True)
            transfer = run(['sftp', *options, '-b', batch, 'root@169.254.13.37'])
            assert digest(payload) == digest(download)
            assert not (work / 'data/sftp-renamed.bin').exists()
            result['sftp_upload_download_rename_delete'] = True
            # Re-run startup after stopping just this test daemon. Under QEMU,
            # /proc/pid/exe points to the interpreter, so production -x matching
            # in start-stop-daemon is not used for this harness-only stop.
            os.kill(int(before.strip()), signal.SIGTERM)
            for _ in range(50):
                if not (work / 'run/chronos-dropbear.pid').exists(): break
                time.sleep(.1)
            started = time.monotonic()
            run(p7_command + ['/etc/init.d/S30chronos-remote', 'start'])
            result['start_returns_seconds'] = round(time.monotonic() - started, 3)
            for _ in range(50):
                try:
                    assert 'RESTART_OK' in ssh('printf RESTART_OK')
                    break
                except (RuntimeError, AssertionError): time.sleep(.1)
            else: raise RuntimeError('SSH restart failed')
            assert digest(key) == key_hash
            result['host_key_persisted_across_restart'] = True
            root_shadow = (root / 'etc/shadow').read_text()
            assert next(line for line in root_shadow.splitlines() if line.startswith('root:')).split(':')[1] == ''
            result['shadow_root_password_empty'] = True
            result['root_readonly'] = 'Read-only file system' in ssh('touch /etc/chronos-write-test 2>&1 || true')
            assert result['root_readonly']
            # Capture only public operational logs; never private key content.
            (kit / 'remote-validation.log').write_text(output + '\n' + transfer + '\n' + (work / 'tmp/chronos-remote.log').read_text())
            result['passed'] = True
        except BaseException:
            if (work / 'tmp/chronos-remote.log').exists():
                print((work / 'tmp/chronos-remote.log').read_text(), file=sys.stderr)
            raise
        finally:
            pid = work / 'run/chronos-dropbear.pid'
            if pid.exists():
                try: os.kill(int(pid.read_text().strip()), signal.SIGTERM)
                except (ProcessLookupError, ValueError): pass
            keeper.terminate()
            try: keeper.wait(timeout=3)
            except subprocess.TimeoutExpired:
                keeper.kill(); keeper.wait(timeout=3)
            keeper_log.close()
            run(['umount', work / 'binfmt'])
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('kit', type=Path)
    p.add_argument('--private', action='store_true', help=argparse.SUPPRESS)
    args = p.parse_args()
    if not args.private:
        if os.geteuid() != 0:
            p.error('Run as root in the Linux test VM (Python 3.12 or later).')
        # Map from the privileged parent so setgroups remains allowed. The
        # usual unshare --map-root-user disables it, preventing SSH sessions.
        ready_read, ready_write = os.pipe()
        mapped_read, mapped_write = os.pipe()
        child = os.fork()
        if child == 0:
            os.close(ready_read); os.close(mapped_write)
            try:
                os.unshare(os.CLONE_NEWUSER)
                os.write(ready_write, b'1'); os.close(ready_write)
                if os.read(mapped_read, 1) != b'1': os._exit(1)
                os.close(mapped_read)
                os.execvp('unshare', ['unshare', '--mount', '--mount-proc', '--net', '--pid', '--fork',
                                     sys.executable, str(Path(__file__).resolve()), str(args.kit.resolve()), '--private'])
            except BaseException:
                import traceback
                traceback.print_exc()
                os._exit(1)
        os.close(ready_write); os.close(mapped_read)
        try:
            if os.read(ready_read, 1) != b'1': raise RuntimeError('Private user namespace failed')
            Path(f'/proc/{child}/uid_map').write_text('0 0 65536\n')
            Path(f'/proc/{child}/gid_map').write_text('0 0 65536\n')
            os.write(mapped_write, b'1')
        finally:
            os.close(ready_read); os.close(mapped_write)
        return os.waitstatus_to_exitcode(os.waitpid(child, 0)[1])
    result = check(args.kit.resolve())
    (args.kit / 'remote-validation.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))
    return 0


if __name__ == '__main__':
    sys.exit(main())
