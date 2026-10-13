const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.join(__dirname, '../src/partitions.js'), 'utf8');
const size = 104857600;
const parts = [
    { id: 6, device_path: '/dev/mmcblk0p6', role: 'Kernel', size_bytes: 8388608 },
    { id: 7, device_path: '/dev/mmcblk0p7', role: 'Linux', size_bytes: size },
    { id: 0, device_path: '/dev/mmcblk0', role: 'Full NAND', size_bytes: 3909091328 },
];
const image = { zip_index: null, name: 'mmcblk0p7.bin', size, crc32: null };
const verified = { token: 'image-token', id: 7, device_path: '/dev/mmcblk0p7', role: 'Linux', size_bytes: size, image_name: image.name, sha256: 'a'.repeat(64) };

function setup(overrides = {}, dialogs = []) {
    class Element {
        constructor() { this.value = ''; this.textContent = ''; this.style = {}; this.dataset = {}; this.children = []; this.listeners = {}; this.hidden = false; this.checked = false; this.disabled = false; }
        appendChild(child) { this.children.push(child); return child; }
        replaceChildren(...children) { this.children = children; this.value = ''; }
        setAttribute(name, value) { this[name] = value; }
        removeAttribute(name) { delete this[name]; }
        addEventListener(name, handler) { (this.listeners[name] ||= []).push(handler); }
        async fire(name) { for (const listener of this.listeners[name] || []) await listener({ target: this }); }
        querySelectorAll(selector) {
            const values = [...elements.values()].flatMap(value => [value, ...descendants(value)]);
            if (selector === '[data-partition-id]') return values.filter(value => value.dataset.partitionId !== undefined);
            return values;
        }
    }
    const descendants = element => element.children.flatMap(child => [child, ...descendants(child)]);
    const elements = new Map();
    const el = id => { if (!elements.has(id)) elements.set(id, new Element()); return elements.get(id); };
    const calls = [], dialogCalls = [];
    let closed = 0;
    const implementations = {
        console_partitions: () => parts,
        console_status: () => ({ reachable: true, environment: 'ram_recovery', message: 'RAM recovery verified.' }),
        console_inspect_image: () => [image],
        console_prepare_restore: () => verified,
        console_cancel_restore: () => {},
        console_restore: () => ({ id: 7, bytes: size, sha256: verified.sha256, sha1: 'b'.repeat(40) }),
        console_backup: () => ({ directory: '/backup', files: [{ id: 7, path: '/backup/mmcblk0p7.bin', bytes: size, sha1: 'b'.repeat(40) }], manifest_path: '/backup/manifest.json' }),
        ...overrides,
    };
    const context = vm.createContext({
        document: { getElementById: el, createElement: () => new Element() },
        Set, Number, console,
    });
    vm.runInContext(source, context);
    const manager = context.initPartitionManager({
        invoke: async (command, args) => { calls.push({ command, args }); assert.ok(implementations[command], `Unexpected command: ${command}`); return implementations[command](args); },
        dialog: { open: async options => { dialogCalls.push(options); return dialogs.shift() ?? null; } },
        Channel: class {},
        onClose: () => { assert.equal(manager.isBusy(), false); closed++; },
    });
    const checkbox = id => el('console-panel').querySelectorAll('[data-partition-id]').find(input => input.dataset.partitionId === String(id));
    const checkConnection = () => el('btn-partitions-refresh').fire('click');
    const pickTarget = async id => { el('partition-restore-target').value = String(id); await el('partition-restore-target').fire('change'); };
    return { manager, el, calls, dialogCalls, checkbox, checkConnection, pickTarget, closed: () => closed };
}

test('Opening the manager only lists known partitions and does not contact or modify the console', async () => {
    const t = setup(); await t.manager.open();
    assert.deepEqual(t.calls.map(call => call.command), ['console_partitions']);
    assert.equal(t.el('btn-partitions-backup').disabled, true);
    assert.equal(t.el('btn-partitions-restore').disabled, true);
});

