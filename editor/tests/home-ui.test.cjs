const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const app = fs.readFileSync(path.join(__dirname, '../src/app.js'), 'utf8');
const home = app.slice(app.indexOf('// ---- Home and console workspace ----'), app.indexOf('// ---- Init ----'));
function setup(saveNow) {
    const elements = new Map();
    const element = id => {
        if (!elements.has(id)) elements.set(id, { style: { display: '' }, classList: { remove() {} } });
        return elements.get(id);
    };
    const alerts = [];
    const calls = [];
    const context = vm.createContext({
        operationBusy: false, workspaceView: 'library', selectedIndex: 0,
        dualLibrary: { path: '/library' }, saveTimeout: null,
        partitionManager: { isBusy: () => false, open: () => calls.push('open') },
        saveNow, clearTimeout, restoreParentLibrary: () => calls.push('restoreParent'),
        modalAlert: async message => alerts.push(message),
        document: { getElementById: element },
    });
    vm.runInContext(home, context);
    return { context, element, alerts, calls };
}
test('Opening console partitions saves the library before changing pages', async () => {
    let finish;
    const waiting = new Promise(resolve => { finish = resolve; });
    const { context, calls, element } = setup(() => waiting);
    const opening = vm.runInContext('openConsolePartitions()', context);
    assert.equal(context.workspaceView, 'library');
    assert.equal(calls.length, 0);
    finish();
    await opening;
    assert.deepEqual(calls, ['restoreParent', 'open']);
    assert.equal(context.workspaceView, 'console');
    assert.equal(context.selectedIndex, -1);
    assert.equal(element('sidebar').style.display, 'none');
});
test('A failed library save keeps the current page and prevents console navigation', async () => {
    const { context, calls, alerts } = setup(async () => { throw new Error('disk full'); });
    await vm.runInContext('openConsolePartitions()', context);
    assert.equal(context.workspaceView, 'library');
    assert.deepEqual(calls, []);
    assert.match(alerts[0], /disk full/);
});
test('Home cannot interrupt an active partition transfer', async () => {
    const { context, calls } = setup(async () => calls.push('save'));
    context.partitionManager.isBusy = () => true;
    assert.equal(await vm.runInContext('showHome()', context), false);
    assert.equal(context.workspaceView, 'library');
    assert.deepEqual(calls, []);
});
