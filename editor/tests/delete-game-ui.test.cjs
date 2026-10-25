const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const app = fs.readFileSync(path.join(__dirname, '../src/app.js'), 'utf8');
const source = app.slice(app.indexOf('function recomputeSortIndices('), app.indexOf('// ---- Reorder ----'))
    + app.slice(app.indexOf('// ---- Delete game ----'), app.indexOf('// ---- Helpers ----'))
    + app.slice(app.indexOf('// ---- Import operations and first-run assistant ----'), app.indexOf('// ---- ROM file import ----'));
const sortFields = ['sor_date', 'sor_demo', 'sor_genr', 'sor_name', 'sor_pnum'];

function game(folder, index) {
    return {
        folder, is_folder: false,
        sort: Object.fromEntries([['folder', folder], ...sortFields.map(field => [field, index])]),
        game: { game_id: 'stable-' + folder, display: { name: folder, name_eng: folder }, genre: 1 },
    };
}

function setup({ insideFolder = false, selectedFolder = false, onlyGame = false, lineup = 'jp', beforeSave = async () => {}, beforeDelete = async () => {}, confirm = true, warning = null } = {}) {
    const entries = (onlyGame ? ['GAME01'] : ['GAME00', 'GAME01', 'GAME02']).map(game);
    if (selectedFolder) {
        entries[1].folder = 'FOLDER01';
        entries[1].sort.folder = 'FOLDER01';
        entries[1].is_folder = true;
        entries[1].game_count = 2;
    }
    const parent = { folder: 'FOLDER00', is_folder: true, game_count: entries.length, game: { display: { name_eng: 'CD games' } } };
    const disk = { path: '/key/library', source_schema: 2, jp: { path: '/key/library/jp', games: [] }, us: { path: '/key/library/us', games: [] } };
    disk[lineup].games = insideFolder ? [parent] : structuredClone(entries);
    const child = { path: '/key/library/' + lineup + '/FOLDER00', games: structuredClone(entries) };
    const ui = structuredClone(disk);
    const currentFolder = insideFolder ? { lineup, name: 'FOLDER00', parentGames: ui[lineup].games } : null;
    if (insideFolder) ui[lineup].games = structuredClone(child.games);
    const container = () => insideFolder ? child : disk[lineup];
    // Model the filesystem separately: a stale metadata save recreates a game
    // directory after delete, but cannot recreate its removed ROM payload.
    const files = new Map(entries.map(entry => [entry.folder, { metadata: true, rom: true }]));
    const calls = [], alerts = [], selections = [], elements = new Map();
    const element = id => {
        if (!elements.has(id)) elements.set(id, {
            style: {}, classList: { add() {}, remove() {}, toggle() {} },
            setAttribute(name, value) { this[name] = value; }, removeAttribute(name) { delete this[name]; },
        });
        return elements.get(id);
    };
    let saveCount = 0, confirmations = 0;
    const context = vm.createContext({
        dualLibrary: ui, currentFolder, currentLineup: lineup, selectedIndex: onlyGame ? 0 : 1,
        editorSettings: { confirm_delete: true }, gamesSettings: { alpha_exclusions: [] },
        saveTimeout: null, saveInFlight: Promise.resolve(), coverCache: {},
        setTimeout, clearTimeout, setInterval, clearInterval, Date, structuredClone, console: { error() {} },
        modalConfirm: async () => { confirmations++; return confirm; },
        modalAlert: async message => alerts.push(message), renderGameList() {}, updateStats() {},
        selectGame(index) { context.selectedIndex = index; selections.push(['game', index]); },
        selectFolder(index) { context.selectedIndex = index; selections.push(['folder', index]); },
        document: { getElementById: element, querySelector: () => element('app') },
        window: { __TAURI__: { core: { Channel: class {} } } },
        invoke: async (command, args) => {
            calls.push({ command, args: structuredClone(args) });
            if (command === 'save_library' || command === 'save_folder_contents') {
                await beforeSave(++saveCount, args);
                const saved = command === 'save_library' ? args.library[lineup] : args.library;
                container().games = structuredClone(saved.games);
                for (const entry of saved.games) {
                    if (entry.is_folder) continue;
                    files.set(entry.folder, { metadata: true, rom: files.get(entry.folder)?.rom || false });
                }
            } else if (command === 'delete_game' || command === 'delete_folder') {
                await beforeDelete(args);
                files.delete(args.folderName);
                const remaining = new Map(container().games.filter(entry => entry.folder !== args.folderName).map(entry => [entry.folder, entry]));
                assert.ok(Array.isArray(args.remainingSort), 'the transaction receives the complete planned remaining gamelist');
                container().games = args.remainingSort.map(sort => {
                    assert.ok(remaining.has(sort.folder), 'no unrelated directory may appear in the deletion plan');
                    return { ...structuredClone(remaining.get(sort.folder)), sort: structuredClone(sort) };
                });
                if (insideFolder) disk[lineup].games[0].game_count = container().games.length;
                return { library: structuredClone(disk), source: structuredClone(container()), warning };
            } else assert.fail('Unexpected command: ' + command);
        },
    });
    vm.runInContext('function getLibrary() { return dualLibrary[currentLineup]; }\n' + source, context);
    return {
        context, calls, alerts, selections, files, disk, child, element, lineup,
        current: () => context.dualLibrary, container, confirmations: () => confirmations,
        delete: () => vm.runInContext('deleteGame()', context),
    };
}

