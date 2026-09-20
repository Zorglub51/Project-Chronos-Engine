#!/bin/sh
# Shared bind/unbind logic for USB hot-plug + boot handlers.

USB_MNT=/mnt/usb
USB_GAME=${USB_MNT}/game
USB_ROMS=${USB_MNT}/library/published/roms
USB_SAVE=${USB_MNT}/library/published/save
LIVE_GAME=/usr/game
LOG=/tmp/pce-usb.log
PCE_USB_DEBUG=${PCE_USB_DEBUG:-0}
[ ! -f /run/chronos-debug ] || PCE_USB_DEBUG=1

pce_log() {
    # Event logs live in RAM. Bound their size even during long debug sessions.
    if [ -f "$LOG" ] && [ "$(wc -c < "$LOG")" -gt 1048576 ]; then
        mv -f "$LOG" "$LOG.previous"
    fi
    echo "[$(date +%H:%M:%S) pid=$$] $*" >>"$LOG"
}

pce_trace() { [ "$PCE_USB_DEBUG" != 1 ] || pce_log "TRACE $*"; }

pce_trace_snapshot() {
    [ "$PCE_USB_DEBUG" = 1 ] || return 0
    pce_log "TRACE snapshot: $*"
    {
        echo "current pack:"
        cat "$USB_MNT/library/published/folders/.current" 2>/dev/null
        echo "mountinfo (including underlying NAND and stacked binds):"
        awk '$5 == "/mnt/usb" || $5 == "/usr/game" || index($5,"/usr/game/") == 1' /proc/self/mountinfo
        echo "memory:"
        awk '/^(MemTotal|MemFree|Buffers|Cached|Slab):/' /proc/meminfo
        echo "file identities (device:inode size):"
        for path in "$USB_GAME" "$LIVE_GAME" "$USB_ROMS" "$LIVE_GAME/system/roms" "$USB_SAVE" "$LIVE_GAME/save"; do
            stat -c '%d:%i %s %n' "$path"
        done
    } >>"$LOG" 2>&1
    return 0
}

# Pop every /dev/sd* mount layer off `target`. Count first, then umount
# exactly that many times — guards against `umount -l` async finalization
# races that could otherwise pop the underlying ext4 mount.
pop_sd_binds_on() {
    target=$1
    count=$(mount | awk -v t="$target" '$3 == t && $1 ~ /^\/dev\/sd[a-z]/' | wc -l)
    pce_trace "unmount target=$target layers=$count"
    [ "$count" -gt 0 ] 2>/dev/null || return
    n=0
    while [ $n -lt $count ]; do
        umount -l "$target" 2>>"$LOG"
        rc=$?
        pce_trace "umount -l target=$target layer=$((n + 1))/$count rc=$rc"
        [ "$rc" -eq 0 ] || { pce_log "UNMOUNT FAILED target=$target rc=$rc"; return "$rc"; }
        n=$((n + 1))
    done
}

# Seed USB save files from NAND if they're publisher zero-stubs.
pce_seed_save() {
    for f in data_008_0000.bin meta_008_0000.bin; do
        usb_file="$USB_SAVE/$f"
        nand_file="/rootfs_data/$f"
        [ -f "$usb_file" ] || continue
        [ -f "$nand_file" ] || continue
        sample=$(head -c 4096 "$usb_file" 2>/dev/null | od -An -tx1 -v | tr -d ' \n0')
        if [ -z "$sample" ]; then
            cp "$nand_file" "$usb_file" 2>>${LOG} && sync && pce_log "seeded save/$f from NAND"
        fi
    done
}

# Validate before stopping the currently running engine.
pce_check_library() {
    [ -x "$USB_GAME/m2engage" ] || { pce_log "no $USB_GAME/m2engage"; return 1; }
    [ -d "$USB_ROMS" ] || { pce_log "no published ROM directory: $USB_ROMS"; return 1; }
    [ -f "$USB_SAVE/data_008_0000.bin" ] || { pce_log "no published settings/save data"; return 1; }
    [ ! -L "$USB_GAME/system/roms" ] || { pce_log "ROM mount point must not be a symlink"; return 1; }
    [ ! -L "$USB_GAME/save" ] || { pce_log "save mount point must not be a symlink"; return 1; }
    # Refuse to hide pre-existing live saves. Migrate them to published/save
    # explicitly before upgrading an old key; first-test keys use an empty dir.
    if [ -d "$USB_GAME/save" ] && [ -n "$(ls -A "$USB_GAME/save" 2>/dev/null)" ]; then
        pce_log "game/save is not empty; migrate live saves to library/published/save first"
        return 1
    fi
    mkdir -p "$USB_GAME/system/roms" "$USB_GAME/save" || return 1
}

# Children must be unmounted before their parent, including the ROM
# directory and the hook's per-file PSB/save mounts. Only touch our tree.
pce_drop_game_binds() {
    pce_trace_snapshot before-unmount
    mount | awk '$1 ~ /^\/dev\/sd[a-z]/ && ($3 == "/usr/game" || index($3, "/usr/game/") == 1) {print $3}' | \
        sort -ru | while read m; do
            if pop_sd_binds_on "$m"; then
                pce_log "popped $m"
            else
                pce_log "mount remains at $m (see unmount failure)"
            fi
        done
    pce_trace_snapshot after-unmount
}

# Called with m2engage stopped (or before its first boot).
pce_apply_bind() {
    pce_trace_snapshot before-bind
    pce_check_library || return 1
    pce_seed_save
    pce_drop_game_binds
    pce_trace "bind source=$USB_GAME target=$LIVE_GAME"
    if ! mount --bind "$USB_GAME" "$LIVE_GAME" 2>>${LOG}; then
        pce_log "game BIND FAILED"
        return 1
    fi
    pce_trace "bind source=$USB_ROMS target=$LIVE_GAME/system/roms"
    if ! mount --bind "$USB_ROMS" "$LIVE_GAME/system/roms" 2>>${LOG}; then
        pce_log "ROM BIND FAILED; dropping partial USB mounts"
        pce_drop_game_binds
        return 1
    fi
    pce_trace "bind source=$USB_SAVE target=$LIVE_GAME/save"
    if ! mount --bind "$USB_SAVE" "$LIVE_GAME/save" 2>>${LOG}; then
        pce_log "save BIND FAILED; dropping partial USB mounts"
        pce_drop_game_binds
        return 1
    fi
    pce_log "bound game + published ROMs + published saves"
    pce_trace_snapshot after-bind
    return 0
}

# Drop every /dev/sd* layer under /usr/game and on /usr/game itself.
pce_drop_binds() {
    pce_drop_game_binds
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
