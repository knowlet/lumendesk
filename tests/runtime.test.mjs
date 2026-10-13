// JS runtime unit tests only. A node:vm window mock does not exercise an OS
// webview, IPC origin metadata, real DOM frame isolation, navigation, or input.
// Run with: node --test tests/runtime.test.mjs
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const source = await readFile(new URL('../crates/lumendesk/src/runtime.js', import.meta.url), 'utf8');
const SESSION = 'independent-unit-test-session';
const RUNTIME = { platform: 'test-mock', windowId: 7 };

function trackedAbortController() {
  const controller = new AbortController();
  const { signal } = controller;
  const listeners = new Set();
  const add = signal.addEventListener.bind(signal);
  const remove = signal.removeEventListener.bind(signal);
  signal.addEventListener = (name, callback, options) => {
    if (name === 'abort') listeners.add(callback);
    add(name, callback, options);
  };
  signal.removeEventListener = (name, callback, options) => {
    if (name === 'abort') listeners.delete(callback);
    remove(name, callback, options);
  };
  return { controller, signal, listeners };
}

function makeRuntime({ iframe = false, ipc = true, throwTransport = false, existing } = {}) {
  const outbound = [];
  const errors = [];
  const timers = new Map();
  const handlers = new Map();
  const dispatched = [];
  let timerId = 0;
  const window = {
    addEventListener(name, callback, options = {}) {
      if (!handlers.has(name)) handlers.set(name, []);
      handlers.get(name).push({ callback, once: Boolean(options.once) });
    },
    dispatchEvent(event) {
      dispatched.push(event.type);
      const records = handlers.get(event.type) || [];
      for (const entry of [...records]) {
        if (entry.once) records.splice(records.indexOf(entry), 1);
        entry.callback(event);
      }
      return true;
    },
  };
  window.top = iframe ? {} : window;
  if (existing !== undefined) window.lumen = existing;
  if (ipc) {
    window.ipc = {
      postMessage(message) {
        assert.equal(this, window.ipc, 'the transport retains its receiver');
        if (throwTransport) throw new Error('mock transport unavailable');
        outbound.push(JSON.parse(message));
      },
    };
  }
  const context = vm.createContext({
    window,
    console: { error(error) { errors.push(error); } },
    CustomEvent: class CustomEvent { constructor(type) { this.type = type; } },
    setTimeout(callback, delay) { const id = ++timerId; timers.set(id, { callback, delay }); return id; },
    clearTimeout(id) { timers.delete(id); },
  });
  const script = source.replace('__SESSION__', JSON.stringify(SESSION)).replace('__RUNTIME__', JSON.stringify(RUNTIME));
  const install = () => vm.runInContext(script, context, { filename: 'lumendesk/runtime.js' });
  install();
  return {
    window, outbound, errors, timers, dispatched, install,
    request(index = outbound.length - 1) { return outbound[index].request; },
    respond(id, result) { window.__lumenResponse({ id, result }); },
    emit(name, payload) { window.__lumenEvent(name, payload); },
    pagehide() { window.dispatchEvent({ type: 'pagehide' }); },
    fireTimer(id = timers.keys().next().value) {
      const entry = timers.get(id);
      assert.ok(entry, 'mock timer exists');
      timers.delete(id);
      entry.callback();
    },
  };
}

test('installs an immutable runtime and announces readiness exactly once', () => {
  const r = makeRuntime();
  assert.equal(r.window.lumen.runtime.platform, RUNTIME.platform);
  assert.equal(r.window.lumen.runtime.windowId, RUNTIME.windowId);
  assert.ok(Object.isFrozen(r.window.lumen));
  assert.ok(Object.isFrozen(r.window.lumen.runtime));
  for (const property of ['lumen', '__lumenResponse', '__lumenEvent']) {
    const descriptor = Object.getOwnPropertyDescriptor(r.window, property);
    assert.equal(descriptor.writable, false);
    assert.equal(descriptor.configurable, false);
  }
  r.install();
  assert.deepEqual(r.dispatched, ['lumendesk-ready']);
  assert.equal(r.window.lumen.session, undefined);
});

