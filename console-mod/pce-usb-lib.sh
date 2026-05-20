#!/bin/sh
# Shared bind/unbind logic for USB hot-plug + boot handlers.

USB_MNT=/mnt/usb
USB_GAME=${USB_MNT}/game
LIVE_GAME=/usr/game
LOG=/tmp/pce-usb.log

pce_log() { echo "[$(date +%H:%M:%S)] $*" >>${LOG}; }

# Pop every /dev/sd* mount layer off `target`. Count first, then umount
# exactly that many times — guards against `umount -l` async finalization
# races that could otherwise pop the underlying ext4 mount.
pop_sd_binds_on() {
    target=$1
    count=$(mount | awk -v t="$target" '$3 == t && $1 ~ /^\/dev\/sd[a-z]/' | wc -l)
    [ "$count" -gt 0 ] 2>/dev/null || return
    n=0
    while [ $n -lt $count ]; do
        umount -l "$target" 2>/dev/null || break
        n=$((n + 1))
    done
}

# Seed USB save files from NAND if they're publisher zero-stubs.
pce_seed_save() {
    for f in data_008_0000.bin meta_008_0000.bin; do
        usb_file="$USB_GAME/save/$f"
        nand_file="/rootfs_data/$f"
        [ -f "$usb_file" ] || continue
        [ -f "$nand_file" ] || continue
        sample=$(head -c 4096 "$usb_file" 2>/dev/null | od -An -tx1 -v | tr -d ' \n0')
        if [ -z "$sample" ]; then
            cp "$nand_file" "$usb_file" 2>>${LOG} && sync && pce_log "seeded save/$f from NAND"
        fi
    done
}

# Wholesale game-folder bind.
pce_apply_bind() {
    [ -x "$USB_GAME/m2engage" ] || { pce_log "no $USB_GAME/m2engage"; return 1; }
    pce_seed_save
    pop_sd_binds_on "$LIVE_GAME"
    if mount --bind "$USB_GAME" "$LIVE_GAME" 2>>${LOG}; then
        pce_log "bound $USB_GAME -> $LIVE_GAME"
        return 0
    fi
    pce_log "BIND FAILED"
    return 1
}

# Drop every /dev/sd* layer under /usr/game and on /usr/game itself.
pce_drop_binds() {
    mount | awk '$1 ~ /^\/dev\/sd[a-z]/ && $3 != "/mnt/usb" && $3 != "/usr/game" {print $3}' | \
        while read m; do
            pop_sd_binds_on "$m"
            pce_log "popped $m"
        done
    pop_sd_binds_on "$LIVE_GAME"
    pce_log "popped $LIVE_GAME"
    # On cold-boot-with-USB the wholesale bind was placed BEFORE rcS's
    # `mount -a`, so mount -a skipped /usr/game (already mounted). After
    # popping the bind, /usr/game is just an empty directory — m2engage
    # binary is gone. Remount the ext4 partition manually if missing.
    if ! mount | grep -q " $LIVE_GAME "; then
        mount /dev/by-name/app "$LIVE_GAME" 2>>${LOG} && pce_log "remounted ext4 on $LIVE_GAME"
    fi
}

# Kill engine + shutdown-detection, marking the kill as intentional via
# EXIT_FILE so our gameapp's wrapper skips its shutdown-detection call.
pce_kill_engine() {
    pce_log "killing m2engage + shutdown-detection (intentional)"
    touch /tmp/.game.exit
    killall -9 m2engage shutdown-detection 2>/dev/null
    sleep 1
}

# Schedule gameapp.start detached from udev's worker session. Clears
# EXIT_FILE so the new wrapper runs normally.
pce_schedule_start() {
    pce_log "scheduling gameapp start (detached)"
    setsid sh -c 'rm -f /tmp/.game.exit; /etc/init.d/gameapp start' </dev/null >/dev/null 2>&1 &
}
