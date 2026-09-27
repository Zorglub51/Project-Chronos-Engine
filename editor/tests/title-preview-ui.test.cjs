const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const app = fs.readFileSync(require('node:path').join(__dirname, '../src/app.js'), 'utf8');
const source = app.slice(app.indexOf('// ---- Console title preview ----'), app.indexOf('// ---- Init ----'));

function setup({manualDecode = false} = {}) {
    const decodes = [];
    function image(src = '', hidden = true) {
        return {src, hidden,
            cloneNode() { return image(this.src, this.hidden); },
            decode() {
                return manualDecode
                    ? new Promise((resolve, reject) => decodes.push({resolve, reject}))
                    : Promise.resolve();
            },
            replaceWith(next) { elements['title-preview-image'] = next; },
        };
    }
    const elements = {
        'title-preview-image': image(),
        'title-preview-status': {textContent:'', classList:{add(){},remove(){}}},
    };
    const entry = {game:{display:{name:'日本語',tname:'魍魎',name_eng:'English',titlebar:0}}};
    let timer;
    const calls = [];
    const context = vm.createContext({
        selectedIndex:0, dualLibrary:{path:'/test/library'}, getLibrary:()=>({games:[entry]}),
        document:{getElementById:id=>elements[id]},
        setTimeout:fn=>{timer=fn;}, clearTimeout:()=>{timer=null;},
        invoke:(name,args)=>new Promise((resolve,reject)=>calls.push({name,args,resolve,reject})),
    });
    vm.runInContext(source, context);
    return {elements,context,calls,decodes,entry,game:entry.game,
        queue(){vm.runInContext('queueTitlePreview()',context);},
        fire(){const fn=timer;timer=null;return fn();},
    };
}

test('Console preview uses the exported title and respects white titlebar and menu language',async()=>{
    const t=setup();t.queue();let pending=t.fire();
    assert.equal(t.calls[0].args.text,'魍魎');assert.equal(t.calls[0].args.titlebar,0);
    t.calls[0].resolve({image:'first'});await pending;
    assert.equal(t.elements['title-preview-image'].src,'first');
    vm.runInContext("titlePreviewLanguage = 'en'",t.context);t.queue();pending=t.fire();
    assert.equal(t.calls[1].args.text,'English');t.calls[1].resolve({image:'second'});await pending;
    assert.equal(t.elements['title-preview-status'].textContent,'');
});

test('Typing and language changes keep the last preview visible until the replacement is decoded',async()=>{
    const t=setup({manualDecode:true});t.queue();let pending=t.fire();
    t.calls[0].resolve({image:'first'});await new Promise(setImmediate);
    t.decodes[0].resolve();await pending;
    const visible=t.elements['title-preview-image'];
    for(const title of ['Y','Ys','Ys IV']) {
        t.game.display.tname=title;t.queue();
        assert.equal(visible.hidden,false);
        assert.equal(visible.src,'first');
        assert.equal(t.elements['title-preview-status'].textContent,'');
    }
    assert.equal(t.calls.length,1,'typing is debounced');
    pending=t.fire();assert.equal(t.calls[1].args.text,'Ys IV');
    t.calls[1].resolve({image:'second'});await new Promise(setImmediate);
    assert.equal(t.elements['title-preview-image'],visible,'undecoded image is never displayed');
    // A language change during decoding invalidates the old result too.
    vm.runInContext("titlePreviewLanguage = 'en'",t.context);t.queue();
    t.decodes[1].resolve();await pending;
    assert.equal(t.elements['title-preview-image'],visible);
    pending=t.fire();t.calls[2].resolve({image:'english'});await new Promise(setImmediate);
    t.decodes[2].resolve();await pending;
    assert.equal(t.elements['title-preview-image'].src,'english');
    assert.equal(t.elements['title-preview-image'].hidden,false);
});

test('Late renders and errors cannot overwrite a newer preview',async()=>{
    const t=setup();t.queue();const first=t.fire();
    t.game.display.tname='New title';t.queue();const second=t.fire();
    t.calls[1].resolve({image:'new'});await second;
    t.calls[0].resolve({image:'old'});await first;
    assert.equal(t.elements['title-preview-image'].src,'new');
    t.queue();const third=t.fire();t.queue();const fourth=t.fire();
    t.calls[3].resolve({image:'newest'});await fourth;
    t.calls[2].reject('obsolete error');await third;
    assert.equal(t.elements['title-preview-image'].src,'newest');
    assert.equal(t.elements['title-preview-image'].hidden,false);
    assert.equal(t.elements['title-preview-status'].textContent,'');
});

test('Render/decode failures are reported and a later valid preview recovers',async()=>{
    const t=setup({manualDecode:true});t.queue();let pending=t.fire();
    t.calls[0].reject('Missing U+10FFFF');await pending;
    assert.equal(t.elements['title-preview-image'].hidden,true);
    assert.match(t.elements['title-preview-status'].textContent,/U\+10FFFF/);
    assert.equal(t.elements['title-preview-status'].title,t.elements['title-preview-status'].textContent);
    t.queue();pending=t.fire();t.calls[1].resolve({image:'invalid'});await new Promise(setImmediate);
    t.decodes[0].reject('Invalid PNG');await pending;
    assert.match(t.elements['title-preview-status'].textContent,/Invalid PNG/);
    t.queue();pending=t.fire();t.calls[2].resolve({image:'valid'});await new Promise(setImmediate);
    t.decodes[1].resolve();await pending;
    assert.equal(t.elements['title-preview-image'].src,'valid');
    assert.equal(t.elements['title-preview-image'].hidden,false);
    assert.equal(t.elements['title-preview-status'].textContent,'');
});