test('ordinary browser without the native IPC transport receives no runtime', () => {
  const r = makeRuntime({ ipc: false });
  assert.equal(r.window.lumen, undefined);
  assert.equal(r.window.__lumenResponse, undefined);
  assert.equal(r.window.__lumenEvent, undefined);
  assert.deepEqual(r.dispatched, []);
  assert.equal(r.timers.size, 0);
});

test('a mocked embedded frame cannot install the runtime even with IPC present', () => {
  const r = makeRuntime({ iframe: true });
  assert.equal(r.window.lumen, undefined);
  assert.equal(r.window.__lumenResponse, undefined);
  assert.equal(r.window.__lumenEvent, undefined);
  assert.deepEqual(r.outbound, []);
  assert.deepEqual(r.dispatched, []);
});

test('an existing runtime is neither replaced nor re-announced', () => {
  const existing = { marker: 'already installed' };
  const r = makeRuntime({ existing });
  assert.equal(r.window.lumen, existing);
  assert.deepEqual(r.dispatched, []);
});

test('round-trips Unicode and nested JSON with session-scoped increasing IDs', async () => {
  const r = makeRuntime();
  const args = { name: '星雲 🌌 café e\u0301', nested: { items: [null, false, 0, ''], number: 1.25 } };
  const first = r.window.lumen.call('echo', args);
  assert.deepEqual(r.outbound[0], { session: SESSION, request: { id: 1, method: 'echo', args } });
  const result = { ...args, message: '您好，Orion' };
  r.respond(1, { Ok: result });
  assert.deepEqual(await first, result);
  const second = r.window.lumen.call('echo', null);
  assert.equal(r.request().id, 2);
  r.respond(2, { Ok: null });
  assert.equal(await second, null);
  assert.equal(r.timers.size, 0);
});

test('successful falsy result values are distinguished from backend errors', async () => {
  const r = makeRuntime();
  for (const value of [false, 0, '', null]) {
    const promise = r.window.lumen.call('echo', value);
    r.respond(r.request().id, { Ok: value });
    assert.equal(await promise, value);
  }
});

test('backend failures preserve error messages and error codes', async () => {
  const r = makeRuntime();
  for (const [code, message] of [
    ['invalid_arguments', 'name must be a string'],
    ['unknown_method', 'does_not_exist'],
    ['backend', 'service failed'],
  ]) {
    const promise = r.window.lumen.call('test', {});
    const rejection = assert.rejects(promise, error => error.message === message && error.code === code);
    r.respond(r.request().id, { Err: { code, message } });
    await rejection;
    assert.equal(r.timers.size, 0);
  }
});

test('a code-only backend failure still rejects with a useful message', async () => {
  const r = makeRuntime();
  const promise = r.window.lumen.call('test', null);
  const rejection = assert.rejects(promise, error => error.message === 'cancelled' && error.code === 'cancelled');
  r.respond(r.request().id, { Err: { code: 'cancelled' } });
  await rejection;
});

for (const [label, result] of [
  ['null result', null],
  ['missing result', undefined],
  ['string result', 'invalid'],
  ['number result', 42],
  ['boolean result', false],
  ['array result', []],
  ['empty result object', {}],
  ['unknown result tag', { Other: null }],
  ['both result tags', { Ok: null, Err: { code: 'backend' } }],
  ['extra result field', { Ok: null, extra: true }],
  ['undefined success', { Ok: undefined }],
  ['null error details', { Err: null }],
  ['string error details', { Err: 'failed' }],
  ['array error details', { Err: [] }],
  ['missing error code', { Err: { message: 'failed' } }],
  ['nonstring error code', { Err: { code: 42 } }],
  ['empty error code', { Err: { code: '' } }],
  ['nonstring error message', { Err: { code: 'backend', message: {} } }],
  ['extra error field', { Err: { code: 'backend', unexpected: true } }],
]) {
  test(`malformed matching reply rejects explicitly and cleans up: ${label}`, async () => {
    const r = makeRuntime();
    const promise = r.window.lumen.call('echo', null);
    const rejection = assert.rejects(promise, error =>
      error.message === 'Malformed backend response' && error.code === 'malformed_response');
    const response = result === undefined ? { id: 1 } : { id: 1, result };
    assert.doesNotThrow(() => r.window.__lumenResponse(response));
    await rejection;
    assert.equal(r.timers.size, 0);
    r.respond(1, { Ok: 'late response ignored' });
    const next = r.window.lumen.call('echo', null);
    r.respond(2, { Ok: 'capacity reusable' });
    assert.equal(await next, 'capacity reusable');
    assert.equal(r.timers.size, 0);
  });
}

