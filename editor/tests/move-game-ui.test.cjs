const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const app = fs.readFileSync(require('node:path').join(__dirname, '../src/app.js'), 'utf8');
const source = app.slice(app.indexOf('function recomputeSortIndices('), app.indexOf('// ---- Add game ----'))
    + app.slice(app.indexOf('// ---- Move game between lineups/folders ----'), app.indexOf('// ---- Delete game ----'))
    + app.slice(app.indexOf('// ---- Auto-save ----'), app.indexOf('// ---- Helpers ----'))
    + app.slice(app.indexOf('// ---- Import operations and first-run assistant ----'), app.indexOf('// ---- ROM file import ----'));
const fields = ['sor_date', 'sor_demo', 'sor_genr', 'sor_name', 'sor_pnum'];

function game(folder, name, index = 0) {
    return {
        folder, is_folder: false, sort: Object.fromEntries([['folder', folder], ...fields.map(field => [field, index])]),
        game: {
            game_id: folder + '-stable-id', native_region_tag: folder + '-tag',
            display: { name, name_eng: name, titlebar: 12, csize: 0, players: 1 },
            rom: { country: 'jp', regionTag: folder + '-original' }, genre: 2,
            future: { retained: true },
        },
    };
}

function folder(name) {
    return { folder: name, is_folder: true, sort: Object.fromEntries([['folder', name], ...fields.map(field => [field, 0])]), game_count: 0, game: { display: { name_eng: name } } };
}

function setup({
    titlebar = 12, csize = 0, sourceFolder = false, destination = { lineup: 'us' },
    collision = false, saveError = false, moveError = false, folderError = false,
    warning = null, beforeSave = async () => {}, beforeMove = async () => {},
    emptySourceAfter = false,
} = {}) {
    const entry = game('GAME001', 'The Zeta');
    Object.assign(entry.game.display, { titlebar, csize });
    const sourceGames = emptySourceAfter ? [entry] : [entry, game('GAME002', 'Alpha', 1)];
    const disk = { path: '/key/library', source_schema: 2, jp: { path: '/key/library/jp', games: [] }, us: { path: '/key/library/us', games: [] } };
    const folders = {};
    if (sourceFolder) {
        disk.jp.games.push(folder('FOLDER00'));
        folders['jp/FOLDER00'] = { path: '/key/library/jp/FOLDER00', games: structuredClone(sourceGames) };
    } else disk.jp.games = structuredClone(sourceGames);
    if (destination?.folder) {
        disk[destination.lineup].games.push(folder(destination.folder));
        folders[destination.lineup + '/' + destination.folder] = { path: '/key/library/' + destination.lineup + '/' + destination.folder, games: [] };
    }
    const container = name => name.includes('/') ? folders[name] : disk[name];
    const srcLineup = sourceFolder ? 'jp/FOLDER00' : 'jp';
    if (collision && destination) container(destination.lineup + (destination.folder ? '/' + destination.folder : '')).games.push(game('GAME001', 'Existing game'));
    function updateCounts() {
        for (const lineup of ['jp', 'us']) for (const item of disk[lineup].games) {
            if (item.is_folder) item.game_count = folders[lineup + '/' + item.folder].games.length;
        }
    }
    updateCounts();
    const ui = structuredClone(disk);
    const currentFolder = sourceFolder ? { lineup: 'jp', name: 'FOLDER00', parentGames: ui.jp.games } : null;
    if (sourceFolder) ui.jp.games = structuredClone(folders[srcLineup].games);
    // The pending UI edit must replace this stale metadata before moving.
    container(srcLineup).games[0].game.display.titlebar = 9;
    const calls = [], alerts = [], selections = [], elements = new Map();
    const element = id => {
        if (!elements.has(id)) elements.set(id, {
            style: {}, classList: { add() {}, remove() {}, toggle() {} },
            setAttribute(name, value) { this[name] = value; }, removeAttribute(name) { delete this[name]; },
        });
        return elements.get(id);
    };
    let pickerCalls = 0, saveCalls = 0;
    const context = vm.createContext({
        dualLibrary: ui, currentFolder, currentLineup: 'jp', selectedIndex: 0,
        gamesSettings: { alpha_exclusions: ['The'] }, saveTimeout: null, saveInFlight: Promise.resolve(), coverCache: { old: 'preview' },
        setTimeout, clearTimeout, setInterval, clearInterval, Date, structuredClone, console: { error() {} },
        modalSelect: async () => { pickerCalls++; return destination; },
        modalAlert: async message => alerts.push(message), renderGameList() {}, updateStats() {},
        selectGame(index) { context.selectedIndex = index; selections.push(['game', index]); },
        selectFolder(index) { context.selectedIndex = index; selections.push(['folder', index]); },
        document: { getElementById: element, querySelector: () => element('app') },
        window: { __TAURI__: { core: { Channel: class {} } } },
        invoke: async (command, args) => {
            calls.push({ command, args: structuredClone(args) });
            if (command === 'save_library' || command === 'save_folder_contents') {
                await beforeSave(++saveCalls, args);
                if (saveError) throw new Error('disk full');
                if (command === 'save_library') {
                    disk.jp = structuredClone(args.library.jp);
                    disk.us = structuredClone(args.library.us);
                } else folders[args.lineup + '/' + args.folderName] = structuredClone(args.library);
            } else if (command === 'load_folder_contents') {
                if (folderError) throw new Error('destination unavailable');
                return structuredClone(folders[args.lineup + '/' + args.folderName]);
            } else if (command === 'move_game') {
                await beforeMove(args, context);
                if (moveError) throw new Error('Game move rolled back: read-only gamelist');
                const origin = container(args.srcLineup), target = container(args.destLineup);
                const moved = structuredClone(origin.games.find(item => item.folder === args.folderName));
                assert.ok(moved, 'pending saves must finish before the transaction reads the game');
                let name = args.folderName, suffix = 2;
                while (target.games.some(item => item.folder === name)) name = args.folderName + '-' + suffix++;
                const sourceEntries = new Map(origin.games.map(item => [item.folder, item]));
                const destinationEntries = new Map(target.games.map(item => [item.folder, item]));
                origin.games = args.sourceSort.map(sort => ({ ...structuredClone(sourceEntries.get(sort.folder)), sort: structuredClone(sort) }));
                target.games = args.destinationSort.map((sort, index) => {
                    if (index !== args.destinationSort.length - 1) return { ...structuredClone(destinationEntries.get(sort.folder)), sort: structuredClone(sort) };
                    moved.folder = name;
                    moved.sort = { ...structuredClone(sort), folder: name };
                    moved.game.rom.country = args.destLineup.split('/')[0];
                    return moved;
                });
                updateCounts();
                return { folder_name: name, library: structuredClone(disk), source: structuredClone(origin), warning };
            } else assert.fail('Unexpected post-move or legacy command: ' + command);
        },
    });
    vm.runInContext('function getLibrary() { return dualLibrary[currentLineup]; }\n' + source, context);
    return {
        context, calls, alerts, selections, disk, folders, element,
        pickerCalls: () => pickerCalls, current: () => context.dualLibrary,
        moved: () => destination ? container(destination.lineup + (destination.folder ? '/' + destination.folder : '')).games.at(-1) : undefined,
        async move() { vm.runInContext('autoSave()', context); await vm.runInContext('moveGame()', context); },
    };
}