test('Deleting waits for an older save snapshot so it cannot recreate a deleted game or gamelist reference', async () => {
    for (const insideFolder of [false, true]) for (const lineup of ['jp', 'us']) {
        let release;
        const gate = new Promise(resolve => { release = resolve; });
        const t = setup({ insideFolder, lineup, beforeSave: async count => { if (count === 1) await gate; } });
        const oldSave = vm.runInContext('saveNow()', t.context);
        await new Promise(setImmediate);
        const deleting = t.delete();
        await new Promise(setImmediate);
        release();
        await oldSave;
        await deleting;
        assert.deepEqual(t.alerts, []);
        assert.equal(t.files.has('GAME01'), false, 'the old save must not recreate an orphan game.json without its ROM');
        assert.deepEqual(t.container().games.map(entry => entry.folder), ['GAME00', 'GAME02']);
        assert.deepEqual(t.current()[lineup].games.map(entry => entry.folder), ['GAME00', 'GAME02']);
        for (const field of sortFields) assert.deepEqual(t.container().games.map(entry => entry.sort[field]).sort(), [0, 1]);
        if (insideFolder) assert.equal(t.context.currentFolder.parentGames[0].game_count, 2);
    }
});

test('A pending debounce is flushed and cleared before deletion and cannot save a former game path later', async () => {
    for (const insideFolder of [false, true]) {
        const t = setup({ insideFolder });
        t.current().jp.games[0].game.display.name_eng = 'Latest unsaved title';
        vm.runInContext('autoSave()', t.context);
        await t.delete();
        assert.equal(t.context.saveTimeout, null);
        assert.equal(t.container().games[0].game.display.name_eng, 'Latest unsaved title');
        const count = t.calls.length;
        await new Promise(resolve => setTimeout(resolve, 320));
        assert.equal(t.calls.length, count, 'no debounce may race with the committed deletion');
        assert.equal(t.files.has('GAME01'), false);
    }
});

test('An unsaved metadata failure prevents destructive deletion and leaves the current view unchanged', async () => {
    const t = setup({ insideFolder: true, beforeSave: async () => { throw new Error('USB drive is read-only'); } });
    const before = structuredClone(t.current());
    const parent = t.context.currentFolder.parentGames;
    await t.delete();
    assert.ok(!t.calls.some(call => call.command === 'delete_game'));
    assert.deepEqual(structuredClone(t.current()), before);
    assert.equal(t.context.currentFolder.parentGames, parent);
    assert.equal(t.context.selectedIndex, 1);
    assert.equal(t.files.get('GAME01').rom, true);
    assert.match(t.alerts[0], /Error deleting game:.*read-only/);
});

