const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../src/app.js'), 'utf8');
const html = fs.readFileSync(path.join(__dirname, '../src/index.html'), 'utf8');
const source = app.slice(app.indexOf('// ---- Console title preview ----'), app.indexOf('// ---- Home and console workspace ----'))
    + app.slice(app.indexOf('// ---- Select game ----'), app.indexOf('// csize → rom.arch mapping'));
const copyListener = app.slice(app.indexOf("document.getElementById('btn-copy-name-to-jp')"), app.indexOf("document.getElementById('btn-add-genre')"));
const fieldListeners = app.slice(app.indexOf("document.querySelectorAll('[data-path]')"), app.indexOf('// Cover drop zones (click only'));

// These names deliberately differ: stock `name` is an internal label, whereas
// the two `tname` blocks become `tname` and `name_eng` when a game is imported.
const stockTitles = [
    {folder: 'GAME005', name: '天外魔境2', tname: '天外魔境II 卍MARU', name_eng: 'Tengai Makyō II Manjimaru'},
    {folder: 'GAME046', name: 'Power Golf', tname: 'POWER GOLF', name_eng: 'Power Golf'},
];

function setup(title, lineup = 'jp') {
    const entry = {
        folder: title.folder,
        game: {
            display: {...title, titlebar: 4, players: 1, ccolor: 0, csize: 0, demo_time: 0},
            rom: {rom: 'game.pce', arch: 'tg16', country: 'jp'},
            genre: 0,
        },
    };
    delete entry.game.display.folder;
    const elements = new Map();
    function element(id) {
        if (!elements.has(id)) {
            const result = {
                value: '', textContent: '', src: '', hidden: true, style: {}, dataset: {},
                classList: {add() {}, remove() {}, toggle() {}},
                setAttribute(key, value) {this[key] = value;},
                addEventListener(event, handler) {this[event] = handler;},
                dispatchEvent(event) {this[event.type]?.(event);},
                cloneNode() {return {...this};},
                decode() {return Promise.resolve();},
                replaceWith(next) {elements.set(id, next);},
            };
            // Read the real form binding, so a regression in index.html is
            // exercised by the same field-change handler used by the UI.
            const tag = html.match(new RegExp('<(input|select)\\b[^>]*\\bid="' + id + '"[^>]*>'));
            if (tag) {
                result.tagName = tag[1].toUpperCase();
                result.type = tag[0].match(/\btype="([^"]+)"/)?.[1] || '';
                result.dataset.path = tag[0].match(/\bdata-path="([^"]+)"/)?.[1];
            }
            elements.set(id, result);
        }
        return elements.get(id);
    }
    const languageButtons = ['jp', 'en'].map(language => {
        const button = element('language-' + language);
        button.dataset.previewLanguage = language;
        return button;
    });
    const listName = element('list-name');
    const listItem = {...element('list-item'), querySelector: () => listName};
    const previews = [], saves = [];
    let timer;
    const context = vm.createContext({
        currentLineup: lineup, currentFolder: null, selectedIndex: -1,
        dualLibrary: {path: '/key/library'}, getLibrary: () => ({games: [entry]}),
        coverCache: {[lineup + '/' + entry.folder]: 'cover'}, expertEditor: null,
        document: {
            getElementById: element,
            querySelectorAll: selector => selector === '[data-preview-language]' ? languageButtons
                : selector === '[data-path]' ? ['f-name', 'f-name-eng'].map(element) : [listItem],
            fonts: {add() {}},
        },
        Event,
        FontFace: class {async load() {}},
        invoke: async (command, args) => {
            if (command === 'title_preview') previews.push(structuredClone(args));
            return command === 'title_preview' ? {image: 'rendered-' + previews.length} : [];
        },
        autoSave: () => saves.push(structuredClone(entry.game)),
        setTimeout: fn => {timer = fn;}, clearTimeout: () => {timer = null;},
        updateCoverBoxColor() {}, updateTitlebarSelection() {}, populateRomDatalist() {},
        loadSaveStates() {}, recomputeSortIndices() {},
    });
    vm.runInContext(source + copyListener + fieldListeners + ';initTitlePreview();selectGame(0);', context);
    return {
        entry, element, previews, saves,
        async render() {
            assert.equal(typeof timer, 'function', 'a metadata or language edit must queue a new spine preview');
            const fn = timer;
            timer = null;
            await fn();
            return previews.at(-1);
        },
        language(value) {languageButtons.find(button => button.dataset.previewLanguage === value).click();},
        edit(id, value) {
            const field = element(id);
            field.value = value;
            field.dispatchEvent(new Event('input', {bubbles: true}));
        },
        copyEnglishToJapanese() {element('btn-copy-name-to-jp').click();},
        editInternalName(value) {
            context.changedField = {value, type: 'text', tagName: 'INPUT', dataset: {path: 'display.name'}};
            vm.runInContext('onFieldChange(changedField)', context);
        },
    };
}

