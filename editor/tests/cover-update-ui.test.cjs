const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const app = fs.readFileSync(path.join(__dirname, '../src/app.js'), 'utf8');
const operations = app.slice(app.indexOf('// ---- Import operations and first-run assistant ----'), app.indexOf('// ---- ROM file import ----'));
const cover = app.slice(app.indexOf('async function importCover('), app.indexOf('// ---- Import operations and first-run assistant ----'));
const folder = app.slice(app.indexOf('async function importFolderCover('), app.indexOf('// From breadcrumb (inside folder)'));
const mosaic = app.slice(app.indexOf('async function buildMosaicCover('), app.indexOf('// ---- Save States ----'));

function setup(importer, save = async () => {}) {
    const elements = new Map();
    const element = id => {
        if (!elements.has(id)) elements.set(id, {
            value: '', textContent: '', src: 'old-preview', style: {},
            classList: { add() {}, remove() {}, toggle() {} },
            removeAttribute(key) { delete this[key]; },
        });
        return elements.get(id);
    };
    const entry = { folder: 'GAME001', has_cover: false, game: { display: { ccolor: 0 } } };
    const calls = [], alerts = [];
    let closed = false;
    const context = vm.createContext({
        window: { __TAURI__: { core: { Channel: class {} } } },
        document: {
            getElementById: element, querySelector: () => element('app'),
            querySelectorAll: () => [{ querySelector: () => element('list-image') }],
        },
        Date, setInterval, clearInterval, clearTimeout, console, saveTimeout: null,
        invoke: async (command, args) => { calls.push({ command, args }); return importer(command, args); },
        saveNow: async () => { calls.push({ command: 'saveNow' }); await save(); },
        dualLibrary: { path: '/key/library' }, currentLineup: 'jp', currentFolder: null,
        selectedIndex: 0, coverCache: {}, getLibrary: () => ({ games: [entry] }),
        getCoverDims: () => ({ width: 240, height: 240 }),
        modalAlert: async message => alerts.push(message),
        mosaicFolderEntry: { folder: 'FOLDER01' }, mosaicSelection: ['A', 'B', 'C', 'D'],
        closeMosaicPicker: () => { closed = true; },
    });
    vm.runInContext(operations + cover + folder + mosaic, context);
    return { context, element, entry, calls, alerts, closed: () => closed };
}

test('Cover import stays locked through asset completion and preview refresh, rejecting another drop', async () => {
    let imported, refreshed;
    const importing = new Promise(resolve => { imported = resolve; });
    const refreshing = new Promise(resolve => { refreshed = resolve; });
    const t = setup(async command => command === 'import_cover' ? importing : refreshing);
    const first = vm.runInContext("importCover('/covers/first.png')", t.context);
    await new Promise(setImmediate);
    assert.equal(t.element('app').inert, true);
    await vm.runInContext("importCover('/covers/second.png')", t.context);
    assert.equal(await vm.runInContext("importFolderCover('FOLDER01', '/covers/second.png')", t.context), null);
    assert.equal(t.calls.filter(c => c.command === 'import_cover').length, 1);
    imported('/key/library/jp/GAME001/cover.png');
    await new Promise(setImmediate);
    assert.equal(t.element('app').inert, true);
    await vm.runInContext('buildMosaicCover()', t.context);
    assert.ok(!t.calls.some(c => c.command === 'generate_folder_mosaic_cover'));
    refreshed('new-preview');
    await first;
    assert.equal(t.element('app').inert, false);
    assert.equal(vm.runInContext('operationBusy', t.context), false);
    assert.equal(t.element('cover-preview').src, 'new-preview');
    assert.deepEqual(t.calls.map(c => c.command), ['saveNow', 'import_cover', 'get_cover']);
});

test('Cover preparation errors remain visible, unlock the editor, and permit a later import', async () => {
    let failing = true;
    const t = setup(async command => {
        if (command === 'import_cover' && failing) throw new Error('Console artwork could not be prepared');
        return 'new-preview';
    });
    await vm.runInContext("importCover('/covers/first.png')", t.context);
    assert.match(t.alerts[0], /Console artwork could not be prepared/);
    assert.equal(t.element('app').inert, false);
    assert.equal(t.entry.has_cover, false);
    failing = false;
    await vm.runInContext("importCover('/covers/second.png')", t.context);
    assert.equal(t.entry.has_cover, true);
    assert.equal(t.element('cover-preview').src, 'new-preview');
});

test('Unsaved metadata must finish before a cover is replaced', async () => {
    const t = setup(async () => assert.fail('cover files must remain untouched'), async () => { throw new Error('Read-only metadata'); });
    await vm.runInContext("importCover('/covers/first.png')", t.context);
    assert.deepEqual(t.calls.map(c => c.command), ['saveNow']);
    assert.match(t.alerts[0], /Read-only metadata/);
    assert.equal(t.element('app').inert, false);
});

test('Folder imports and mosaics use the same operation lifecycle', async () => {
    for (const invocation of ["importFolderCover('FOLDER01', '/covers/folder.png')", 'buildMosaicCover()']) {
        let finish;
        const pending = new Promise(resolve => { finish = resolve; });
        const t = setup(async command => command === 'get_cover' ? 'folder-preview' : pending);
        const work = vm.runInContext(invocation, t.context);
        await new Promise(setImmediate);
        assert.equal(t.element('app').inert, true);
        finish('/key/library/jp/FOLDER01/cover.png');
        await work;
        assert.equal(t.element('app').inert, false);
        assert.equal(vm.runInContext('operationBusy', t.context), false);
        assert.deepEqual(t.calls.map(c => c.command), ['saveNow', invocation.startsWith('build') ? 'generate_folder_mosaic_cover' : 'import_cover', 'get_cover']);
        if (invocation.startsWith('build')) assert.equal(t.closed(), true);
    }
});