test('unidentifiable malformed response envelopes leave valid calls and timers intact', async () => {
  const r = makeRuntime();
  const promise = r.window.lumen.call('echo', null);
  for (const response of [null, undefined, false, 42, 'bad', [], {}, { id: '1', result: null }]) {
    assert.doesNotThrow(() => r.window.__lumenResponse(response));
    assert.equal(r.timers.size, 1);
  }
  r.respond(1, { Ok: 'still pending until a matching reply' });
  assert.equal(await promise, 'still pending until a matching reply');
  assert.equal(r.timers.size, 0);
});

test('a throwing matching result accessor cannot strand its promise', async () => {
  const r = makeRuntime();
  const promise = r.window.lumen.call('echo', null);
  const rejection = assert.rejects(promise, /Malformed backend response/);
  assert.doesNotThrow(() => r.window.__lumenResponse({
    id: 1,
    get result() { throw new Error('invalid accessor'); },
  }));
  await rejection;
  assert.equal(r.timers.size, 0);
});

test('concurrent responses resolve only the matching calls even out of order', async () => {
  const r = makeRuntime();
  const first = r.window.lumen.call('echo', 'first');
  const second = r.window.lumen.call('echo', 'second');
  r.respond(2, { Ok: 'second-result' });
  assert.equal(await second, 'second-result');
  assert.equal(r.timers.size, 1);
  r.respond(1, { Ok: 'first-result' });
  assert.equal(await first, 'first-result');
});

test('unknown or duplicate response IDs cannot settle a different call', async () => {
  const r = makeRuntime();
  const promise = r.window.lumen.call('echo', null);
  r.respond(100, { Ok: 'wrong' });
  assert.equal(r.timers.size, 1);
  r.respond(1, { Ok: 'right' });
  r.respond(1, { Ok: 'duplicate' });
  assert.equal(await promise, 'right');
  assert.equal(r.timers.size, 0);
});

test('invalid method names reject before allocating timers or invoking IPC', async () => {
  const r = makeRuntime();
  for (const method of ['', null, undefined, 42, {}, []]) {
    await assert.rejects(r.window.lumen.call(method, {}), /Invalid method/);
  }
  assert.equal(r.timers.size, 0);
  assert.deepEqual(r.outbound, []);
});

test('transport failure rejects immediately and releases timer and pending capacity', async () => {
  const r = makeRuntime({ throwTransport: true });
  for (let index = 0; index < 40; index++) {
    await assert.rejects(r.window.lumen.call('echo', null), /mock transport unavailable/);
  }
  assert.equal(r.timers.size, 0);
  assert.deepEqual(r.outbound, []);
});

test('non-serializable circular arguments reject without leaking pending calls', async () => {
  const r = makeRuntime();
  const circular = {};
  circular.self = circular;
  for (let index = 0; index < 40; index++) {
    await assert.rejects(r.window.lumen.call('echo', circular), /circular/i);
  }
  assert.equal(r.timers.size, 0);
  assert.deepEqual(r.outbound, []);
});

test('pending call count is bounded at 32 and freed capacity is reusable', async () => {
  const r = makeRuntime();
  const promises = Array.from({ length: 32 }, (_, n) => r.window.lumen.call('echo', n));
  await assert.rejects(r.window.lumen.call('echo', 'overflow'), /Too many pending calls/);
  assert.equal(r.outbound.length, 32);
  assert.equal(r.timers.size, 32);
  r.respond(1, { Ok: 'first done' });
  assert.equal(await promises[0], 'first done');
  const replacement = r.window.lumen.call('echo', 'replacement');
  assert.equal(r.request().id, 33);
  for (let id = 2; id <= 33; id++) r.respond(id, { Ok: id });
  await Promise.all([...promises, replacement]);
  assert.equal(r.timers.size, 0);
});

