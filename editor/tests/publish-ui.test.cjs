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
    const status = {textContent:'',title:'',setAttribute(key,value){this[key]=value;}};
    const alerts = [];
    const context = vm.createContext({
        dualLibrary: { path: '/key/library', source_schema: 1, lastEdit: 'new' }, currentFolder: null,
        currentLineup: 'jp', saveTimeout: null, saveInFlight: Promise.resolve(),
        structuredClone, setTimeout, clearTimeout, invoke,
        console: { error() {} },
        document: { getElementById: id => id==='save-status'?status:button },
        modalAlert: msg => alerts.push(msg),
    });
    vm.runInContext(source, context);
    return { context, button, alerts, status };
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

test('Source V2 flushes its pending edit once and never invokes the legacy publisher', async () => {
    const calls=[];
    const {context}=setup(async command=>{calls.push(command);});
    context.dualLibrary.source_schema=2;
    vm.runInContext('autoSave()',context);
    await vm.runInContext('publishLibrary()',context);
    assert.equal(context.saveTimeout,null);
    await new Promise(resolve=>setTimeout(resolve,320));
    assert.deepEqual(calls,['save_library']);
});

test('An unrecognized library format cannot invoke legacy publication', async () => {
    for (const schema of [undefined,null,0,3,'1','2']) {
        const calls=[];
        const {context,alerts}=setup(async command=>calls.push(command));
        context.dualLibrary.source_schema=schema;
        await vm.runInContext('publishLibrary()',context);
        assert.deepEqual(calls,[]);
        assert.deepEqual(alerts,['Reopen the library before exporting.']);
    }
});

test('Source V2 save errors are visible without invoking the publisher', async () => {
    const calls=[];
    const {context,alerts}=setup(async command=>{calls.push(command);throw new Error('read-only library');});
    context.dualLibrary.source_schema=2;
    await vm.runInContext('publishLibrary()',context);
    assert.deepEqual(calls,['save_library']);
    assert.match(alerts[0],/Could not save the library:.*read-only library/);
});

test('Publish starts hidden and is shown only after opening an explicit legacy library', async () => {
    const html=fs.readFileSync(path.join(__dirname,'../src/index.html'),'utf8');
    const markup=html.match(/<button\b[^>]*id="btn-publish"[^>]*>/)?.[0];
    assert.match(markup,/style="display:none"/);
    const load=app.slice(app.indexOf('async function loadLibrary('),app.indexOf('function updateStats('));
    for (const schema of [1,2,undefined,null,0,3,'1']) {
        const loaded={path:'/key/library',source_schema:schema,jp:{games:[]},us:{games:[]}};
        const elements=new Map();
        const element=id=>{
            if(!elements.has(id)) elements.set(id,{style:{display:'none'},classList:{remove(){}}});
            return elements.get(id);
        };
        const context=vm.createContext({
            dualLibrary:null,runOperation:async(title,work)=>work({}),
            invoke:async command=>command==='load_library'?loaded:{},
            loadGamesSettings:async()=>{},getLibrary:()=>loaded.jp,
            renderGameList(){},updateStats(){},modalAlert:async message=>assert.fail(message),
            document:{getElementById:element,querySelectorAll:()=>[]},
        });
        vm.runInContext(load,context);
        assert.equal(await vm.runInContext("loadLibrary('/key/library')",context),true);
        assert.equal(element('btn-publish').style.display,schema===1?'':'none');
    }
});

test('Automatic source updates keep font errors visible without interrupting name typing', async () => {
    const {context,status}=setup(async()=>{throw new Error('Library metadata saved, but console fonts could not be updated: invalid original font');});
    await assert.rejects(vm.runInContext('saveNow()',context),/invalid original font/);
    assert.match(status.textContent,/Update failed:.*invalid original font/);
    assert.equal(status.title,status.textContent);
    assert.equal(status['data-state'],'error');
});

test('Genre/settings font update failures are visible even when no game autosave follows', async () => {
    const {context,status,alerts}=setup(async()=>{throw new Error('Settings saved, but console fonts could not be updated');});
    context.editorSettings={games_path:'/key/library'};
    context.gamesSettings={genres:['日本']};
    vm.runInContext(app.slice(app.indexOf('async function saveGamesSettingsToBackend('),app.indexOf('function deleteGenre(')),context);
    await context.saveGamesSettingsToBackend();
    assert.match(status.textContent,/Update failed/);
    assert.match(alerts[0],/Settings could not be fully updated/);
    assert.match(alerts[0],/console fonts/);
});
