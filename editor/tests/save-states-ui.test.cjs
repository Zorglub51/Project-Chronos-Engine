const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../src/app.js'), 'utf8');
const source = app.slice(app.indexOf('// ---- Save States ----'), app.indexOf('// ---- Sort index normalization ----'));
function element() {
    return { children: [], textContent: '',
        appendChild(child) { this.children.push(child); },
        set innerHTML(_) { this.children = []; },
    };
}
function setup(invoke) {
    const grid = element(), status = element();
    const context = vm.createContext({ invoke,
        currentFolder: { name: 'FOLDER_CD' }, getLibrary: () => ({ path: '/key/library/jp' }),
        document: { createElement: element, getElementById: id => id === 'sram-status' ? status : grid },
    });
    vm.runInContext(source, context);
    return { context, grid, status };
}
function saves(present, thumbnail) {
    return { sram_present: present, states: Array.from({ length: 4 }, (_, slot) => ({
        slot, exists: slot === 1, thumbnail: slot === 1 ? thumbnail : null,
    })) };
}

test('Native slots and SRAM status use the selected nested game path', async () => {
    const { context, grid, status } = setup(async (command, args) => {
        assert.equal(command, 'get_save_states');
        assert.equal(args.gamePath, '/key/library/jp/FOLDER_CD/GAME002');
        return saves(true, 'data:image/png;base64,native');
    });
    await vm.runInContext('loadSaveStates({folder: "GAME002"})', context);
    assert.equal(grid.children.length, 4);
    assert.equal(grid.children[0].children[0].textContent, 'Empty');
    assert.equal(grid.children[1].children[0].src, 'data:image/png;base64,native');
    assert.equal(grid.children[1].children[1].textContent, 'Slot 2');
    assert.match(status.textContent, /SRAM.*file available/);
});

test('A late response cannot replace the newly selected game saves', async () => {
    const resolve = [];
    const { context, grid, status } = setup(() => new Promise(r => resolve.push(r)));
    const old = vm.runInContext('loadSaveStates({folder: "OLD"})', context);
    const current = vm.runInContext('loadSaveStates({folder: "NEW"})', context);
    resolve[1](saves(false, 'new-preview')); await current;
    resolve[0](saves(true, 'old-preview')); await old;
    assert.equal(grid.children[1].children[0].src, 'new-preview');
    assert.match(status.textContent, /SRAM.*no file/);
});

test('Read errors are visible rather than presented as empty slots', async () => {
    const { context, grid, status } = setup(async () => { throw new Error('USB unavailable'); });
    await vm.runInContext('loadSaveStates({folder: "GAME002"})', context);
    assert.match(status.textContent, /Could not read save data.*USB unavailable/);
    assert.equal(grid.children.length, 0);
});
