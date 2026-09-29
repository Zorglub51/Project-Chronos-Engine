const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs');
const vm = require('node:vm');

// Exercise event sequencing without USB access: the UI must not tell the user
// to power on merely because a command was queued on the frontend.
const source = fs.readFileSync(`${__dirname}/app.js`, 'utf8');
function setup(failure) {
  const elements = new Map();
  const get = id => {
    if (!elements.has(id)) elements.set(id, {
      value: id === 'payloads-dir' ? '/payloads' : '120',
      textContent: '', className: '', replaceChildren() {}, appendChild() {},
    });
    return elements.get(id);
  };
  const events = new Map(), calls = [], buttons = [{ disabled: false }];
  const context = vm.createContext({
    window: { __TAURI__: {
      core: { invoke: async (command, args) => {
        calls.push({command, args});
        if (command === 'start_recovery' && failure) throw Error(failure);
        return [];
      } },
      event: { listen: async (name, fn) => events.set(name, fn) },
    } },
    document: { getElementById: get, querySelectorAll: () => buttons, createElement: () => ({}) },
  });
  vm.runInContext(source.slice(0, source.indexOf('\n(async()=>{')), context);
  vm.runInContext('buildSteps = () => {};', context);
  return { context, calls, buttons, get, emit: (name, payload) => events.get(name)({payload}) };
}

test('power-on prompt waits for backend detection phase, then startup runs automatically', async () => {
  const ui = setup();
  await ui.context.wireBackend();
  await ui.context.startRecovery();
  assert.equal(ui.calls[0].command, 'start_recovery');
  assert.match(ui.get('recovery-prompt').textContent, /Keep the console OFF/);
  assert.equal(ui.buttons[0].disabled, true);
  await ui.emit('phase-start', {phase: 'Connect', total: 0, label: 'Waiting'});
  assert.match(ui.get('recovery-prompt').textContent, /switch the console ON now/);
  await ui.emit('phase-start', {phase: 'Trigger', total: 0, label: 'Trigger'});
  assert.match(ui.get('recovery-prompt').textContent, /running automatically/);
  assert.equal(ui.calls.length, 1, 'no extra click or frontend command is needed to answer the probe');
  await ui.emit('recovery-done', {ok: false, msg: 'No startup USB probe detected. Switch OFF and retry.'});
  assert.match(ui.get('recovery-prompt').textContent, /Switch OFF and retry/);
  assert.equal(ui.buttons[0].disabled, false);
});

test('failure to start detection keeps the power-on instruction hidden and unlocks retry', async () => {
  const ui = setup('Missing recovery files');
  await ui.context.startRecovery();
  assert.match(ui.get('recovery-prompt').textContent, /Recovery failed:.*Missing recovery files/);
  assert.doesNotMatch(ui.get('recovery-prompt').textContent, /switch the console ON now/);
  assert.equal(ui.buttons[0].disabled, false);
});
