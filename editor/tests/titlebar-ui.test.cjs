const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const app = fs.readFileSync(require('node:path').join(__dirname, '../src/app.js'), 'utf8');
const source = app.slice(app.indexOf('// Returns the forced titlebar'), app.indexOf('// ---- Cover ----'));

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
    vm.runInContext(source+';initTitlebarPicker();updateTitlebarSelection(getLibrary().games[0].game.display.titlebar);applyForcedTitlebar();',context);
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

test('Forced US styles preserve No titlebar and allow switching between none and the US banner',()=>{
    for(const [csize,forced] of [[0,9],[1,9],[3,10],[4,10]]) {
        const t=setup('us',0,csize);
        assert.equal(t.entry.game.display.titlebar,0);
        assert.equal(t.options[0].disabled,false);
        assert.equal(t.options[forced].disabled,false);
        assert.equal(t.options[4].disabled,true);
        t.choose(4);
        assert.equal(t.entry.game.display.titlebar,0);
        t.choose(forced);
        assert.equal(t.entry.game.display.titlebar,forced);
        t.choose(0);
        vm.runInContext('applyForcedTitlebar()',t.context);
        assert.equal(t.entry.game.display.titlebar,0);
        assert.equal(t.field.value,'0');
    }
    const t=setup('us',4);
    assert.equal(t.entry.game.display.titlebar,9);
    t.context.editorSettings.force_us_titlebar=false;
    vm.runInContext('applyForcedTitlebar()',t.context);
    assert.ok(t.options.every(opt=>!opt.disabled));
});
