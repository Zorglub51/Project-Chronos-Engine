const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

// Test the actual autosave/publication functions without a Tauri webview.
const app = fs.readFileSync(path.join(__dirname, '../src/app.js'), 'utf8');
const source = app.slice(app.indexOf('// ---- Auto-save ----'));
const result = { roms_packed: 1, roms_copied: 0, psb_files_written: 6,
    folders_emitted: 2, save_files_copied: 0, sram_blocks_embedded: 0,
    output_root: '/key/library/published',
    usb: { missing_original_files: ['game/m2engage'] } };

function setup(invoke) {
    const button = { disabled: false, textContent: 'Publish…' };
    const alerts = [];
    const context = vm.createContext({
        dualLibrary: { path: '/key/library', lastEdit: 'new' }, currentFolder: null,
        currentLineup: 'jp', saveTimeout: null, saveInFlight: Promise.resolve(),
        structuredClone, setTimeout, clearTimeout, invoke,
        console: { error() {} },
        document: { getElementById: () => button },
        modalAlert: msg => alerts.push(msg),
    });
    vm.runInContext(source, context);
    return { context, button, alerts };
}

test('Publish flushes a pending edit before export and reports missing console files', async () => {
    const calls = [];
    const { context, button, alerts } = setup(async (command, args) => {
        calls.push({ command, args });
        return result;
    });
    vm.runInContext('autoSave()', context);
    await vm.runInContext('publishLibrary()', context);
    assert.deepEqual(calls.map(c => c.command), ['save_library', 'publish_library']);
    assert.equal(calls[0].args.library.lastEdit, 'new');
    assert.match(alerts[0], /Original console files are still missing/);
    assert.equal(button.disabled, false);
    assert.equal(context.saveTimeout, null);
});

test('Publish waits for earlier saves and prevents a second concurrent export', async () => {
    const calls = [];
    let finishFirst;
    const pending = new Promise(resolve => { finishFirst = resolve; });
    const { context } = setup(async (command, args) => {
        calls.push({ command, args });
        if (calls.length === 1) await pending;
        return result;
    });
    const oldSave = vm.runInContext('saveNow()', context);
    context.dualLibrary.lastEdit = 'latest';
    const publishing = vm.runInContext('publishLibrary()', context);
    await vm.runInContext('publishLibrary()', context);
    assert.ok(calls.every(c => c.command !== 'publish_library'));
    finishFirst();
    await Promise.all([oldSave, publishing]);
    assert.deepEqual(calls.map(c => c.command), ['save_library', 'save_library', 'publish_library']);
    assert.equal(calls[0].args.library.lastEdit, 'new');
    assert.equal(calls[1].args.library.lastEdit, 'latest');
});

test('A failed save aborts publication and restores the button', async () => {
    const calls = [];
    const { context, button, alerts } = setup(async command => {
        calls.push(command);
        throw new Error('disk full');
    });
    await vm.runInContext('publishLibrary()', context);
    assert.deepEqual(calls, ['save_library']);
    assert.match(alerts[0], /Publish failed:.*disk full/);
    assert.equal(button.disabled, false);
});
