const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const catalog = JSON.parse(fs.readFileSync(path.join(__dirname, '../src/game-parameters.json')));
const source = fs.readFileSync(path.join(__dirname, '../src/expert.js'), 'utf8');

function setup() {
    const nodes = new Map();
    class Element {
        constructor(tag) { this.tagName = tag.toUpperCase(); this.children = []; this.dataset = {}; this.events = {}; this.attributes = {}; this.value = ''; }
        set id(value) { this._id = value; nodes.set(value, this); }
        get id() { return this._id; }
        get options() { return this.children; }
        append(...nodes) { for (const node of nodes) { this.children.push(node); node.parent = this; } }
        replaceChildren(...nodes) { this.children = []; this.append(...nodes); }
        remove() { this.parent.children = this.parent.children.filter(node => node !== this); }
        addEventListener(event, handler) { this.events[event] = handler; }
        setAttribute(key, value) { this.attributes[key] = value; }
        removeAttribute(key) { delete this.attributes[key]; }
    }
    const context = vm.createContext({ window: {}, document: {
        createElement: tag => new Element(tag), createTextNode: text => ({ textContent: text }),
    } });
    vm.runInContext(source, context);
    const api = context.window.ChronosExpert;
    const container = new Element('div'), changes = [];
    const editor = api.create(container, catalog, key => changes.push(key));
    const input = key => nodes.get('expert-' + key.replaceAll('.', '-'));
    const change = (key, value) => { const control = input(key); control.value = value; control.events.change(); };
    return { api, editor, input, change, changes, container };
}

test('Opening/switching games preserves inherited settings, unknown metadata and high stock gain', () => {
    const t = setup();
    const game = { rom: { preamp: 6.94, country: 'jp', future_option: { enabled: true } }, emulator: { ScreenMode: 4 } };
    const before = JSON.stringify(game);
    t.editor.render(game);
    assert.equal(JSON.stringify(game), before);
    assert.equal(t.input('rom.preamp').value, '6.94');
    assert.equal(t.input('rom.overdrive').value, '');
    t.change('rom.overdrive', '2');
    assert.equal(game.rom.overdrive, 2);
    const second = { rom: { preamp: 0, country: 'us' } };
    t.editor.render(second);
    assert.equal(t.input('rom.overdrive').value, '');
    t.change('rom.preamp', '-6');
    assert.equal(second.rom.preamp, -6);
    assert.equal(game.rom.preamp, 6.94);
    assert.equal(game.rom.future_option.enabled, true);
});

test('Explicit zero and false survive; Inherit removes overrides instead of writing null', () => {
    const t = setup(), game = { rom: { preamp: 0 } };
    t.editor.render(game);
    t.change('rom.overdrive', '0');
    t.change('rom.use_analog', 'false');
    t.change('emulator.EmuOfsX', '-8');
    assert.equal(game.rom.overdrive, 0);
    assert.equal(game.rom.use_analog, false);
    assert.equal(game.emulator.EmuOfsX, -8);
    t.input('rom.overdrive').parent.children[1].events.click();
    assert.equal(Object.hasOwn(game.rom, 'overdrive'), false);
    assert.ok(t.changes.includes('rom.overdrive'));
});

test('Complex values keep their types; invalid edits cannot replace saved valid values', () => {
    const t = setup(), game = { rom: { preamp: 0, tg16_option: 5 } };
    t.editor.render(game);
    t.change('rom.view_split', '[2, 3]');
    t.change('rom.attr', '{"mode":0}');
    t.change('rom.string', '{"driver":"test"}');
    assert.equal(JSON.stringify(game.rom.view_split), '[2,3]');
    assert.equal(game.rom.attr.mode, 0);
    assert.equal(game.rom.string.driver, 'test');
    const count = t.changes.length;
    for (const [key, value] of [['rom.tg16_option', '16'], ['rom.attr', '{"mode":false}'],
        ['rom.view_split', '[0,1]'], ['rom.preamp', ''], ['rom.preamp', 'Infinity'],
        ['rom.attr', '{"__chronos_array":1}']]) {
        t.change(key, value);
        assert.equal(t.input(key).attributes['aria-invalid'], 'true');
    }
    assert.equal(t.changes.length, count);
    assert.equal(game.rom.tg16_option, 5);
    assert.equal(game.rom.attr.mode, 0);
    assert.equal(game.rom.preamp, 0);
});

test('Ordinary and managed fields are not duplicated; preamp belongs to Expert', () => {
    const t = setup();
    assert.ok(t.input('rom.preamp'));
    assert.equal(t.input('rom.rom'), undefined);
    assert.equal(t.input('display.players'), undefined);
    assert.equal(t.input('rom.systemdata_sram_data_no'), undefined);
    const html = fs.readFileSync(path.join(__dirname, '../src/index.html'), 'utf8');
    assert.match(html, /data-section="expert"/);
    assert.ok(!html.includes('f-preamp-slider'));
});

test('Information bubbles expose English effects and values on hover, focus, click and Escape', () => {
    const t = setup();
    const field = catalog.parameters.find(field => field.key === 'tg16_option');
    const bubble = t.api.infoBubble(field);
    const [button, tooltip] = bubble.children;
    assert.equal(tooltip.hidden, true);
    assert.match(tooltip.children.map(node => node.textContent).join(' '), /Possible values: Whole number, 0 to 15/);
    assert.match(tooltip.children[0].textContent, /VBlank/);
    bubble.events.pointerenter();
    assert.equal(tooltip.hidden, false);
    bubble.events.pointerleave();
    assert.equal(tooltip.hidden, true);
    button.events.focus();
    assert.equal(button.attributes['aria-expanded'], 'true');
    button.events.keydown({ key: 'Escape', stopPropagation() {} });
    assert.equal(tooltip.hidden, true);
    button.events.click({ preventDefault() {}, stopPropagation() {} });
    assert.equal(tooltip.hidden, false);
    assert.equal(button.attributes['aria-describedby'], tooltip.id);
});

test('ScreenMode choices and information follow the selected native core', () => {
    const t = setup();
    for (const [arch, label] of [['tg16', '4 · 272 → 256, 352 → 336, 512 → 256'],
        ['tg16cd', '4 · Same crop as 3'], ['sgx', '4 · Full raster']]) {
        t.editor.render({ rom: { arch, preamp: 0 }, emulator: { ScreenMode: 4 } });
        const input = t.input('emulator.ScreenMode');
        assert.equal(input.options.find(option => option.value === '4').textContent, label);
        const row = input.parent.parent;
        const tooltip = row.children[1].children[1];
        assert.ok(tooltip.children.map(node => node.textContent).join(' ').includes(label));
    }
});