test('Deletion locks editing through completion and rejects another delete of the same selected entry', async () => {
    let release;
    const pending = new Promise(resolve => { release = resolve; });
    const t = setup({ beforeDelete: async () => pending });
    const deleting = t.delete();
    await new Promise(setImmediate);
    const locked = t.element('app').inert;
    const busy = vm.runInContext('operationBusy', t.context);
    const second = t.delete();
    await new Promise(setImmediate);
    release();
    await Promise.all([deleting, second]);
    assert.equal(locked, true);
    assert.equal(busy, true);
    assert.equal(t.calls.filter(call => call.command === 'delete_game').length, 1);
    assert.equal(t.element('app').inert, false);
    assert.equal(vm.runInContext('operationBusy', t.context), false);
    assert.equal(t.confirmations(), 1);
});

test('Cancelling deletion leaves files, metadata and selection untouched', async () => {
    const t = setup({ confirm: false });
    const before = structuredClone(t.current());
    await t.delete();
    assert.deepEqual(t.calls, []);
    assert.deepEqual(structuredClone(t.current()), before);
    assert.equal(t.context.selectedIndex, 1);
    assert.equal(t.files.get('GAME01').rom, true);
});

test('A failed backend transaction leaves the view, parent alias, selection and cache unchanged', async () => {
    for (const insideFolder of [false, true]) {
        const t = setup({ insideFolder, beforeDelete: async () => { throw new Error('Deletion rolled back: read-only gamelist'); } });
        t.context.coverCache = { old: 'thumbnail' };
        const before = structuredClone(t.current());
        const parent = t.context.currentFolder?.parentGames;
        await t.delete();
        assert.deepEqual(structuredClone(t.current()), before);
        assert.equal(t.context.currentFolder?.parentGames, parent);
        assert.deepEqual(t.context.coverCache, { old: 'thumbnail' });
        assert.equal(t.context.selectedIndex, 1);
        assert.equal(t.files.get('GAME01').rom, true);
        assert.match(t.alerts[0], /Error deleting game:.*rolled back/);
        assert.equal(t.element('app').inert, false);
        assert.equal(vm.runInContext('operationBusy', t.context), false);
    }
});

test('Deleting a root folder uses the same transaction and applies committed roots without a second save', async () => {
    const t = setup({ selectedFolder: true });
    await t.delete();
    assert.deepEqual(t.alerts, []);
    assert.deepEqual(t.calls.map(call => call.command), ['save_library', 'delete_folder']);
    assert.equal(t.files.has('FOLDER01'), false);
    assert.deepEqual(t.current().jp.games.map(entry => entry.folder), ['GAME00', 'GAME02']);
    assert.deepEqual(t.selections, [['game', 1]]);
    assert.deepEqual(Object.keys(t.context.coverCache), []);
});

test('A post-commit font warning reports success and keeps the deleted game out of the current folder and parent count', async () => {
    const t = setup({ insideFolder: true, warning: 'console fonts could not be updated: missing font resource' });
    await t.delete();
    assert.equal(t.files.has('GAME01'), false);
    assert.deepEqual(t.current().jp.games.map(entry => entry.folder), ['GAME00', 'GAME02']);
    assert.equal(t.context.currentFolder.parentGames[0].game_count, 2);
    assert.equal(t.alerts.length, 1);
    assert.match(t.alerts[0], /deleted, but console fonts could not be updated/);
    assert.deepEqual(t.calls.map(call => call.command), ['save_folder_contents', 'delete_game']);
    assert.equal(t.element('app').inert, false);
});

test('Deleting the final game leaves the empty US folder view and its parent count consistent', async () => {
    const t = setup({ insideFolder: true, onlyGame: true, lineup: 'us' });
    await t.delete();
    assert.deepEqual(t.alerts, []);
    assert.equal(t.files.has('GAME01'), false);
    assert.equal(t.current().us.games.length, 0);
    assert.equal(t.context.currentFolder.name, 'FOLDER00');
    assert.equal(t.context.currentFolder.parentGames[0].game_count, 0);
    assert.equal(t.context.selectedIndex, -1);
    assert.equal(t.element('placeholder').style.display, '');
    assert.deepEqual(t.calls.map(call => call.command), ['save_folder_contents', 'delete_game']);
});