test('The transaction preserves all titlebars and the final pending edit on JP to US moves', async () => {
    for (let titlebar = 0; titlebar < 13; titlebar++) for (const csize of [0, 3]) {
        const t = setup({ titlebar, csize });
        await t.move();
        assert.deepEqual(t.alerts, []);
        assert.equal(t.moved().game.display.titlebar, titlebar);
        assert.equal(t.moved().game.display.csize, csize);
        assert.equal(t.moved().game.rom.country, 'us');
        assert.equal(t.moved().game.game_id, 'GAME001-stable-id');
        assert.equal(t.moved().game.rom.regionTag, 'GAME001-original');
        assert.deepEqual(t.calls.map(call => call.command), ['save_library', 'move_game']);
        assert.equal(t.context.saveTimeout, null);
    }
});

test('Root/folder and same/cross-lineup moves apply fresh roots and retain the current folder view', async () => {
    for (const sourceFolder of [false, true]) for (const lineup of ['jp', 'us']) for (const intoFolder of [false, true]) {
        if (!sourceFolder && !intoFolder && lineup === 'jp') continue;
        const destination = { lineup, ...(intoFolder ? { folder: 'FOLDER01' } : {}) };
        const t = setup({ sourceFolder, destination });
        await t.move();
        assert.deepEqual(t.alerts, []);
        assert.equal(t.moved().game.display.titlebar, 12);
        assert.equal(t.moved().game.rom.country, lineup);
        assert.equal(t.current().source_schema, 2);
        assert.deepEqual(Object.keys(t.context.coverCache), []);
        assert.deepEqual(t.calls.map(call => call.command), [sourceFolder ? 'save_folder_contents' : 'save_library', ...(intoFolder ? ['load_folder_contents'] : []), 'move_game']);
        if (sourceFolder) {
            assert.equal(t.context.currentFolder.name, 'FOLDER00');
            assert.equal(t.context.currentFolder.parentGames.find(item => item.folder === 'FOLDER00').game_count, 1);
            assert.deepEqual(t.current().jp.games.map(item => item.folder), ['GAME002']);
            if (!intoFolder && lineup === 'jp') assert.ok(t.context.currentFolder.parentGames.some(item => item.folder === 'GAME001'));
        } else assert.ok(!t.current().jp.games.some(item => item.folder === 'GAME001'));
        assert.deepEqual(t.selections.at(-1), ['game', 0]);
    }
});

