const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const app = fs.readFileSync(require('node:path').join(__dirname, '../src/app.js'), 'utf8');
const source = app.slice(app.indexOf('// ---- Move game between lineups/folders ----'), app.indexOf('// ---- Delete game ----'))
    + app.slice(app.indexOf('// ---- Auto-save ----'), app.indexOf('// ---- Helpers ----'));

function setup({titlebar=12,csize=0,sourceFolder=false,destinationFolder=false,saveError=false}={}) {
    const entry={folder:'GAME001',is_folder:false,sort:{},game:{display:{name:'Test',titlebar,csize},rom:{country:'jp'}}};
    const folder={folder:'FOLDER_001',is_folder:true,game_count:1,game:{display:{name_eng:'Folder'}}};
    const dualLibrary={path:'/test/library',jp:{games:[entry]},us:{games:destinationFolder?[folder]:[]}};
    const destination={lineup:'us',...(destinationFolder?{folder:folder.folder}:{})};
    const currentFolder=sourceFolder?{name:folder.folder,parentGames:[structuredClone(folder)]}:null;
    let persisted=structuredClone(entry);
    persisted.game.display.titlebar=9; // Deliberately stale until the pending save completes.
    let destinationContents;
    const calls=[],alerts=[];
    const context=vm.createContext({
        dualLibrary,currentFolder,currentLineup:'jp',selectedIndex:0,
        editorSettings:{force_us_titlebar:true},saveTimeout:null,saveInFlight:Promise.resolve(),
        setTimeout,clearTimeout,structuredClone,console:{error(){}},
        getLibrary:()=>dualLibrary.jp,modalSelect:async()=>destination,
        modalAlert:async message=>alerts.push(message),
        recomputeSortIndices(){},renderGameList(){},updateStats(){},
        document:{getElementById:()=>({style:{},classList:{remove(){}}})},
        invoke:async(command,args)=>{
            calls.push({command,args:structuredClone(args)});
            if(command==='save_library'||command==='save_folder_contents') {
                if(saveError) throw new Error('disk full');
                const games=command==='save_library'?args.library.jp.games:args.library.games;
                const saved=games.find(g=>g.folder===entry.folder);
                if(saved) persisted=structuredClone(saved);
                if(command==='save_folder_contents'&&args.lineup==='us') destinationContents=structuredClone(args.library);
            }
            if(command==='move_game') {persisted.game.rom.country='us';return entry.folder;}
            if(command==='load_game_entry') return structuredClone(persisted);
            if(command==='load_folder_contents') return {games:[]};
        },
    });
    vm.runInContext(source,context);
    return {context,calls,alerts,dualLibrary,
        moved:()=>destinationFolder?destinationContents?.games[0]:dualLibrary.us.games[0],
        async move(){vm.runInContext('autoSave()',context);await vm.runInContext('moveGame()',context);},
    };
}

test('JP to US moves preserve every titlebar, including an unsaved last edit',async()=>{
    for(let titlebar=0;titlebar<13;titlebar++) {
        for(const csize of [0,3]) {
            const t=setup({titlebar,csize});await t.move();
            assert.deepEqual(t.alerts,[]);
            assert.equal(t.moved().game.display.titlebar,titlebar);
            assert.equal(t.moved().game.rom.country,'us');
            assert.deepEqual(t.calls.map(c=>c.command),['save_library','move_game','load_game_entry','save_library']);
            assert.equal(t.context.saveTimeout,null);
        }
    }
});

test('Titlebar is retained when moving from or into a folder',async()=>{
    for(const sourceFolder of [false,true]) {
        for(const destinationFolder of [false,true]) {
            const t=setup({sourceFolder,destinationFolder});await t.move();
            assert.deepEqual(t.alerts,[]);
            assert.equal(t.moved().game.display.titlebar,12);
            assert.equal(t.calls[0].command,sourceFolder?'save_folder_contents':'save_library');
        }
    }
});

test('A failed pending save prevents moving and reloading stale game settings',async()=>{
    const t=setup({saveError:true});await t.move();
    assert.deepEqual(t.calls.map(c=>c.command),['save_library']);
    assert.match(t.alerts[0],/disk full/);
    assert.equal(t.dualLibrary.jp.games.length,1);
    assert.equal(t.dualLibrary.us.games.length,0);
});
