const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const app = fs.readFileSync(require('node:path').join(__dirname, '../src/app.js'), 'utf8');
const operations = app.slice(app.indexOf('// ---- Import operations and first-run assistant ----'), app.indexOf('// ---- ROM file import ----'));
const romImport = app.slice(app.indexOf('async function importRomFile('), app.indexOf('async function populateRomDatalist'));
function setup(invoke) {
    const elements = new Map();
    const element = id => {
        if (!elements.has(id)) elements.set(id, {value:'',textContent:'',style:{},classList:{add(){},remove(){},toggle(){}},removeAttribute(key){delete this[key];}});
        return elements.get(id);
    };
    const entry = {folder:'GAME001',game:{rom:{rom:'old.pcd'},display:{csize:0}}};
    const alerts=[],calls=[];
    const context = vm.createContext({
        window:{__TAURI__:{core:{Channel:class {}}}},
        document:{getElementById:element,querySelector:()=>element('app')},
        Date,setInterval,clearInterval,console,
        invoke: async (command,args)=>{calls.push({command,args});return invoke(command,args);},
        editorSettings:{bios:{super_path:'super',system_path:'system'}},
        dualLibrary:{path:'/key/library'}, currentLineup:'jp',currentFolder:{name:'FOLDER_CD'}, selectedIndex:0,
        getLibrary:()=>({games:[entry]}),
        saveNow:async()=>calls.push({command:'saveNow'}),
        populateRomDatalist:async()=>{},syncArchFromPlatform:()=>{},setField:()=>{},updateTags:()=>{},
        modalAlert:async msg=>alerts.push(msg),openSettings:()=>calls.push({command:'openSettings'}),
    });
    vm.runInContext(operations+romImport,context);
    return {context,element,entry,alerts,calls};
}

test('CUE conversion stores the returned PCD filename and promotes HuCard to Super CD',async()=>{
    const t=setup(async command=>command==='get_bios_status'?{ready:true}:{filename:'Ys IV (2).pcd',converted:true,cd:true});
    await vm.runInContext("importRomFile('/roms/Ys IV.CUE','rom.rom',document.getElementById('f-rom'))",t.context);
    assert.equal(t.entry.game.rom.rom,'Ys IV (2).pcd');assert.equal(t.entry.game.display.csize,3);
    assert.equal(t.calls.find(c=>c.command==='import_rom').args.destDir,'/key/library/jp/FOLDER_CD/GAME001');
    assert.deepEqual(t.calls.map(c=>c.command),['get_bios_status','import_rom','saveNow']);
    assert.equal(t.element('app').inert,false);
});
test('PCD bypasses BIOS requirements and keeps Arcade CD selection',async()=>{
    const t=setup(async()=>({filename:'Game.pcd',cd:true,converted:false}));
    t.entry.game.display.csize=4;
    vm.runInContext('editorSettings.bios=null',t.context);
    await vm.runInContext("importRomFile('/roms/Game.pcd','rom.rom',document.getElementById('f-rom'))",t.context);
    assert.equal(t.entry.game.display.csize,4);
    assert.deepEqual(t.calls.map(c=>c.command),['import_rom','saveNow']);
});
test('Import failure unlocks the UI, shows the error and preserves the old ROM',async()=>{
    const t=setup(async command=>{if(command==='get_bios_status')return{ready:true};throw new Error('Track 02.bin is missing');});
    await vm.runInContext("importRomFile('/roms/Game.cue','rom.rom',document.getElementById('f-rom'))",t.context);
    assert.equal(t.entry.game.rom.rom,'old.pcd');assert.equal(t.element('app').inert,false);
    assert.match(t.alerts[0],/Track 02.bin/);assert.ok(!t.calls.some(c=>c.command==='saveNow'));
    assert.equal(vm.runInContext('operationBusy',t.context),false);
});
test('Progress is scoped to the running operation and rejects concurrent work',async()=>{
    let finish,channel;
    const pending=new Promise(resolve=>finish=resolve);
    const t=setup(async(_,args)=>{channel=args.onProgress;return pending;});
    const work=vm.runInContext("runOperation('Copy',onProgress=>invoke('copy',{onProgress}))",t.context);
    channel.onmessage({message:'Copying',completed:4,total:10});
    assert.equal(t.element('operation-progress').value,4);
    await assert.rejects(vm.runInContext("runOperation('Other',()=>{})",t.context),/already running/);
    finish({});await work;
    channel.onmessage({message:'stale',completed:10,total:10});
    assert.equal(t.element('operation-message').textContent,'Copying');
});
test('Missing BIOS opens settings without importing or changing the game',async()=>{
    const t=setup(async()=>({ready:false,message:'No BIOS configured'}));
    await vm.runInContext("importRomFile('/roms/Game.cue','rom.rom',document.getElementById('f-rom'))",t.context);
    assert.deepEqual(t.calls.map(c=>c.command),['get_bios_status','openSettings']);
    assert.equal(t.entry.game.rom.rom,'old.pcd');assert.match(t.alerts[0],/No BIOS/);
});
test('Creation passes the chosen empty/original mode and opens only a completed library',async()=>{
    for (const mode of ['empty','stock']) {
        const t=setup(async()=>({root:'/parent/New library',games:mode==='stock'?58:0,bios:{source:'dump'},warnings:[]}));
        t.context.dualLibrary=null;
        t.element('new-source').value='/backup/nand.bin';t.element('new-parent').value='/parent';
        t.element('new-name').value='New library';t.element('new-contents').value=mode;
        const opened=[];
        t.context.loadLibrary=async root=>opened.push(root);
        t.context.rememberLibrary=async root=>opened.push(root);
        await vm.runInContext('createNewLibrary()',t.context);
        assert.equal(t.calls[0].command,'create_library_from_dump');
        assert.equal(t.calls[0].args.includeGames,mode==='stock');
        assert.deepEqual(opened,['/parent/New library','/parent/New library']);
        assert.equal(t.element('btn-new-create').disabled,false);
    }
});
test('Creation errors remain in the assistant without replacing the open library',async()=>{
    const t=setup(async()=>{throw new Error('Truncated P9');});
    t.element('new-source').value='/backup/p9.bin';t.element('new-parent').value='/parent';t.element('new-name').value='New';
    t.context.loadLibrary=async()=>assert.fail('must not open failed output');
    await vm.runInContext('createNewLibrary()',t.context);
    assert.match(t.element('new-error').textContent,/Truncated P9/);
    assert.equal(t.element('btn-new-create').disabled,false);
    assert.equal(t.context.dualLibrary.path,'/key/library');
});