test('Planned sort rows use clones, normalize gaps and leave the moved row last for conflict renaming', async () => {
    let original;
    const t = setup({ collision: true, beforeMove: async (args, context) => {
        assert.deepEqual(structuredClone(context.dualLibrary), original);
        assert.equal(args.destinationSort.at(-1).folder, 'GAME001');
        assert.deepEqual(args.sourceSort.map(item => item.folder), ['GAME002']);
        for (const field of fields) {
            assert.deepEqual(args.sourceSort.map(item => item[field]), [0]);
            assert.deepEqual(args.destinationSort.map(item => item[field]).sort(), [0, 1]);
        }
    } });
    original = structuredClone(t.current());
    await t.move();
    assert.deepEqual(t.alerts, []);
    assert.equal(t.moved().folder, 'GAME001-2');
    assert.equal(t.moved().sort.folder, 'GAME001-2');
    assert.equal(t.current().us.games[0].folder, 'GAME001');
    assert.equal(t.current().us.games[1].game.game_id, 'GAME001-stable-id');
});

test('A pending save must complete before the move; the editor stays blocked and rejects a second move', async () => {
    let release;
    const pending = new Promise(resolve => { release = resolve; });
    const t = setup({ beforeSave: async count => { if (count === 1) await pending; } });
    const oldSave = vm.runInContext('saveNow()', t.context);
    t.current().jp.games[0].game.display.name = 'Latest edit';
    const moving = t.move();
    await new Promise(setImmediate);
    assert.equal(t.element('app').inert, true);
    assert.equal(vm.runInContext('operationBusy', t.context), true);
    assert.deepEqual(t.calls.map(call => call.command), ['save_library']);
    await vm.runInContext('moveGame()', t.context);
    assert.equal(t.pickerCalls(), 1);
    release();
    await oldSave;
    await moving;
    assert.equal(t.moved().game.display.name, 'Latest edit');
    assert.deepEqual(t.calls.map(call => call.command), ['save_library', 'save_library', 'move_game']);
    assert.equal(t.element('app').inert, false);
    assert.equal(vm.runInContext('operationBusy', t.context), false);
    await new Promise(resolve => setTimeout(resolve, 320));
    assert.equal(t.calls.length, 3, 'the cleared debounce must not save an old-path snapshot after moving');
});

test('Save, destination-read and transaction failures leave the UI model and folder-parent alias untouched', async () => {
    for (const failure of ['saveError', 'folderError', 'moveError']) {
        const t = setup({ sourceFolder: true, destination: { lineup: 'us', folder: 'FOLDER01' }, [failure]: true });
        const before = structuredClone(t.current());
        const parent = t.context.currentFolder.parentGames;
        await t.move();
        assert.deepEqual(structuredClone(t.current()), before);
        assert.equal(t.context.currentFolder.parentGames, parent);
        assert.equal(t.context.selectedIndex, 0);
        assert.deepEqual(t.context.coverCache, { old: 'preview' });
        assert.match(t.alerts[0], /Error moving game:.*(disk full|destination unavailable|rolled back)/);
        assert.equal(t.element('app').inert, false);
        assert.equal(vm.runInContext('operationBusy', t.context), false);
        assert.equal(t.calls.filter(call => call.command === 'save_folder_contents').length, 1);
        if (failure !== 'moveError') assert.ok(!t.calls.some(call => call.command === 'move_game'));
    }
});

test('A post-commit font warning reports a successful move after applying the committed model', async () => {
    const t = setup({ warning: 'console fonts could not be updated: missing font resource' });
    await t.move();
    assert.ok(!t.current().jp.games.some(item => item.folder === 'GAME001'));
    assert.equal(t.current().us.games[0].folder, 'GAME001');
    assert.deepEqual(t.alerts, ['Game moved, but console fonts could not be updated: missing font resource']);
    assert.equal(t.element('app').inert, false);
    assert.deepEqual(t.calls.map(call => call.command), ['save_library', 'move_game']);
});

test('Moving the final game shows the empty source view without saving it again', async () => {
    const t = setup({ sourceFolder: true, emptySourceAfter: true });
    await t.move();
    assert.deepEqual(t.alerts, []);
    assert.equal(t.current().jp.games.length, 0);
    assert.equal(t.context.currentFolder.parentGames[0].game_count, 0);
    assert.equal(t.context.selectedIndex, -1);
    assert.equal(t.element('placeholder').style.display, '');
    assert.deepEqual(t.calls.map(call => call.command), ['save_folder_contents', 'move_game']);
});

test('Cancelling the picker has no filesystem effects', async () => {
    const t = setup({ destination: null });
    await vm.runInContext('moveGame()', t.context);
    assert.deepEqual(t.calls, []);
    assert.deepEqual(t.alerts, []);
    assert.equal(t.current().jp.games.length, 2);
});
