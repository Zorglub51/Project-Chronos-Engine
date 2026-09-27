const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const app = fs.readFileSync(require('node:path').join(__dirname, '../src/app.js'), 'utf8');
const source = app.slice(app.indexOf('// ---- Titlebar picker ----'), app.indexOf('// ---- Cover ----'));

function setup(lineup, titlebar, csize = 0) {
    const options = [];
    function element() {
        return {dataset:{},attributes:{},children:[],
            setAttribute(name,value){this.attributes[name]=value;},
            appendChild(child){this.children.push(child);},
            replaceChildren(child){this.children=[child];},
            addEventListener(name,fn){this[name]=fn;},focus(){}};
    }
    const elements = Object.fromEntries(['f-titlebar','titlebar-picker','titlebar-toggle','titlebar-options','titlebar-selected'].map(id=>[id,element()]));
    elements['titlebar-options'].appendChild=option=>options.push(option);
    const field = elements['f-titlebar'];
    const entry = {game:{display:{titlebar,csize}}};
    let saves = 0;
    const context = vm.createContext({
        editorSettings:{force_us_titlebar:true}, currentLineup:lineup, selectedIndex:0,
        getLibrary:()=>({games:[entry]}), autoSave:()=>saves++,
        queueTitlePreview:()=>{},
        document:{
            getElementById:id=>elements[id],
            querySelectorAll:()=>options,
            createElement:element,
            addEventListener(){},
        },
    });
    vm.runInContext(source+';initTitlebarPicker();updateTitlebarSelection(getLibrary().games[0].game.display.titlebar);',context);
    return {context,options,entry,field,saves:()=>saves,
        choose(value){options[value].click();}};
}

test('Stock Neutopia II value zero is visibly selected and can be restored after editing',()=>{
    const t=setup('jp',0);
    assert.equal(t.options.length,13);
    assert.equal(t.options[0].attributes['aria-label'],'White bar');
    assert.equal(t.options[0].children[0].className,'titlebar-white');
    assert.equal(t.field.value,'0');
    t.choose(4);
    assert.equal(t.entry.game.display.titlebar,4);
    t.choose(0);
    assert.equal(t.entry.game.display.titlebar,0);
    assert.equal(t.field.value,'0');
    assert.equal(t.saves(),2);
});

test('Every titlebar is available in either lineup on every platform, even with legacy settings',()=>{
    for(const lineup of ['jp','us']) {
        for(const csize of [0,1,2,3,4]) {
            const t=setup(lineup,12,csize);
            assert.equal(t.entry.game.display.titlebar,12);
            assert.equal(t.field.value,'12');
            assert.ok(t.options.every(opt=>!opt.disabled));
            for(let value=0;value<13;value++) {
                t.choose(value);
                assert.equal(t.entry.game.display.titlebar,value);
                assert.equal(t.field.value,String(value));
                assert.equal(t.options[value].attributes['aria-pressed'],'true');
            }
        }
    }
});
