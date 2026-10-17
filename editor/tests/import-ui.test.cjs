const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const app = fs.readFileSync(require('node:path').join(__dirname, '../src/app.js'), 'utf8');
const operations = app.slice(app.indexOf('// ---- Import operations and first-run assistant ----'), app.indexOf('// ---- ROM file import ----'));
const romImport = app.slice(app.indexOf('async function importRomFile('), app.indexOf('async function populateRomDatalist'));
function setup(invoke, save = async () => {}) {
    const elements = new Map();
    const element = id => {
        if (!elements.has(id)) elements.set(id, {value:'',textContent:'',style:{},listeners:{},classList:{add(){},remove(){},toggle(){}},removeAttribute(key){delete this[key];},addEventListener(type,handler){this.listeners[type]=handler;}});
        return elements.get(id);
    };
    const entry = {folder:'GAME001',game:{rom:{rom:'old.pcd',arch:'tg16'},display:{csize:0}}};
    const alerts=[],calls=[];
    const context = vm.createContext({
        window:{__TAURI__:{core:{Channel:class {}}}},
        document:{getElementById:element,querySelector:()=>element('app')},
        Date,setInterval,clearInterval,setTimeout,clearTimeout,console,saveTimeout:null,
        invoke: async (command,args)=>{calls.push({command,args});return invoke(command,args);},
        editorSettings:{bios:{super_path:'super',system_path:'system'}},
        dualLibrary:{path:'/key/library'}, currentLineup:'jp',currentFolder:{name:'FOLDER_CD'}, selectedIndex:0,
        getLibrary:()=>({games:[entry]}),
        saveNow:async()=>{calls.push({command:'saveNow',rom:entry.game.rom.rom});await save();},
        populateRomDatalist:async()=>{},syncArchFromPlatform:()=>{entry.game.rom.arch='tg16cd';},setField:()=>{},updateTags:()=>{},
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
    assert.deepEqual(t.calls.map(c=>c.command),['get_bios_status','saveNow','import_rom','saveNow','remove_replaced_rom']);
    assert.equal(t.element('app').inert,false);
});
test('PCD bypasses BIOS requirements and keeps Arcade CD selection',async()=>{
    const t=setup(async()=>({filename:'Game.pcd',cd:true,converted:false}));
    t.entry.game.display.csize=4;
    vm.runInContext('editorSettings.bios=null',t.context);
    await vm.runInContext("importRomFile('/roms/Game.pcd','rom.rom',document.getElementById('f-rom'))",t.context);
    assert.equal(t.entry.game.display.csize,4);
    assert.deepEqual(t.calls.map(c=>c.command),['saveNow','import_rom','saveNow','remove_replaced_rom']);
});
test('Packed HuCard import keeps its original filename without requesting BIOS',async()=>{
    const t=setup(async()=>({filename:'Neutopia_II_J.PCE.m',cd:false,converted:false}));
    vm.runInContext('editorSettings.bios=null',t.context);
    await vm.runInContext("importRomFile('/roms/Neutopia_II_J.PCE.m','rom.rom',document.getElementById('f-rom'))",t.context);
    assert.equal(t.entry.game.rom.rom,'Neutopia_II_J.PCE.m');
    assert.equal(t.entry.game.display.csize,0);
    assert.equal(t.element('f-rom').value,'Neutopia_II_J.PCE.m');
    assert.deepEqual(t.calls.map(c=>c.command),['saveNow','import_rom','saveNow','remove_replaced_rom']);
});
test('Import failure unlocks the UI, shows the error and preserves the old ROM',async()=>{
    const t=setup(async command=>{if(command==='get_bios_status')return{ready:true};throw new Error('Track 02.bin is missing');});
    await vm.runInContext("importRomFile('/roms/Game.cue','rom.rom',document.getElementById('f-rom'))",t.context);
    assert.equal(t.entry.game.rom.rom,'old.pcd');assert.equal(t.element('app').inert,false);
    assert.match(t.alerts[0],/Track 02.bin/);assert.deepEqual(t.calls.filter(c=>c.command==='saveNow').map(c=>c.rom),['old.pcd']);
    assert.ok(!t.calls.some(c=>c.command==='remove_replaced_rom'));
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
        const t=setup(async()=>({root:'/Volumes/CHRONOS',source_schema:2,games:mode==='stock'?58:0,bios:{source:'dump'},warnings:[]}));
        t.context.dualLibrary=null;
        t.element('new-source').value='/backup/nand.bin';t.element('new-destination').value='/Volumes/CHRONOS';
        t.element('new-contents').value=mode;
        const opened=[];
        t.context.loadLibrary=async root=>opened.push(root);
        t.context.rememberLibrary=async root=>opened.push(root);
        await vm.runInContext('createNewLibrary()',t.context);
        assert.equal(t.calls[0].command,'create_library_from_dump');
        assert.equal(t.calls[0].args.destination,'/Volumes/CHRONOS');
        assert.equal(t.calls[0].args.includeGames,mode==='stock');
        assert.deepEqual(opened,['/Volumes/CHRONOS','/Volumes/CHRONOS']);
        assert.equal(t.element('btn-new-create').disabled,false);
    }
});
test('Creation errors remain in the assistant without replacing the open library',async()=>{
    const t=setup(async()=>{throw new Error('Truncated P9');});
    t.element('new-source').value='/backup/p9.bin';t.element('new-destination').value='/Volumes/CHRONOS';
    t.context.loadLibrary=async()=>assert.fail('must not open failed output');
    await vm.runInContext('createNewLibrary()',t.context);
    assert.match(t.element('new-error').textContent,/Truncated P9/);
    assert.equal(t.element('btn-new-create').disabled,false);
    assert.equal(t.context.dualLibrary.path,'/key/library');
});

