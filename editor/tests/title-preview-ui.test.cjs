const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const app = fs.readFileSync(require('node:path').join(__dirname, '../src/app.js'), 'utf8');
const source = app.slice(app.indexOf('// ---- Console title preview ----'), app.indexOf('// ---- Init ----'));

function setup() {
    const elements = Object.fromEntries(['title-preview-image','title-preview-status'].map(id=>[id,{
        classList:{add(){},remove(){}}
    }]));
    const game={display:{name:'日本語',tname:'魍魎',name_eng:'English',titlebar:0}};
    let timer;
    const calls=[];
    const context=vm.createContext({
        selectedIndex:0,dualLibrary:{path:'/test/library'},getLibrary:()=>({games:[{game}]}),
        document:{getElementById:id=>elements[id]},setTimeout:fn=>{timer=fn;},clearTimeout:()=>{timer=null;},
        invoke:(name,args)=>new Promise((resolve,reject)=>calls.push({name,args,resolve,reject}))
    });
    vm.runInContext(source,context);
    return {elements,context,calls,game,queue(){vm.runInContext('queueTitlePreview()',context);return timer();}};
}

test('Console preview uses the exported title and respects No titlebar and menu language',async()=>{
    const t=setup();let pending=t.queue();
    assert.equal(t.calls[0].args.text,'魍魎');assert.equal(t.calls[0].args.titlebar,0);
    t.calls[0].resolve({image:'first',scale:1});await pending;
    assert.equal(t.elements['title-preview-image'].src,'first');
    vm.runInContext("titlePreviewLanguage = 'en'", t.context);pending=t.queue();
    assert.equal(t.calls[1].args.text,'English');t.calls[1].resolve({image:'second',scale:0.5});await pending;
    assert.equal(t.elements['title-preview-status'].textContent,'');
});
test('Late previews cannot overwrite a newer title and missing characters show an error',async()=>{
    const t=setup();const first=t.queue();t.game.display.tname='New title';const second=t.queue();
    t.calls[1].resolve({image:'new',scale:1});await second;
    t.calls[0].resolve({image:'old',scale:1});await first;
    assert.equal(t.elements['title-preview-image'].src,'new');
    const third=t.queue();t.calls[2].reject('Missing U+10FFFF');await third;
    assert.equal(t.elements['title-preview-image'].hidden,true);
    assert.match(t.elements['title-preview-status'].textContent,/U\+10FFFF/);
});