test('timeout rejects, sends a session-scoped cancellation, and frees capacity', async () => {
  const r = makeRuntime();
  const promise = r.window.lumen.call('slow', null, { timeoutMs: 17 });
  const rejection = assert.rejects(promise, /Call timed out/);
  assert.equal([...r.timers.values()][0].delay, 17);
  r.fireTimer();
  await rejection;
  assert.deepEqual(r.outbound[1], { session: SESSION, cancel: 1 });
  assert.equal(r.timers.size, 0);
  r.respond(1, { Ok: 'late response ignored' });
  const next = r.window.lumen.call('echo', null);
  r.respond(2, { Ok: 'still works' });
  assert.equal(await next, 'still works');
});

test('generated-client AbortSignal argument cancels a pending call exactly once', async () => {
  const r = makeRuntime();
  const { controller, signal, listeners } = trackedAbortController();
  const promise = r.window.lumen.call('slow', null, signal);
  const rejection = assert.rejects(promise, error => error.name === 'AbortError' && error.code === 'cancelled');
  assert.equal(listeners.size, 1);
  controller.abort();
  await rejection;
  assert.equal(listeners.size, 0);
  assert.equal(r.timers.size, 0);
  assert.deepEqual(r.outbound[1], { session: SESSION, cancel: 1 });
  controller.abort();
  r.respond(1, { Ok: 'late result ignored' });
  assert.equal(r.outbound.length, 2);
  const next = r.window.lumen.call('echo', null);
  r.respond(2, { Ok: 'capacity reusable' });
  assert.equal(await next, 'capacity reusable');
});

for (const wrapped of [false, true]) {
  test(`already-aborted ${wrapped ? 'options.signal' : 'direct signal'} rejects without IPC or timers`, async () => {
    const r = makeRuntime();
    const { controller, signal, listeners } = trackedAbortController();
    controller.abort();
    await assert.rejects(r.window.lumen.call('echo', null, wrapped ? { signal, timeoutMs: 9 } : signal),
      error => error.name === 'AbortError' && error.code === 'cancelled');
    assert.equal(listeners.size, 0);
    assert.equal(r.timers.size, 0);
    assert.deepEqual(r.outbound, []);
  });
}

test('timeout options can include AbortSignal and timeout removes its listener', async () => {
  const r = makeRuntime();
  const { controller, signal, listeners } = trackedAbortController();
  const promise = r.window.lumen.call('slow', null, { timeoutMs: 23, signal });
  const rejection = assert.rejects(promise, /Call timed out/);
  assert.equal([...r.timers.values()][0].delay, 23);
  assert.equal(listeners.size, 1);
  r.fireTimer();
  await rejection;
  assert.equal(listeners.size, 0);
  assert.equal(r.timers.size, 0);
  controller.abort();
  assert.deepEqual(r.outbound[1], { session: SESSION, cancel: 1 });
  assert.equal(r.outbound.length, 2);
});

for (const [label, response] of [
  ['success', { Ok: 'done' }],
  ['backend failure', { Err: { code: 'backend', message: 'failed' } }],
  ['malformed reply', null],
]) {
  test(`response cleanup removes the abort listener: ${label}`, async () => {
    const r = makeRuntime();
    const { controller, signal, listeners } = trackedAbortController();
    const promise = r.window.lumen.call('echo', null, signal);
    const settled = response?.Ok ? promise : assert.rejects(promise);
    assert.equal(listeners.size, 1);
    r.respond(1, response);
    await settled;
    assert.equal(listeners.size, 0);
    assert.equal(r.timers.size, 0);
    controller.abort();
    assert.equal(r.outbound.length, 1);
  });
}

test('pagehide removes abort listeners while rejecting pending calls', async () => {
  const r = makeRuntime();
  const { controller, signal, listeners } = trackedAbortController();
  const promise = r.window.lumen.call('echo', null, signal);
  const rejection = assert.rejects(promise, /Page session ended/);
  r.pagehide();
  await rejection;
  assert.equal(listeners.size, 0);
  assert.equal(r.timers.size, 0);
  controller.abort();
  assert.equal(r.outbound.length, 1);
});

test('transport failure removes the attached abort listener', async () => {
  const r = makeRuntime({ throwTransport: true });
  const { signal, listeners } = trackedAbortController();
  await assert.rejects(r.window.lumen.call('echo', null, signal), /mock transport unavailable/);
  assert.equal(listeners.size, 0);
  assert.equal(r.timers.size, 0);
});

