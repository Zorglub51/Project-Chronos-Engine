const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const app = fs.readFileSync(path.join(__dirname, '../src/app.js'), 'utf8');
const source = app.slice(app.indexOf('async function loadLibrary('), app.indexOf('function updateStats('));

function setup(sync, schema = 1) {
    const oldLibrary = { path: '/previous/library', jp: { games: [] }, us: { games: [] } };
    const opened = { path: '/new/library', source_schema:schema, jp: { games: [] }, us: { games: [] } };
    const calls = [], alerts = [], elements = new Map();
    const el = id => {
        if (!elements.has(id)) elements.set(id, {style:{},classList:{remove(){}}});
        return elements.get(id);
    };
    const context = vm.createContext({
        dualLibrary: oldLibrary, workspaceView: 'library', coverCache: {}, currentFolder: null,
        currentLineup: 'jp', selectedIndex: 4, Error,
        invoke: async (command, args) => {
            calls.push({command,args});
            if (command === 'sync_library') return sync();
            if (command === 'load_library') return opened;
            assert.fail('Unexpected command: ' + command);
        },
        runOperation: async (title, work) => { calls.push({command:'operation',title}); return work({}); },
        loadGamesSettings: async gamesPath => { calls.push({command:'settings',gamesPath}); },
        renderGameList() {}, updateStats() {}, getLibrary: () => ({games:[]}),
        modalAlert: async message => alerts.push(message),
        document: { getElementById: el, querySelectorAll: () => [] },
    });
    vm.runInContext(source, context);
    return {context,calls,alerts,oldLibrary,opened,el};
}

test('An unsafe save migration is visible and stops opening before replacing the current library', async () => {
    const t = setup(() => { throw new Error('Legacy saves match multiple games'); });
    const opened = await vm.runInContext("loadLibrary('/new')",t.context);
    assert.equal(opened,false);
    assert.equal(t.context.dualLibrary,t.oldLibrary);
    assert.equal(t.context.selectedIndex,4);
    assert.deepEqual(t.calls.map(call=>call.command),['operation','sync_library']);
    assert.match(t.alerts[0],/Save data could not be migrated safely/);
    assert.match(t.alerts[0],/Original save archives are retained/);
    assert.match(t.alerts[0],/match multiple games/);
});

test('Unmatched states and SRAM are reported after opening while the original archives remain available', async () => {
    const t = setup(() => ({state_files_unmapped:2,sram_blocks_unmapped:1,lock_masks_imported:3}));
    assert.equal(await vm.runInContext("loadLibrary('/new')",t.context),true);
    assert.equal(t.context.dualLibrary,t.opened);
    assert.deepEqual(t.calls.map(call=>call.command),['operation','sync_library','load_library','settings']);
    assert.match(t.alerts[0],/Save state files: 2/);
    assert.match(t.alerts[0],/SRAM\) blocks: 1/);
    assert.match(t.alerts[0],/\/new\/library\/published/);
});

test('A complete save migration opens the library without adding a warning', async () => {
    const t = setup(() => ({state_files_unmapped:0,sram_blocks_unmapped:0,state_files_imported:2,lock_masks_imported:1}));
    assert.equal(await vm.runInContext("loadLibrary('/new')",t.context),true);
    assert.deepEqual(t.alerts,[]);
});

test('Source V2 libraries hide Publish while legacy editing trees keep their existing export action', async () => {
    for (const schema of [1,2]) {
        const t = setup(() => ({state_files_unmapped:0,sram_blocks_unmapped:0}),schema);
        await vm.runInContext("loadLibrary('/new')",t.context);
        assert.equal(t.el('btn-publish').style.display,schema===2?'none':'');
    }
});

test('BACKUP/game first-run initialization prepares a source runtime without a catalogue export', async () => {
    const calls=[],alerts=[],channel={};
    const context=vm.createContext({
        dialogOpen:async()=>'/key',modalConfirm:async()=>true,
        invoke:async(command,args)=>{
            calls.push({command,args});
            if(command==='check_library_status') return {has_library:false,has_backup:true,missing_templates:[]};
            if(command==='init_library') return {library_root:'/key/library',source_schema:2};
            assert.fail('Unexpected command: '+command);
        },
        runOperation:async(title,work)=>work(channel),
        loadLibrary:async path=>calls.push({command:'load',path}),
        onSettingChange:(key,path)=>calls.push({command:'remember',key,path}),
        updateGamesPathDisplay(){},modalAlert:async message=>alerts.push(message),console:{log(){}},
    });
    vm.runInContext(app.slice(app.indexOf('async function openFolder('),app.indexOf('async function loadLibrary(')),context);
    await context.openFolder();
    assert.deepEqual(calls.map(call=>call.command),['check_library_status','init_library','load','remember']);
    assert.equal(calls[1].args.onProgress,channel);
    assert.deepEqual(alerts,[]);
});