test('Creation uses the selected destination exactly on macOS, Linux and Windows without a Chronos child',async()=>{
    for(const destination of ['/Volumes/CHRONOS','/Volumes/CHRONOS/','/media/user/CHRONOS/','/','C:\\','E:\\','\\\\server\\share\\','/home/user/Library preparation']) {
        const t=setup(async()=>({root:destination,source_schema:2,games:0,bios:null,warnings:[]}));
        t.context.dualLibrary=null;
        t.element('new-source').value='/backup/Extracted P9';
        t.element('new-destination').value=destination;
        t.element('new-contents').value='empty';
        const opened=[];
        t.context.loadLibrary=async root=>opened.push(root);
        t.context.rememberLibrary=async root=>opened.push(root);
        await vm.runInContext('createNewLibrary()',t.context);
        assert.equal(t.calls.length,1);
        assert.equal(t.calls[0].command,'create_library_from_dump');
        assert.equal(t.calls[0].args.destination,destination);
        assert.equal(t.calls[0].args.sourcePath,'/backup/Extracted P9');
        assert.equal(t.calls[0].args.includeGames,false);
        assert.deepEqual(opened,[destination,destination]);
        assert.match(t.alerts[0],/Empty library ready for the console/);
        assert.equal(t.element('new-error').textContent,'');
    }
    const html=fs.readFileSync(require('node:path').join(__dirname,'../src/index.html'),'utf8');
    assert.doesNotMatch(html,/id="new-name"|id="new-parent"/);
    assert.match(html,/Destination USB drive or folder/);
    assert.match(html,/Creates game\/ and library\/ directly in the selected destination/);
    assert.match(html,/Existing libraries are never replaced/);
});

test('The destination picker selects the drive itself and preserves each original-backup picker mode',async()=>{
    const t=setup(async()=>assert.fail('Choosing paths must not create a library'));
    const dialogs=[];
    t.context.dialogOpen=async options=>{dialogs.push(structuredClone(options));return 'E:\\';};
    vm.runInContext('setupImportUI()',t.context);
    await t.element('btn-new-destination').listeners.click();
    assert.equal(t.element('new-destination').value,'E:\\');
    assert.deepEqual(dialogs[0],{directory:true,title:'Select destination USB drive or folder'});
    for(const type of ['image','partitions','folder']) {
        t.element('new-source-type').value=type;
        await t.element('btn-new-source').listeners.click();
        assert.equal(dialogs.at(-1).directory,type!=='image');
        assert.equal(dialogs.at(-1).title,'Select original console backup');
    }
    assert.deepEqual(t.calls,[]);
});

