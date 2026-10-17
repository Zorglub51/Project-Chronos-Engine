const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const vm=require('node:vm');
const app=fs.readFileSync(path.join(__dirname,'../src/app.js'),'utf8');
const source=app.slice(app.indexOf('function updateMenuLimitWarning('),app.indexOf('// ---- Render game list ----'));

function setup(jp,us=[],folder=null) {
    const warning={hidden:true,textContent:''};
    const library={games:jp};
    const context=vm.createContext({
        dualLibrary:{jp:library,us:{games:us}},currentFolder:folder,
        getLibrary:()=>library,document:{getElementById:()=>warning},
    });
    vm.runInContext(source,context);
    return {warning,context,library};
}

test('The native 50-entry root limit warns without removing entries or blocking editing',()=>{
    const t=setup(Array.from({length:51},()=>({is_folder:false})),[
        {is_folder:true,game_count:50,game:{display:{name_eng:'HuCards'}}},
    ]);
    t.context.updateMenuLimitWarning();
    assert.equal(t.warning.hidden,false);
    assert.match(t.warning.textContent,/JP has 51 menu entries/);
    assert.match(t.warning.textContent,/HuCards \(US\) has 50 games/);
    assert.equal(t.library.games.length,51);
    t.library.games.pop();
    t.context.dualLibrary.us.games[0].game_count=49;
    t.context.updateMenuLimitWarning();
    assert.equal(t.warning.hidden,true);
    assert.equal(t.warning.textContent,'');
});

test('A folder reserves its Back entry and permits all 49 game positions',()=>{
    const t=setup(Array.from({length:49},()=>({is_folder:false})),[],{name:'FOLDER'});
    t.context.updateMenuLimitWarning();
    assert.equal(t.warning.hidden,true);
    t.library.games.push({is_folder:false});
    t.context.updateMenuLimitWarning();
    assert.match(t.warning.textContent,/This folder has 50 games/);
    assert.match(t.warning.textContent,/49 games plus Back/);
    assert.equal(t.library.games.length,50);
});
