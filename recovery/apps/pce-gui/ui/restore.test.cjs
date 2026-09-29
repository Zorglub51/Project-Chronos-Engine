const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs');
const vm = require('node:vm');

// Exercise the real restore handler with Tauri calls mocked. No console access.
const source = fs.readFileSync(`${__dirname}/app.js`, 'utf8');
const handler = source.slice(source.indexOf('async function doRestore(p) {'), source.indexOf('\n// ---- recovery controls'));

class Element {
  constructor(tag = 'span') {
    this.tag = tag;
    this.children = [];
    this.listeners = {};
    this.disabled = false;
    this.value = '';
    this.classList = { add() {}, remove() {} };
  }
  set textContent(value) { this.text = value; this.children = []; }
  get textContent() { return this.text || ''; }
  set innerHTML(_) { throw Error('Restore filenames must never be inserted as HTML'); }
  setAttribute() {}
  appendChild(child) { this.children.push(child); return child; }
  replaceChildren() { this.children = []; this.text = ''; }
  addEventListener(type, fn) { this.listeners[type] = fn; }
  async event(type) { return this.listeners[type]?.(); }
  find(className) { return this.children.find(child => child.className === className); }
}

function setup(entries, options = {}) {
  const cell = new Element();
  const buttons = [new Element('button'), new Element('button')];
  const row = { querySelector: () => cell, querySelectorAll: () => buttons };
  const calls = [];
  const context = vm.createContext({
    busy: false,
    setBusy: value => buttons.forEach(b => { b.disabled = value; }),
    document: { querySelector: () => row, createElement: tag => new Element(tag) },
    fmtBytes: n => `${n} bytes`,
    invoke: async (command, args) => {
      calls.push({ command, args });
      if (command === 'pick_open_file') return options.cancel ? null : '/tmp/kit.zip';
      if (command === 'partition_inspect_image') {
        if (options.error) throw Error(options.error);
        return entries;
      }
      if (command === 'partition_restore' && options.restoreError) throw Error(options.restoreError);
    },
  });
  vm.runInContext(handler, context);
  return { cell, buttons, calls, run: () => context.doRestore({ id: 7, device_path: '/dev/mmcblk0p7', size_bytes: 100 }) };
}

const image = (zip_index, name = 'p7.bin') => ({ zip_index, name, size: 100, crc32: 42 });

test('single ZIP image is named safely and restore waits for confirmation', async () => {
  const entry = image(1, '<img src=x onerror=alert(1)>.bin');
  const ui = setup([entry]);
  await ui.run();
  assert.equal(ui.calls.length, 2);
  assert.ok(ui.cell.children.some(el => el.textContent.includes(entry.name)));
  assert.ok(ui.buttons.every(button => button.disabled));
  await ui.cell.find('confirm-yes').event('click');
  assert.equal(ui.calls.at(-1).command, 'partition_restore');
  assert.equal(ui.calls.at(-1).args.selection, entry);
});

test('multiple ZIP entries require explicit choice before confirmation', async () => {
  const entries = [image(1, 'stock.bin'), image(3, 'mod.bin')];
  const ui = setup(entries);
  await ui.run();
  const confirm = ui.cell.find('confirm-yes');
  const select = ui.cell.find('restore-entry');
  assert.equal(confirm.disabled, true);
  await confirm.event('click');
  assert.equal(ui.calls.length, 2);
  select.value = '1';
  await select.event('change');
  assert.equal(confirm.disabled, false);
  await confirm.event('click');
  assert.equal(ui.calls.at(-1).args.selection, entries[1]);
});

test('cancel confirmation never starts a restore', async () => {
  const ui = setup([image(0)]);
  await ui.run();
  await ui.cell.find('confirm-no').event('click');
  assert.equal(ui.calls.length, 2);
  assert.ok(ui.buttons.every(button => !button.disabled));
  assert.equal(ui.cell.children.length, 0);
});

test('invalid archive fails before confirmation and unlocks row', async () => {
  const ui = setup([], { error: 'no image matching partition size' });
  await ui.run();
  assert.match(ui.cell.textContent, /no image matching/);
  assert.equal(ui.cell.find('confirm-yes'), undefined);
  assert.equal(ui.calls.length, 2);
  assert.ok(ui.buttons.every(button => !button.disabled));
});

test('raw files preserve the confirmation workflow', async () => {
  const ui = setup([{ ...image(null), crc32: null }]);
  await ui.run();
  assert.equal(ui.cell.find('restore-entry'), undefined);
  await ui.cell.find('confirm-yes').event('click');
  assert.equal(ui.calls.at(-1).args.selection.zip_index, null);
});

test('cancel file dialog and command errors release row buttons', async () => {
  const cancelled = setup([], { cancel: true });
  await cancelled.run();
  assert.equal(cancelled.calls.length, 1);
  assert.ok(cancelled.buttons.every(button => !button.disabled));
  const failed = setup([image(0)], { restoreError: 'command failed' });
  await failed.run();
  await failed.cell.find('confirm-yes').event('click');
  assert.match(failed.cell.textContent, /command failed/);
  assert.ok(failed.buttons.every(button => !button.disabled));
});