test('Creation requires both chosen paths without requiring or inventing a folder name',async()=>{
    for(const [source,destination] of [['','/Volumes/CHRONOS'],['/backup/nand.bin','']]) {
        const t=setup(async()=>assert.fail('Incomplete choices must not start creation'));
        t.element('new-source').value=source;t.element('new-destination').value=destination;
        await vm.runInContext('createNewLibrary()',t.context);
        assert.deepEqual(t.calls,[]);
        assert.match(t.element('new-error').textContent,/Choose a backup and a destination USB drive or folder/);
    }
});

test('Creation clears the old debounce and flushes existing edits before preparing the chosen drive',async()=>{
    const t=setup(async command=>{
        assert.equal(command,'create_library_from_dump');
        return {root:'/Volumes/CHRONOS',source_schema:2,games:0,bios:null,warnings:[]};
    });
    t.element('new-source').value='/backup/nand.bin';t.element('new-destination').value='/Volumes/CHRONOS';
    t.context.loadLibrary=async()=>{};t.context.rememberLibrary=async()=>{};
    vm.runInContext("saveTimeout=setTimeout(()=>invoke('stale-save'),300)",t.context);
    await vm.runInContext('createNewLibrary()',t.context);
    assert.equal(t.context.saveTimeout,null);
    await new Promise(resolve=>setTimeout(resolve,320));
    assert.deepEqual(t.calls.map(call=>call.command),['saveNow','create_library_from_dump']);
});

test('An existing library save failure aborts creation and leaves its destination untouched',async()=>{
    const t=setup(async()=>assert.fail('Failed saving must prevent destination writes'),async()=>{throw new Error('read-only current library');});
    t.element('new-source').value='/backup/nand.bin';t.element('new-destination').value='/Volumes/CHRONOS';
    await vm.runInContext('createNewLibrary()',t.context);
    assert.deepEqual(t.calls.map(call=>call.command),['saveNow']);
    assert.match(t.element('new-error').textContent,/read-only current library/);
    assert.equal(t.context.dualLibrary.path,'/key/library');
    assert.equal(t.element('btn-new-create').disabled,false);
});

test('Cleanup runs after the new choice is saved and stays inside the selected game folder',async()=>{
    const t=setup(async()=>({filename:'new.pce',cd:false}));
    await vm.runInContext("importRomFile('/source/new.pce','rom.rom',document.getElementById('f-rom'))",t.context);
    assert.deepEqual(t.calls.filter(c=>c.command==='saveNow').map(c=>c.rom),['old.pcd','new.pce']);
    const cleanup=t.calls.find(c=>c.command==='remove_replaced_rom');
    assert.equal(cleanup.args.destDir,'/key/library/jp/FOLDER_CD/GAME001');
    assert.equal(cleanup.args.previous,'old.pcd');
    assert.equal(cleanup.args.replacement,'new.pce');
    assert.equal(t.calls.at(-1),cleanup);
});

test('Reusing the same ROM or importing into an empty entry does not delete a file',async()=>{
    for(const previous of ['same.pce.m','']) {
        const t=setup(async()=>({filename:'same.pce.m',cd:false}));
        t.entry.game.rom.rom=previous;
        await vm.runInContext("importRomFile('/source/same.pce.m','rom.rom',document.getElementById('f-rom'))",t.context);
        assert.equal(t.entry.game.rom.rom,'same.pce.m');
        assert.ok(!t.calls.some(c=>c.command==='remove_replaced_rom'));
    }
});