test('A normal running console cannot enable backup or restore actions', async () => {
    const t = setup({ console_status: () => ({ reachable: true, environment: 'not_ram_recovery', message: 'Use RAM recovery.' }) });
    await t.manager.open(); await t.checkConnection();
    t.checkbox(7).checked = true; await t.checkbox(7).fire('change');
    await t.el('btn-partitions-backup').fire('click');
    assert.equal(t.el('btn-partitions-backup').disabled, true);
    assert.match(t.el('partition-connection-status').textContent, /RAM recovery required/);
    assert.ok(!t.calls.some(call => call.command === 'console_backup'));
});

test('A full MMC backup is exclusive with numbered partition backups', async () => {
    const t = setup({}, ['/backup']); await t.manager.open(); await t.checkConnection();
    await t.el('btn-partitions-select-all').fire('click');
    assert.equal(t.checkbox(6).checked, true); assert.equal(t.checkbox(7).checked, true); assert.equal(t.checkbox(0).checked, false);
    t.checkbox(0).checked = true; await t.checkbox(0).fire('change');
    assert.equal(t.checkbox(6).checked, false); assert.equal(t.checkbox(7).checked, false);
    await t.el('btn-partitions-backup').fire('click');
    const call = t.calls.find(call => call.command === 'console_backup');
    assert.deepEqual([...call.args.ids], [0]);
    assert.equal(call.args.directory, '/backup'); assert.match(t.el('partition-result').textContent, /verified image/);
});

test('Image inspection and preparation never write; restore requires explicit confirmation and a staged token', async () => {
    const t = setup({}, ['/images/chronos-p7.zip']); await t.manager.open(); await t.checkConnection(); await t.pickTarget(7);
    await t.el('btn-partitions-image').fire('click');
    assert.deepEqual(t.calls.slice(2).map(call => call.command), ['console_inspect_image', 'console_prepare_restore']);
    assert.equal(t.el('partition-image-target').textContent, 'P7 — Linux (/dev/mmcblk0p7)');
    assert.equal(t.el('partition-image-sha256').textContent, verified.sha256);
    assert.equal(t.el('btn-partitions-restore').disabled, true);
    await t.el('btn-partitions-restore').fire('click');
    assert.ok(!t.calls.some(call => call.command === 'console_restore'));
    t.el('partition-restore-confirm').checked = true; await t.el('partition-restore-confirm').fire('change');
    assert.equal(t.el('btn-partitions-restore').disabled, false);
    await t.el('btn-partitions-restore').fire('click');
    const call = t.calls.find(call => call.command === 'console_restore');
    assert.equal(call.args.token, verified.token); assert.equal(call.args.confirmed, true);
    assert.equal(call.args.inPath, undefined); assert.equal(call.args.selection, undefined);
    assert.match(t.el('partition-result').textContent, /P7 restored and verified/);
    assert.equal(t.el('partition-restore-review').hidden, true);
});

test('A ZIP with several matching images waits for member selection before preparing or confirming', async () => {
    const t = setup({ console_inspect_image: () => [
        { ...image, zip_index: 0, name: 'stock/p7.bin' },
        { ...image, zip_index: 1, name: 'chronos/p7.bin' },
    ] }, ['/images/p7.zip']);
    await t.manager.open(); await t.checkConnection(); await t.pickTarget(7); await t.el('btn-partitions-image').fire('click');
    assert.equal(t.el('partition-archive-choice').hidden, false);
    assert.ok(!t.calls.some(call => call.command === 'console_prepare_restore'));
    assert.equal(t.el('partition-restore-confirm').disabled, true);
    t.el('partition-image-member').value = '1'; await t.el('partition-image-member').fire('change');
    const call = t.calls.find(call => call.command === 'console_prepare_restore');
    assert.equal(call.args.selection.name, 'chronos/p7.bin');
    assert.equal(t.el('partition-restore-confirm').disabled, false);
});

test('Changing the destination or cancelling releases the staged image and clears confirmation', async () => {
    const t = setup({}, ['/images/p7.bin']); await t.manager.open(); await t.checkConnection(); await t.pickTarget(7);
    await t.el('btn-partitions-image').fire('click'); t.el('partition-restore-confirm').checked = true;
    await t.pickTarget(6);
    assert.equal(t.calls.at(-1).command, 'console_cancel_restore');
    assert.equal(t.calls.at(-1).args.token, verified.token);
    assert.equal(t.el('partition-restore-confirm').checked, false);
    assert.equal(t.el('partition-restore-review').hidden, true);
    assert.equal(t.el('btn-partitions-restore').disabled, true);
});

