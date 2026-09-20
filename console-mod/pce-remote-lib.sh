#!/bin/sh
# USB-only SSH + on-demand SFTP. Shared by the boot entry and local tests.
REMOTE_IP=169.254.13.37
REMOTE_IF=rndis0
GADGET=/sys/devices/virtual/android_usb/android0
KEY_DIR=/rootfs_data/chronos/ssh
DROPBEAR=/usr/sbin/dropbear
DROPBEARKEY=/usr/bin/dropbearkey
REMOTE_PID=/run/chronos-dropbear.pid

remote_node() {
    [ -e "$GADGET/$1" ] || { echo "Missing USB gadget attribute: $1"; return 1; }
    [ "$(cat "$GADGET/$1")" = "$2" ] || printf '%s\n' "$2" > "$GADGET/$1"
}

remote_descriptor() {
    # Some A33 kernels (including 3.4.113) keep these strings fixed and do not
    # expose their sysfs attributes. VID/PID and RNDIS controls remain required.
    [ -e "$GADGET/$1" ] || return 0
    remote_node "$1" "$2"
}

remote_network() {
    [ -d "$GADGET" ] || { echo 'USB RNDIS gadget unavailable in this kernel'; return 1; }
    remote_node enable 0 &&
    remote_descriptor iManufacturer Chronos &&
    remote_descriptor iProduct classic &&
    remote_descriptor iSerial 3730BC73B12E00EC &&
    remote_node f_rndis/manufacturer Chronos &&
    remote_node f_rndis/vendorID 057e &&
    remote_node f_rndis/wceis 1 &&
    remote_node idVendor 04e8 &&
    remote_node idProduct 6863 &&
    remote_node functions rndis &&
    remote_node bDeviceClass 224 &&
    remote_node enable 1 || { echo 'Cannot configure the USB RNDIS gadget'; return 1; }
    n=0
    while ! ip link show "$REMOTE_IF" >/dev/null 2>&1; do
        n=$((n + 1))
        [ "$n" -lt 20 ] || { echo 'RNDIS interface did not appear'; return 1; }
        sleep 0.5
    done
    ip link set lo up || return 1
    if ! ip -4 addr show dev "$REMOTE_IF" | grep -q "inet $REMOTE_IP/16 "; then
        ip addr add "$REMOTE_IP/16" dev "$REMOTE_IF" || return 1
    fi
    ip link set "$REMOTE_IF" up || return 1
}

remote_start() {
    umask 077
    remote_network || return 1
    # P8 is writable even when the stock boot remounts P7 read-only. Generate
    # a per-console identity, once; no private key is distributed in the image.
    if ! mkdir -p "$KEY_DIR"; then
        KEY_DIR=/run/chronos-ssh
        mkdir -p "$KEY_DIR" || return 1
        echo 'Persistent key storage unavailable; using a temporary host key'
    fi
    chmod 700 "$KEY_DIR" || return 1
    key="$KEY_DIR/dropbear_ed25519_host_key"
    if [ ! -s "$key" ]; then
        rm -f "$key.new"
        "$DROPBEARKEY" -t ed25519 -f "$key.new" || return 1
        chmod 600 "$key.new" && mv "$key.new" "$key" || return 1
        sync
    fi
    # Bind explicitly to the USB address. SFTP is spawned by Dropbear only
    # during a file-transfer session, with no second listening daemon.
    "$DROPBEAR" -E -B -p "$REMOTE_IP:22" -r "$key" -P "$REMOTE_PID" || return 1
    echo "SSH/SFTP ready at $REMOTE_IP:22 (root, empty password)"
}