test('A failed pre-import save aborts before copying or deleting any ROM',async()=>{
    const t=setup(async()=>assert.fail('must not invoke file operations'),async()=>{throw new Error('read-only disk');});
    await vm.runInContext("importRomFile('/source/new.pce','rom.rom',document.getElementById('f-rom'))",t.context);
    assert.deepEqual(t.calls.map(c=>c.command),['saveNow']);
    assert.equal(t.entry.game.rom.rom,'old.pcd');
    assert.match(t.alerts[0],/read-only disk/);
    assert.equal(t.element('app').inert,false);
});

test('A failed final save keeps the old ROM and restores the form and platform',async()=>{
    let saves=0;
    const t=setup(async()=>({filename:'new.pcd',cd:true}),async()=>{if(++saves===2)throw new Error('disk full');});
    await vm.runInContext("importRomFile('/source/new.pcd','rom.rom',document.getElementById('f-rom'))",t.context);
    assert.equal(t.entry.game.rom.rom,'old.pcd');
    assert.equal(t.element('f-rom').value,'old.pcd');
    assert.equal(t.entry.game.display.csize,0);
    assert.equal(t.entry.game.rom.arch,'tg16');
    assert.ok(!t.calls.some(c=>c.command==='remove_replaced_rom'));
    assert.match(t.alerts[0],/disk full/);
    assert.equal(t.element('app').inert,false);
});

test('Cleanup failure is reported separately without rolling back the saved new ROM',async()=>{
    const t=setup(async command=>{
        if(command==='remove_replaced_rom') throw new Error('permission denied');
        return {filename:'new.pce',cd:false};
    });
    await vm.runInContext("importRomFile('/source/new.pce','rom.rom',document.getElementById('f-rom'))",t.context);
    assert.equal(t.entry.game.rom.rom,'new.pce');
    assert.equal(t.element('f-rom').value,'new.pce');
    assert.match(t.alerts[0],/new ROM was imported and saved.*previous file was kept/s);
    assert.match(t.alerts[0],/permission denied/);
    assert.equal(t.element('app').inert,false);
});

test('The operation remains locked through saving and cleanup',async()=>{
    let saves=0,finishSave,finishCleanup;
    const saveGate=new Promise(resolve=>finishSave=resolve);
    const cleanupGate=new Promise(resolve=>finishCleanup=resolve);
    const t=setup(async command=>command==='remove_replaced_rom'?cleanupGate:{filename:'new.pce',cd:false},
        async()=>{if(++saves===2)await saveGate;});
    const work=vm.runInContext("importRomFile('/source/new.pce','rom.rom',document.getElementById('f-rom'))",t.context);
    await new Promise(setImmediate);
    assert.equal(t.element('app').inert,true);
    assert.ok(!t.calls.some(c=>c.command==='remove_replaced_rom'));
    await vm.runInContext("importRomFile('/source/other.pce','rom.rom',document.getElementById('f-rom'))",t.context);
    assert.equal(t.calls.filter(c=>c.command==='import_rom').length,1);
    finishSave();await new Promise(setImmediate);
    assert.equal(t.calls.at(-1).command,'remove_replaced_rom');
    assert.equal(t.element('app').inert,true);
    finishCleanup();await work;
    assert.equal(t.element('app').inert,false);
    assert.equal(vm.runInContext('operationBusy',t.context),false);
});

test('ROM picker lists the shared pool including packed SuperGrafx ROMs',async()=>{
    const t=setup(async()=>['CD.pcd','Cart.pce.m','Super.sgx.m','cover.png']);
    const options=[];
    t.element('rom-datalist').appendChild=option=>options.push(option.value);
    t.context.document.createElement=()=>({});
    const code=app.slice(app.indexOf('async function populateRomDatalist('),app.indexOf('// ---- Folder cover import ----'));
    vm.runInContext(code,t.context);
    await t.context.populateRomDatalist(t.entry);
    assert.equal(t.calls[0].args.folderPath,'/key/library/published/roms');
    assert.deepEqual(options,['CD.pcd','Cart.pce.m','Super.sgx.m']);
});