test('A failed image check reports the error and cannot enable restore', async () => {
    const t = setup({ console_prepare_restore: () => { throw new Error('ZIP checksum mismatch'); } }, ['/images/broken.zip']);
    await t.manager.open(); await t.checkConnection(); await t.pickTarget(7); await t.el('btn-partitions-image').fire('click');
    assert.match(t.el('partition-error').textContent, /ZIP checksum mismatch/);
    assert.equal(t.el('btn-partitions-restore').disabled, true); assert.equal(t.manager.isBusy(), false);
    assert.ok(!t.calls.some(call => call.command === 'console_restore'));
});

test('A preview for a different partition is discarded and cannot be confirmed', async () => {
    const t = setup({ console_prepare_restore: () => ({ ...verified, id: 6 }) }, ['/images/p7.bin']);
    await t.manager.open(); await t.checkConnection(); await t.pickTarget(7); await t.el('btn-partitions-image').fire('click');
    assert.equal(t.calls.at(-1).command, 'console_cancel_restore');
    assert.match(t.el('partition-error').textContent, /does not match/);
    assert.equal(t.el('btn-partitions-restore').disabled, true);
});

test('The page remains locked during transfer and rejects concurrent actions; stale progress is ignored', async () => {
    let finish, channel;
    const pending = new Promise(resolve => { finish = resolve; });
    const t = setup({ console_backup: args => { channel = args.onProgress; return pending; } }, ['/backup']);
    await t.manager.open(); await t.checkConnection(); t.checkbox(7).checked = true; await t.checkbox(7).fire('change');
    const operation = t.el('btn-partitions-backup').fire('click'); await new Promise(setImmediate);
    assert.equal(t.manager.isBusy(), true); assert.equal(t.el('btn-partitions-close').disabled, true);
    channel.onmessage({ message: 'Verifying backup', current: size / 2, total: size, elapsed_ms: 3200 });
    assert.equal(t.el('partition-operation-progress').value, size / 2);
    assert.equal(await t.manager.close(), false); await t.checkConnection();
    assert.equal(t.calls.filter(call => call.command === 'console_status').length, 1);
    finish({ directory: '/backup', files: [{}], manifest_path: '/backup/manifest.json' }); await operation;
    channel.onmessage({ message: 'Old transfer message', current: 0, total: size });
    assert.equal(t.el('partition-operation-message').textContent, 'Complete');
    assert.equal(t.manager.isBusy(), false);
    await t.manager.close(); assert.equal(t.closed(), 1);
});

test('A failed restore uses its token once, preserves the error and unlocks the page', async () => {
    const t = setup({ console_restore: () => { throw new Error('Console disconnected'); } }, ['/images/p7.bin']);
    await t.manager.open(); await t.checkConnection(); await t.pickTarget(7); await t.el('btn-partitions-image').fire('click');
    t.el('partition-restore-confirm').checked = true; await t.el('btn-partitions-restore').fire('click');
    assert.match(t.el('partition-error').textContent, /Console disconnected/);
    assert.equal(t.manager.isBusy(), false); assert.equal(t.el('btn-partitions-restore').disabled, true);
    await t.el('btn-partitions-restore').fire('click');
    assert.equal(t.calls.filter(call => call.command === 'console_restore').length, 1);
});

test('A cancelled file picker keeps the previously verified image and cleanup failures unlock the page', async () => {
    const t = setup({ console_cancel_restore: () => { throw new Error('Temporary file could not be removed'); } }, ['/images/p7.bin', null]);
    await t.manager.open(); await t.checkConnection(); await t.pickTarget(7); await t.el('btn-partitions-image').fire('click');
    t.el('partition-restore-confirm').checked = true;
    await t.el('btn-partitions-image').fire('click');
    assert.equal(t.el('partition-image-sha256').textContent, verified.sha256);
    assert.equal(t.el('btn-partitions-restore').disabled, false);
    assert.equal(await t.manager.close(), false);
    assert.equal(t.manager.isBusy(), false);
    assert.equal(t.closed(), 0);
    assert.match(t.el('partition-error').textContent, /Temporary file could not be removed/);
    assert.equal(t.el('btn-partitions-restore').disabled, true);
});