test('abort during argument serialization prevents sending the request', async () => {
  const r = makeRuntime();
  const { controller, signal, listeners } = trackedAbortController();
  const args = { toJSON() { controller.abort(); return 'not sent'; } };
  await assert.rejects(r.window.lumen.call('echo', args, signal), error => error.name === 'AbortError');
  assert.equal(listeners.size, 0);
  assert.equal(r.timers.size, 0);
  assert.deepEqual(r.outbound, []);
});

test('invalid options.signal rejects without sending or leaking timers', async () => {
  const r = makeRuntime();
  await assert.rejects(r.window.lumen.call('echo', null, { signal: {} }), /Invalid abort signal/);
  assert.equal(r.timers.size, 0);
  assert.deepEqual(r.outbound, []);
});

test('successful completion removes the timeout without sending cancellation', async () => {
  const r = makeRuntime();
  const promise = r.window.lumen.call('echo', null, { timeoutMs: 10 });
  r.respond(1, { Ok: null });
  await promise;
  assert.equal(r.timers.size, 0);
  assert.equal(r.outbound.length, 1);
});

test('timeout duration has a default and clamps positive extremes', async () => {
  const r = makeRuntime();
  for (const [options, expected] of [[{}, 30000], [{ timeoutMs: -5 }, 1], [{ timeoutMs: 500000 }, 300000]]) {
    const promise = r.window.lumen.call('echo', null, options);
    assert.equal([...r.timers.values()][0].delay, expected);
    r.respond(r.request().id, { Ok: null });
    await promise;
  }
});

test('pagehide rejects all pending calls and permanently rejects new calls', async () => {
  const r = makeRuntime();
  const promises = [r.window.lumen.call('echo', 1), r.window.lumen.call('echo', 2)];
  const rejections = promises.map(promise => assert.rejects(promise, /Page session ended/));
  r.pagehide();
  await Promise.all(rejections);
  assert.equal(r.timers.size, 0);
  await assert.rejects(r.window.lumen.call('echo', 3), /Page session ended/);
  assert.equal(r.outbound.length, 2);
  r.respond(1, { Ok: 'ignored after pagehide' });
  r.pagehide();
  assert.equal(r.timers.size, 0);
});

test('events preserve Unicode/nested payloads and do not cross event names', () => {
  const r = makeRuntime();
  const received = [];
  const payload = { title: '星雲 🌠', values: [null, { checked: true }] };
  r.window.lumen.on('greeting', value => received.push(value));
  r.emit('different', 'ignored');
  r.emit('greeting', payload);
  assert.deepEqual(received, [payload]);
});

test('once listeners are removed before their callback, including recursive emission', () => {
  const r = makeRuntime();
  let once = 0;
  let regular = 0;
  r.window.lumen.on('greeting', () => { once++; r.emit('greeting', 'recursive'); }, { once: true });
  r.window.lumen.on('greeting', () => { regular++; });
  r.emit('greeting', 'first');
  r.emit('greeting', 'second');
  assert.equal(once, 1);
  assert.equal(regular, 3);
});

test('unsubscribe is idempotent and prevents all later event deliveries', () => {
  const r = makeRuntime();
  let count = 0;
  const unsubscribe = r.window.lumen.on('greeting', () => { count++; });
  r.emit('greeting', null);
  unsubscribe();
  unsubscribe();
  r.emit('greeting', null);
  assert.equal(count, 1);
});

test('one event callback throwing does not block remaining listeners', () => {
  const r = makeRuntime();
  let reached = false;
  r.window.lumen.on('greeting', () => { throw new Error('listener failed'); });
  r.window.lumen.on('greeting', () => { reached = true; });
  r.emit('greeting', null);
  assert.equal(reached, true);
  assert.equal(r.errors.length, 1);
  assert.match(r.errors[0].message, /listener failed/);
});

test('invalid event listeners are rejected synchronously', () => {
  const r = makeRuntime();
  for (const [name, callback] of [[null, () => {}], [1, () => {}], ['greeting', null], ['greeting', 'bad']]) {
    assert.throws(() => r.window.lumen.on(name, callback), /Invalid event listener/);
  }
});

test('pagehide clears previously registered event listeners', () => {
  const r = makeRuntime();
  let count = 0;
  r.window.lumen.on('greeting', () => { count++; });
  r.pagehide();
  r.emit('greeting', null);
  assert.equal(count, 0);
});