test('Imported stock spine names populate the form and preview by menu language in either lineup', async () => {
    for (const lineup of ['jp', 'us']) for (const title of stockTitles) {
        const t = setup(title, lineup);
        assert.equal(t.element('f-name').value, title.tname);
        assert.equal(t.element('f-name-eng').value, title.name_eng);
        assert.equal(t.element('editor-title').textContent, title.name_eng + ' / ' + title.tname);
        assert.equal((await t.render()).text, title.tname, lineup + ' lineup with Japanese menu');
        t.language('en');
        assert.equal((await t.render()).text, title.name_eng, lineup + ' lineup with English menu');
        t.language('jp');
        assert.equal((await t.render()).text, title.tname);
        assert.equal(t.entry.game.display.name, title.name);
        assert.equal(t.entry.game.display.tname, title.tname);
        assert.equal(t.entry.game.display.name_eng, title.name_eng);
        assert.equal(t.saves.length, 0, 'selection and language changes must not rewrite title metadata');
    }
});

test('Editing the Japanese title saves and previews tname without replacing the internal stock name', async () => {
    for (const lineup of ['jp', 'us']) {
        const t = setup(stockTitles[0], lineup);
        await t.render();
        const title = '天外魔境II 卍MARU 龘';
        t.edit('f-name', title);
        assert.equal(t.entry.game.display.tname, title);
        assert.equal(t.entry.game.display.name, '天外魔境2');
        assert.equal(t.entry.game.display.name_eng, 'Tengai Makyō II Manjimaru');
        assert.equal(t.saves.length, 1);
        assert.equal(t.saves[0].display.tname, title);
        assert.equal(t.saves[0].display.name, '天外魔境2');
        assert.equal(t.element('editor-title').textContent, 'Tengai Makyō II Manjimaru / ' + title);
        assert.equal((await t.render()).text, title);
        t.language('en');
        assert.equal((await t.render()).text, 'Tengai Makyō II Manjimaru');
    }
});

test('Editing the English spine preserves case and the Japanese spine, including in the US lineup', async () => {
    for (const lineup of ['jp', 'us']) {
        const t = setup(stockTitles[1], lineup);
        await t.render();
        t.language('en');
        await t.render();
        t.edit('f-name-eng', 'Power Golf II');
        assert.equal(t.entry.game.display.name, 'Power Golf');
        assert.equal(t.entry.game.display.tname, 'POWER GOLF');
        assert.equal(t.saves[0].display.name_eng, 'Power Golf II');
        assert.equal(t.element('editor-title').textContent, 'Power Golf II / POWER GOLF');
        assert.equal((await t.render()).text, 'Power Golf II');
        t.language('jp');
        assert.equal((await t.render()).text, 'POWER GOLF');
    }
});

test('Editing an internal stock name never overwrites the distinct Japanese tname', async () => {
    const t = setup(stockTitles[0]);
    await t.render();
    t.editInternalName('Internal label only');
    assert.equal(t.entry.game.display.name, 'Internal label only');
    assert.equal(t.entry.game.display.tname, '天外魔境II 卍MARU');
    assert.equal(t.element('editor-title').textContent, 'Tengai Makyō II Manjimaru / 天外魔境II 卍MARU');
    // Trigger a normal language toggle too: the resulting spine must still use
    // the Japanese title after an internal metadata edit.
    t.language('en');
    await t.render();
    t.language('jp');
    assert.equal((await t.render()).text, '天外魔境II 卍MARU');
});

test('The English-to-Japanese copy button follows the form binding and preserves the internal name', async () => {
    for (const lineup of ['jp', 'us']) {
        const t = setup(stockTitles[0], lineup);
        await t.render();
        t.copyEnglishToJapanese();
        assert.equal(t.element('f-name').value, 'Tengai Makyō II Manjimaru');
        assert.equal(t.entry.game.display.tname, 'Tengai Makyō II Manjimaru');
        assert.equal(t.entry.game.display.name, '天外魔境2');
        assert.equal(t.entry.game.display.name_eng, 'Tengai Makyō II Manjimaru');
        assert.equal(t.saves.length, 1);
        assert.equal(t.saves[0].display.tname, 'Tengai Makyō II Manjimaru');
        assert.equal(t.saves[0].display.name, '天外魔境2');
        assert.equal(t.element('editor-title').textContent, 'Tengai Makyō II Manjimaru / Tengai Makyō II Manjimaru');
        assert.equal((await t.render()).text, 'Tengai Makyō II Manjimaru');
    }
});

test('Legacy entries without a Japanese tname remain editable and acquire one without changing their name', async () => {
    for (const tname of [undefined, '']) {
        const t = setup({folder: 'GAME001', name: 'Legacy Title', tname, name_eng: 'English Title'});
        assert.equal(t.element('f-name').value, 'Legacy Title');
        assert.equal(t.element('editor-title').textContent, 'English Title / Legacy Title');
        assert.equal((await t.render()).text, 'Legacy Title');
        t.edit('f-name', '新しいタイトル');
        assert.equal(t.entry.game.display.name, 'Legacy Title');
        assert.equal(t.entry.game.display.tname, '新しいタイトル');
        assert.equal(t.saves[0].display.tname, '新しいタイトル');
        assert.equal((await t.render()).text, '新しいタイトル');
    }
});
