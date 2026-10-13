// Console storage management. No console writes occur until the user confirms a restore.
function initPartitionManager({ invoke, dialog, Channel, onClose }) {
    const el = id => document.getElementById(id);
    const panel = el('console-panel');
    let partitions = [];
    let selected = new Set();
    let busy = false;
    let ready = false;
    let sourcePath = null;
    let entries = [];
    let preview = null;

    const target = () => partitions.find(p => String(p.id) === el('partition-restore-target').value);
    const targetName = p => p.id === 0 ? 'Full MMC' : `P${p.id}`;
    const bytes = n => {
        if (n >= 1024 ** 3) return `${(n / 1024 ** 3).toFixed(2)} GiB`;
        if (n >= 1024 ** 2) return `${(n / 1024 ** 2).toFixed(0)} MiB`;
        return `${(n / 1024).toFixed(0)} KiB`;
    };

    function controls() {
        panel.querySelectorAll('button, input, select').forEach(control => { control.disabled = busy; });
        el('btn-partitions-backup').disabled = busy || !ready || selected.size === 0;
        el('btn-partitions-image').disabled = busy || !target();
        el('partition-restore-confirm').disabled = busy || !preview;
        el('btn-partitions-restore').disabled = busy || !ready || !preview || !el('partition-restore-confirm').checked;
    }

    function setBusy(value) { busy = value; controls(); }
    function clearMessages() { el('partition-error').textContent = ''; el('partition-result').textContent = ''; }
    function error(message) { el('partition-error').textContent = String(message); }

    function selectionChanged() {
        const size = partitions.filter(p => selected.has(p.id)).reduce((sum, p) => sum + p.size_bytes, 0);
        el('partition-selection-status').textContent = selected.size ?
            `${selected.has(0) ? 'Full MMC' : `${selected.size} partition${selected.size === 1 ? '' : 's'}`} selected · ${bytes(size)}` : 'No partitions selected';
        panel.querySelectorAll('[data-partition-id]').forEach(input => { input.checked = selected.has(Number(input.dataset.partitionId)); });
        controls();
    }

    function renderPartitions() {
        const body = el('partition-table-body');
        body.replaceChildren();
        const select = el('partition-restore-target');
        select.replaceChildren();
        const placeholder = document.createElement('option');
        placeholder.value = ''; placeholder.textContent = 'Choose a partition…'; select.appendChild(placeholder);
        partitions.forEach(p => {
            const row = document.createElement('tr');
            if (p.id === 0) row.className = 'partition-full-row';
            const cell = document.createElement('td');
            const input = document.createElement('input');
            input.type = 'checkbox'; input.dataset.partitionId = String(p.id);
            input.setAttribute('aria-label', `Back up ${targetName(p)} — ${p.role}`);
            input.addEventListener('change', () => {
                if (busy) return;
                if (input.checked) {
                    if (p.id === 0) selected.clear(); else selected.delete(0);
                    selected.add(p.id);
                } else { selected.delete(p.id); }
                selectionChanged();
            });
            cell.appendChild(input); row.appendChild(cell);
            [targetName(p), p.id === 0 ? 'Complete internal storage' : p.role, bytes(p.size_bytes)].forEach(text => {
                const td = document.createElement('td'); td.textContent = text; row.appendChild(td);
            });
            body.appendChild(row);
            const option = document.createElement('option');
            option.value = String(p.id); option.textContent = `${targetName(p)} — ${p.id === 0 ? 'Complete internal storage' : p.role} (${bytes(p.size_bytes)})`;
            select.appendChild(option);
        });
        selectionChanged();
    }

    async function withProgress(title, work) {
        el('partition-operation').hidden = false;
        el('partition-operation-title').textContent = title;
        el('partition-operation-message').textContent = 'Preparing…';
        el('partition-operation-detail').textContent = '';
        const progress = el('partition-operation-progress');
        progress.removeAttribute('value');
        const channel = new Channel();
        let active = true;
        channel.onmessage = update => {
            if (!active) return;
            const currentPartition = partitions.find(p => p.id === update.id);
            el('partition-operation-message').textContent =
                (currentPartition ? `${targetName(currentPartition)} — ` : '') + (update.message || 'Working…');
            if (update.total > 0 && Number.isFinite(update.current)) {
                progress.max = update.total;
                progress.value = Math.min(update.total, Math.max(0, update.current));
            } else { progress.removeAttribute('value'); }
            const count = update.count > 1 ? `${update.index}/${update.count} · ` : '';
            const transfer = update.total > 0 ? `${bytes(update.current || 0)} / ${bytes(update.total)} · ` : '';
            el('partition-operation-detail').textContent = `${count}${transfer}${Math.floor((update.elapsed_ms || 0) / 1000)} s elapsed`;
        };
        try {
            const result = await work(channel);
            el('partition-operation-message').textContent = 'Complete';
            return result;
        } catch (e) {
            el('partition-operation-message').textContent = 'Operation stopped';
            throw e;
        } finally { active = false; }
    }

    async function releasePreview() {
        const previous = preview;
        preview = null;
        el('partition-restore-confirm').checked = false;
        el('partition-image-sha256').textContent = 'Not verified yet';
        if (previous) await invoke('console_cancel_restore', { token: previous.token });
    }

    async function resetRestore() {
        await releasePreview();
        sourcePath = null; entries = [];
        el('partition-restore-review').hidden = true;
        controls();
    }

    async function refresh() {
        if (busy) return;
        setBusy(true); clearMessages();
        ready = false;
        el('partition-connection-status').textContent = 'Checking connection…';
        try {
            const status = await invoke('console_status');
            ready = status.reachable && status.environment === 'ram_recovery';
            el('partition-connection-status').textContent = ready ? 'RAM recovery connected' :
                status.reachable ? 'Console connected — RAM recovery required' : 'Console not connected';
            el('partition-connection-status').dataset.state = ready ? 'ready' : 'unavailable';
            el('partition-connection-detail').textContent = status.message;
            el('btn-partitions-refresh').textContent = 'Refresh connection';
        } catch (e) {
            el('partition-connection-status').textContent = 'Could not check connection';
            el('partition-connection-status').dataset.state = 'unavailable';
            error(e);
        } finally { setBusy(false); }
    }

    async function backup() {
        if (busy || !ready || selected.size === 0) return;
        setBusy(true); clearMessages();
        try {
            const directory = await dialog.open({ directory: true, multiple: false, title: 'Choose a folder for the console backup' });
            if (!directory) return;
            const report = await withProgress('Back up console storage', onProgress => invoke('console_backup', {
                ids: [...selected].sort((a, b) => a - b), directory, onProgress,
            }));
            el('partition-result').textContent = `Backup complete: ${report.files.length} verified image${report.files.length === 1 ? '' : 's'} saved in ${report.directory}. Manifest: ${report.manifest_path}`;
        } catch (e) { error(`Backup failed: ${e}`); }
        finally { setBusy(false); }
    }

    async function prepare(entry) {
        await releasePreview();
        if (!entry) return;
        const p = target();
        el('partition-image-size').textContent = `${bytes(entry.size)} (${entry.size.toLocaleString()} bytes)`;
        const result = await withProgress('Verify restore image', onProgress => invoke('console_prepare_restore', {
            id: p.id, inPath: sourcePath, selection: entry, onProgress,
        }));
        if (result.id !== p.id || result.size_bytes !== p.size_bytes) {
            await invoke('console_cancel_restore', { token: result.token });
            throw new Error('The verified image does not match the selected partition.');
        }
        preview = result;
        el('partition-image-sha256').textContent = result.sha256;
        el('partition-image-path').textContent = `${sourcePath}${entry.zip_index !== null ? ` → ${result.image_name}` : ''}`;
    }

    async function chooseImage() {
        if (busy || !target()) return;
        setBusy(true); clearMessages();
        try {
            const path = await dialog.open({ multiple: false, title: 'Restore a raw image or ZIP archive', filters: [
                { name: 'Partition images and ZIP archives', extensions: ['bin', 'img', 'mod', 'zip'] },
                { name: 'All files', extensions: ['*'] },
            ] });
            if (!path) return;
            await resetRestore();
            const p = target();
            entries = await invoke('console_inspect_image', { id: p.id, inPath: path });
            if (!entries.length) throw new Error('No image matches this partition’s size.');
            sourcePath = path;
            el('partition-restore-review').hidden = false;
            el('partition-image-path').textContent = path;
            el('partition-image-target').textContent = `${targetName(p)} — ${p.role} (${p.device_path})`;
            el('partition-image-size').textContent = `${bytes(p.size_bytes)} (${p.size_bytes.toLocaleString()} bytes)`;
            el('partition-restore-warning').textContent = p.id === 0 ?
                'Restoring the full MMC replaces all internal storage, including the system, games and saves.' :
                `Restoring ${targetName(p)} replaces its current ${p.role.toLowerCase()} contents.`;
            el('partition-restore-confirm-text').textContent = `I want to replace ${targetName(p)} with this verified image`;
            el('btn-partitions-restore').textContent = `Restore ${targetName(p)}`;
            const member = el('partition-image-member');
            member.replaceChildren();
            el('partition-archive-choice').hidden = entries.length === 1;
            if (entries.length === 1) await prepare(entries[0]);
            else {
                const placeholder = document.createElement('option'); placeholder.value = ''; placeholder.textContent = 'Choose the image to verify…'; member.appendChild(placeholder);
                entries.forEach((entry, index) => {
                    const option = document.createElement('option'); option.value = String(index);
                    option.textContent = `${entry.name} (${bytes(entry.size)})`; member.appendChild(option);
                });
            }
        } catch (e) { error(`Could not prepare image: ${e}`); }
        finally { setBusy(false); }
    }

    async function memberChanged() {
        if (busy) return;
        setBusy(true); clearMessages();
        try {
            const index = el('partition-image-member').value;
            await prepare(index === '' ? null : entries[Number(index)]);
        } catch (e) { error(`Could not verify image: ${e}`); }
        finally { setBusy(false); }
    }

    async function restore() {
        if (busy || !ready || !preview || !el('partition-restore-confirm').checked) return;
        const verified = preview;
        preview = null; // Tokens are single-use, including when the device reports an error.
        setBusy(true); clearMessages();
        try {
            const report = await withProgress(`Restore ${targetName(verified)}`, onProgress => invoke('console_restore', {
                token: verified.token, confirmed: true, onProgress,
            }));
            el('partition-result').textContent = `${targetName(verified)} restored and verified (${bytes(report.bytes)}). SHA-256: ${report.sha256}`;
        } catch (e) { error(`Restore failed: ${e}`); }
        finally {
            try { await resetRestore(); }
            catch (e) { error(`${el('partition-error').textContent}${el('partition-error').textContent ? ' ' : ''}Could not clear restore image: ${e}`); }
            finally { setBusy(false); }
        }
    }

    async function close() {
        if (busy) return false;
        setBusy(true);
        try {
            await resetRestore();
            setBusy(false);
            if (await onClose() === false) return false;
            panel.style.display = 'none';
            return true;
        } catch (e) { error(`Could not close partition manager: ${e}`); return false; }
        finally { setBusy(false); }
    }

    async function open() {
        panel.style.display = '';
        if (busy) return;
        ready = false;
        el('partition-connection-status').textContent = 'Connection not checked';
        el('partition-connection-status').dataset.state = '';
        el('partition-connection-detail').textContent = 'Connect the console’s micro-USB port with a data cable. Start RAM recovery with the recovery utility, then connect its USB network at 169.254.13.37.';
        el('btn-partitions-refresh').textContent = 'Check connection';
        setBusy(true); clearMessages();
        try {
            if (!partitions.length) {
                partitions = await invoke('console_partitions');
                renderPartitions();
            }
        } catch (e) { error(`Could not load partition list: ${e}`); }
        finally { setBusy(false); }
    }

    el('btn-partitions-refresh').addEventListener('click', refresh);
    el('btn-partitions-close').addEventListener('click', close);
    el('btn-partitions-select-all').addEventListener('click', () => {
        if (busy) return;
        selected = new Set(partitions.filter(p => p.id !== 0).map(p => p.id)); selectionChanged();
    });
    el('btn-partitions-backup').addEventListener('click', backup);
    el('btn-partitions-image').addEventListener('click', chooseImage);
    el('partition-image-member').addEventListener('change', memberChanged);
    el('partition-restore-confirm').addEventListener('change', controls);
    el('btn-partitions-restore').addEventListener('click', restore);
    el('partition-restore-target').addEventListener('change', async () => {
        if (busy) return;
        setBusy(true); clearMessages();
        try { await resetRestore(); } catch (e) { error(e); } finally { setBusy(false); }
    });
    el('btn-partitions-image-cancel').addEventListener('click', async () => {
        if (busy) return;
        setBusy(true); clearMessages();
        try { await resetRestore(); } catch (e) { error(e); } finally { setBusy(false); }
    });
    controls();
    return { open, close, isBusy: () => busy };
}
