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

function makeRuntime({ iframe = false, ipc = true, throwTransport = false, throwTransportAt, existing } = {}) {
  const outbound = [];
  const errors = [];
  const timers = new Map();
  const handlers = new Map();
  const dispatched = [];
  let timerId = 0;
  let transportCalls = 0;
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
        transportCalls++;
        if (throwTransport || transportCalls === throwTransportAt) throw new Error('mock transport unavailable');
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
    streamFrame(frame) { window.__lumenStream(frame); },
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
  for (const property of ['lumen', '__lumenResponse', '__lumenEvent', '__lumenStream']) {
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
  assert.equal(r.window.__lumenStream, undefined);
  assert.deepEqual(r.dispatched, []);
  assert.equal(r.timers.size, 0);
});

test('a mocked embedded frame cannot install the runtime even with IPC present', () => {
  const r = makeRuntime({ iframe: true });
  assert.equal(r.window.lumen, undefined);
  assert.equal(r.window.__lumenResponse, undefined);
  assert.equal(r.window.__lumenEvent, undefined);
  assert.equal(r.window.__lumenStream, undefined);
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

// Streaming cases below use the same mock VM. They do not exercise a webview,
// native host scheduling, or a graphical desktop.
const normalized = value => JSON.parse(JSON.stringify(value));
const pullFrames = runtime => runtime.outbound.filter(frame => 'pull' in frame);
const cancelFrames = runtime => runtime.outbound.filter(frame => 'cancel' in frame);
const isCancelled = error => error.name === 'AbortError' && error.code === 'cancelled';
const isMalformedStream = error => error.code === 'malformed_response' && /Malformed backend stream/.test(error.message);

function completeStream(runtime, id, value) {
  runtime.streamFrame({ id, done: true, result: { Ok: value } });
}

test('stream registration is independent and IDs are shared monotonically with calls', async () => {
  const r = makeRuntime();
  const call = r.window.lumen.call('echo', 'before');
  const args = { title: '星雲 🌌', items: [null, false, 0] };
  const stream = r.window.lumen.stream('numbers', args);
  const after = r.window.lumen.call('echo', 'after');
  assert.deepEqual(r.outbound, [
    { session: SESSION, request: { id: 1, method: 'echo', args: 'before' } },
    { session: SESSION, stream: { id: 2, method: 'numbers', args } },
    { session: SESSION, request: { id: 3, method: 'echo', args: 'after' } },
  ]);
  assert.equal(stream[Symbol.asyncIterator](), stream);
  const property = Object.getOwnPropertyDescriptor(stream, 'result');
  assert.equal(property.writable, false);
  assert.equal(property.configurable, false);
  const next = stream.next();
  completeStream(r, 2, 'finished');
  assert.deepEqual(normalized(await next), { done: true, value: 'finished' });
  assert.equal(await stream.result, 'finished');
  r.respond(1, { Ok: 1 }); r.respond(3, { Ok: 3 });
  await Promise.all([call, after]);
  assert.equal(r.timers.size, 0);
});

test('slow consumers grant exactly one pull for each next with no eager item buffering', async () => {
  const r = makeRuntime();
  const stream = r.window.lumen.stream('numbers', null);
  let complete = false;
  stream.result.then(() => { complete = true; });
  assert.equal(pullFrames(r).length, 0);
  assert.equal(r.timers.size, 1, 'unstarted streams have an idle timer');
  for (let value = 0; value < 200; value++) {
    const next = stream.next();
    assert.deepEqual(pullFrames(r).at(-1), { session: SESSION, pull: 1 });
    assert.equal(pullFrames(r).length, value + 1);
    r.streamFrame({ id: 1, item: value });
    assert.deepEqual(normalized(await next), { done: false, value });
    await Promise.resolve();
    assert.equal(pullFrames(r).length, value + 1, 'consumer pause grants no extra credit');
    assert.equal(r.timers.size, 1, 'one bounded idle timer replaces the completed read timer');
    assert.equal(complete, false);
  }
  const final = stream.next();
  completeStream(r, 1, { count: 200 });
  assert.deepEqual(normalized(await final), { done: true, value: { count: 200 } });
  assert.deepEqual(await stream.result, { count: 200 });
  assert.equal(complete, true);
  assert.equal(r.timers.size, 0);
  assert.equal(cancelFrames(r).length, 0);
  const pullsBefore = pullFrames(r).length;
  assert.equal((await stream.next()).done, true);
  assert.equal(pullFrames(r).length, pullsBefore, 'completed reads do not contact the host');
});

test('for-await delivers ordered Unicode and falsy items before terminal completion', async () => {
  const r = makeRuntime();
  const stream = r.window.lumen.stream('values', null);
  const values = [null, false, 0, '', '星雲 🌠', { nested: [1, 2] }];
  const received = [];
  const consume = (async () => {
    for await (const item of stream) received.push(item);
  })();
  for (const value of values) {
    r.streamFrame({ id: 1, item: value });
    await new Promise(resolve => setImmediate(resolve));
  }
  completeStream(r, 1, 'all delivered');
  await consume;
  assert.deepEqual(received, values);
  assert.equal(await stream.result, 'all delivered');
  assert.equal(pullFrames(r).length, values.length + 1);
  assert.equal(r.timers.size, 0);
});

test('terminal immediately after the last item is retained until the following next', async () => {
  const r = makeRuntime();
  const stream = r.window.lumen.stream('one', null);
  const item = stream.next();
  r.streamFrame({ id: 1, item: 'last' });
  completeStream(r, 1, 'final result');
  assert.deepEqual(normalized(await item), { done: false, value: 'last' });
  assert.equal(await stream.result, 'final result');
  assert.deepEqual(normalized(await stream.next()), { done: true, value: 'final result' });
  assert.equal((await stream.next()).value, undefined);
  assert.equal(pullFrames(r).length, 1);
  assert.equal(r.timers.size, 0);
});

test('a concurrent next rejects explicitly without stealing the first read or sending credit', async () => {
  const r = makeRuntime();
  const stream = r.window.lumen.stream('numbers', null);
  const first = stream.next();
  await assert.rejects(stream.next(), error => error.code === 'concurrent_read');
  assert.equal(pullFrames(r).length, 1);
  r.streamFrame({ id: 1, item: 123 });
  assert.deepEqual(normalized(await first), { done: false, value: 123 });
  const final = stream.next();
  completeStream(r, 1, null);
  assert.equal((await final).done, true);
  assert.equal(await stream.result, null);
});

test('multiple streams and a call remain isolated across out-of-order replies', async () => {
  const r = makeRuntime();
  const left = r.window.lumen.stream('left', null);
  const right = r.window.lumen.stream('right', null);
  const call = r.window.lumen.call('echo', null);
  const l1 = left.next(); const r1 = right.next();
  r.streamFrame({ id: 3, item: 'cannot settle a call' });
  r.respond(1, { Ok: 'cannot settle a stream' });
  r.streamFrame({ id: 2, item: 'right first' });
  r.respond(3, { Ok: 'call result' });
  r.streamFrame({ id: 1, item: 'left first' });
  assert.equal((await l1).value, 'left first');
  assert.equal((await r1).value, 'right first');
  assert.equal(await call, 'call result');
  const lf = left.next(); const rf = right.next();
  completeStream(r, 2, 'right done'); completeStream(r, 1, 'left done');
  assert.equal((await lf).value, 'left done');
  assert.equal((await rf).value, 'right done');
  assert.equal(await left.result, 'left done');
  assert.equal(await right.result, 'right done');
  assert.equal(r.timers.size, 0);
});

for (const pending of [false, true]) {
  test(`return cancels exactly once, cleans listeners, and closes ${pending ? 'a pending' : 'an idle'} read`, async () => {
    const r = makeRuntime();
    const { controller, signal, listeners } = trackedAbortController();
    const stream = r.window.lumen.stream('numbers', null, signal);
    const read = pending ? stream.next() : undefined;
    const result = assert.rejects(stream.result, isCancelled);
    const returned = await stream.return('consumer result');
    assert.deepEqual(normalized(returned), { done: true, value: 'consumer result' });
    if (read) assert.equal((await read).done, true);
    await result;
    assert.deepEqual(cancelFrames(r), [{ session: SESSION, cancel: 1 }]);
    assert.equal(listeners.size, 0);
    assert.equal(r.timers.size, 0);
    await stream.return(); controller.abort();
    r.streamFrame({ id: 1, item: 'late' }); completeStream(r, 1, 'late');
    assert.equal((await stream.next()).done, true);
    assert.equal(cancelFrames(r).length, 1);
  });
}

test('breaking for-await closes the host stream and rejects the final-result promise', async () => {
  const r = makeRuntime();
  const stream = r.window.lumen.stream('numbers', null);
  const result = assert.rejects(stream.result, isCancelled);
  const consume = (async () => { for await (const value of stream) { assert.equal(value, 5); break; } })();
  r.streamFrame({ id: 1, item: 5 });
  await consume; await result;
  assert.deepEqual(cancelFrames(r), [{ session: SESSION, cancel: 1 }]);
  assert.equal(r.timers.size, 0);
});

test('throw rejects a pending reader and result, cancels once, and returns its rejection', async () => {
  const r = makeRuntime();
  const { signal, listeners } = trackedAbortController();
  const stream = r.window.lumen.stream('numbers', null, signal);
  const failure = new Error('consumer failed');
  const read = assert.rejects(stream.next(), error => error === failure);
  const result = assert.rejects(stream.result, error => error === failure);
  await assert.rejects(stream.throw(failure), error => error === failure);
  await Promise.all([read, result]);
  assert.equal((await stream.next()).done, true);
  assert.equal(cancelFrames(r).length, 1);
  assert.equal(listeners.size, 0);
  assert.equal(r.timers.size, 0);
});

for (const wrapped of [false, true]) {
  test(`already-aborted stream ${wrapped ? 'options' : 'direct signal'} sends no IPC and allocates no resources`, async () => {
    const r = makeRuntime();
    const { controller, signal, listeners } = trackedAbortController();
    controller.abort();
    const stream = r.window.lumen.stream('numbers', null, wrapped ? { signal, timeoutMs: 8 } : signal);
    await assert.rejects(stream.next(), isCancelled);
    await assert.rejects(stream.result, isCancelled);
    assert.equal((await stream.next()).done, true);
    assert.equal(listeners.size, 0);
    assert.equal(r.timers.size, 0);
    assert.deepEqual(r.outbound, []);
  });
}

for (const pending of [false, true]) {
  test(`abort closes ${pending ? 'a pending' : 'an idle'} stream and ignores later items`, async () => {
    const r = makeRuntime();
    const { controller, signal, listeners } = trackedAbortController();
    const stream = r.window.lumen.stream('numbers', null, { signal, timeoutMs: 11 });
    const next = pending ? assert.rejects(stream.next(), isCancelled) : undefined;
    const result = assert.rejects(stream.result, isCancelled);
    controller.abort();
    if (next) await next; else await assert.rejects(stream.next(), isCancelled);
    await result;
    r.streamFrame({ id: 1, item: 'late' });
    assert.equal((await stream.next()).done, true);
    assert.equal(listeners.size, 0);
    assert.equal(r.timers.size, 0);
    assert.deepEqual(cancelFrames(r), [{ session: SESSION, cancel: 1 }]);
  });
}

test('aborting while serializing stream arguments sends nothing and leaves no listener or timer', async () => {
  const r = makeRuntime();
  const { controller, signal, listeners } = trackedAbortController();
  const stream = r.window.lumen.stream('numbers', { toJSON() { controller.abort(); return null; } }, signal);
  await assert.rejects(stream.next(), isCancelled);
  await assert.rejects(stream.result, isCancelled);
  assert.deepEqual(r.outbound, []);
  assert.equal(listeners.size, 0);
  assert.equal(r.timers.size, 0);
});

test('pagehide rejects pending and idle streams, cancels both, and bars new streams', async () => {
  const r = makeRuntime();
  const { controller, signal, listeners } = trackedAbortController();
  const pending = r.window.lumen.stream('numbers', null, signal);
  const idle = r.window.lumen.stream('numbers', null, signal);
  const read = assert.rejects(pending.next(), /Page session ended/);
  const results = [pending, idle].map(stream => assert.rejects(stream.result, /Page session ended/));
  r.pagehide();
  await read; await Promise.all(results);
  await assert.rejects(idle.next(), /Page session ended/);
  assert.deepEqual(cancelFrames(r), [{ session: SESSION, cancel: 1 }, { session: SESSION, cancel: 2 }]);
  assert.equal(listeners.size, 0);
  assert.equal(r.timers.size, 0);
  const created = r.window.lumen.stream('numbers', null);
  await assert.rejects(created.next(), /Page session ended/);
  await assert.rejects(created.result, /Page session ended/);
  controller.abort(); r.pagehide();
  assert.equal(r.outbound.length, 5);
});

for (const stage of ['unstarted', 'pending read', 'between reads']) {
  test(`stream timeout bounds ${stage} lifetime and releases every resource`, async () => {
    const r = makeRuntime();
    const { signal, listeners } = trackedAbortController();
    const stream = r.window.lumen.stream('numbers', null, { signal, timeoutMs: 27 });
    let read;
    if (stage === 'pending read') read = assert.rejects(stream.next(), /Stream timed out/);
    if (stage === 'between reads') {
      const item = stream.next(); r.streamFrame({ id: 1, item: 42 }); await item;
    }
    const result = assert.rejects(stream.result, error => error.code === 'timeout');
    assert.equal(r.timers.size, 1);
    assert.equal([...r.timers.values()][0].delay, 27);
    r.fireTimer();
    if (read) await read; else await assert.rejects(stream.next(), /Stream timed out/);
    await result;
    assert.equal(r.timers.size, 0);
    assert.equal(listeners.size, 0);
    assert.deepEqual(cancelFrames(r), [{ session: SESSION, cancel: 1 }]);
  });
}

test('each next and item renews one stream timer rather than retaining a whole-stream deadline', async () => {
  const r = makeRuntime();
  const stream = r.window.lumen.stream('numbers', null);
  const initialTimer = [...r.timers.keys()][0];
  const first = stream.next();
  const pendingTimer = [...r.timers.keys()][0];
  assert.notEqual(pendingTimer, initialTimer);
  assert.equal(r.timers.size, 1);
  r.streamFrame({ id: 1, item: 1 }); await first;
  const idleTimer = [...r.timers.keys()][0];
  assert.notEqual(idleTimer, pendingTimer);
  assert.equal([...r.timers.values()][0].delay, 30000);
  const final = stream.next();
  assert.notEqual([...r.timers.keys()][0], idleTimer);
  completeStream(r, 1, null); await final;
  assert.equal(r.timers.size, 0);
});

test('stream timeout clamps extremes and uses a finite default', async () => {
  const r = makeRuntime();
  for (const [timeoutMs, expected] of [[undefined, 30000], [-5, 1], [900000, 300000], [Infinity, 30000], [NaN, 30000]]) {
    const stream = r.window.lumen.stream('numbers', null, { timeoutMs });
    assert.equal([...r.timers.values()][0].delay, expected);
    await stream.return();
  }
  assert.equal(r.timers.size, 0);
});

test('live streams are limited to 16 and cancelled capacity is reusable', async () => {
  const r = makeRuntime();
  const streams = Array.from({ length: 16 }, () => r.window.lumen.stream('numbers', null));
  const overflow = r.window.lumen.stream('numbers', null);
  await assert.rejects(overflow.next(), /Too many live streams/);
  await assert.rejects(overflow.result, /Too many live streams/);
  assert.equal(r.outbound.length, 16);
  assert.equal(r.timers.size, 16);
  await streams[0].return();
  const replacement = r.window.lumen.stream('numbers', null);
  assert.equal(r.outbound.at(-1).stream.id, 17);
  await Promise.all([...streams.slice(1), replacement].map(stream => stream.return()));
  assert.equal(r.timers.size, 0);
});

test('combined call and stream capacity is limited to 32 in either registration order', async () => {
  const r = makeRuntime();
  const calls = Array.from({ length: 24 }, () => r.window.lumen.call('echo', null));
  const streams = Array.from({ length: 8 }, () => r.window.lumen.stream('numbers', null));
  await assert.rejects(r.window.lumen.call('echo', null), /Too many pending calls/);
  const overflow = r.window.lumen.stream('numbers', null);
  await assert.rejects(overflow.next(), /Too many pending calls/);
  assert.equal(r.outbound.length, 32);
  assert.equal(r.timers.size, 32);
  r.respond(1, { Ok: null }); await calls[0];
  const replacement = r.window.lumen.stream('numbers', null);
  assert.equal(r.outbound.at(-1).stream.id, 33);
  await streams[0].return();
  const newCall = r.window.lumen.call('echo', null);
  assert.equal(r.request().id, 34);
  for (let id = 2; id <= 24; id++) r.respond(id, { Ok: null });
  r.respond(34, { Ok: null });
  await Promise.all([...calls, newCall]);
  await Promise.all([...streams, replacement].map(stream => stream.return()));
  assert.equal(r.timers.size, 0);
});

test('completed unconsumed terminal state does not occupy a live stream slot', async () => {
  const r = makeRuntime();
  const completed = [];
  for (let index = 0; index < 40; index++) {
    const stream = r.window.lumen.stream('numbers', null);
    completeStream(r, index + 1, index);
    completed.push(stream);
  }
  assert.equal(r.timers.size, 0);
  for (let index = 0; index < completed.length; index++) {
    assert.equal((await completed[index].next()).value, index);
    assert.equal(await completed[index].result, index);
  }
});

for (const pending of [false, true]) {
  test(`terminal backend errors reject result and ${pending ? 'pending' : 'future'} next with their code`, async () => {
    const r = makeRuntime();
    const { signal, listeners } = trackedAbortController();
    const stream = r.window.lumen.stream('unknown', null, signal);
    const predicate = error => error.code === 'unknown_method' && error.message === 'No such stream';
    const read = pending ? assert.rejects(stream.next(), predicate) : undefined;
    const result = assert.rejects(stream.result, predicate);
    r.streamFrame({ id: 1, done: true, result: { Err: { code: 'unknown_method', message: 'No such stream' } } });
    if (read) await read; else await assert.rejects(stream.next(), predicate);
    await result;
    assert.equal((await stream.next()).done, true);
    assert.equal(listeners.size, 0);
    assert.equal(r.timers.size, 0);
    assert.equal(cancelFrames(r).length, 0);
  });
}

test('code-only terminal errors retain a useful message', async () => {
  const r = makeRuntime();
  const stream = r.window.lumen.stream('numbers', null);
  r.streamFrame({ id: 1, done: true, result: { Err: { code: 'backend' } } });
  await assert.rejects(stream.result, error => error.code === 'backend' && error.message === 'backend');
  await assert.rejects(stream.next(), /backend/);
});

for (const [label, frame] of [
  ['missing body', { id: 1 }],
  ['undefined item', { id: 1, item: undefined }],
  ['item and terminal', { id: 1, item: 1, done: true, result: { Ok: null } }],
  ['item extra field', { id: 1, item: 1, unexpected: true }],
  ['false done', { id: 1, done: false, result: { Ok: null } }],
  ['missing done', { id: 1, result: { Ok: null } }],
  ['missing result', { id: 1, done: true }],
  ['extra terminal field', { id: 1, done: true, result: { Ok: null }, unexpected: true }],
  ['null result', { id: 1, done: true, result: null }],
  ['empty result', { id: 1, done: true, result: {} }],
  ['array result', { id: 1, done: true, result: [] }],
  ['unknown result', { id: 1, done: true, result: { Other: null } }],
  ['both result tags', { id: 1, done: true, result: { Ok: null, Err: { code: 'backend' } } }],
  ['undefined success', { id: 1, done: true, result: { Ok: undefined } }],
  ['null error', { id: 1, done: true, result: { Err: null } }],
  ['missing code', { id: 1, done: true, result: { Err: {} } }],
  ['empty code', { id: 1, done: true, result: { Err: { code: '' } } }],
  ['nonstring message', { id: 1, done: true, result: { Err: { code: 'backend', message: 42 } } }],
  ['extra error field', { id: 1, done: true, result: { Err: { code: 'backend', unexpected: true } } }],
]) {
  test(`malformed matching stream frame rejects and cancels without leaking: ${label}`, async () => {
    const r = makeRuntime();
    const { signal, listeners } = trackedAbortController();
    const stream = r.window.lumen.stream('numbers', null, signal);
    const read = assert.rejects(stream.next(), isMalformedStream);
    const result = assert.rejects(stream.result, isMalformedStream);
    assert.doesNotThrow(() => r.streamFrame(frame));
    await read; await result;
    assert.equal((await stream.next()).done, true);
    assert.equal(listeners.size, 0);
    assert.equal(r.timers.size, 0);
    assert.deepEqual(cancelFrames(r), [{ session: SESSION, cancel: 1 }]);
  });
}

test('unidentifiable and unknown stream frames are ignored without consuming a read', async () => {
  const r = makeRuntime();
  const stream = r.window.lumen.stream('numbers', null);
  const read = stream.next();
  for (const frame of [null, undefined, [], false, 12, 'bad', {}, { id: '1' }, { id: 0 }, { id: -1 }, { id: 1.5 }, { id: 99 },
    { get id() { throw new Error('bad accessor'); } }]) {
    assert.doesNotThrow(() => r.streamFrame(frame));
    assert.equal(r.timers.size, 1);
    assert.equal(cancelFrames(r).length, 0);
  }
  r.streamFrame({ id: 1, item: 'valid' });
  assert.equal((await read).value, 'valid');
  await stream.return();
});

for (const pending of [false, true]) {
  test(`throwing ${pending ? 'item' : 'terminal'} accessor cannot strand a stream`, async () => {
    const r = makeRuntime();
    const stream = r.window.lumen.stream('numbers', null);
    const read = pending ? assert.rejects(stream.next(), isMalformedStream) : undefined;
    const result = assert.rejects(stream.result, isMalformedStream);
    const frame = pending ? { id: 1, get item() { throw new Error('invalid accessor'); } } :
      { id: 1, done: true, get result() { throw new Error('invalid accessor'); } };
    assert.doesNotThrow(() => r.streamFrame(frame));
    if (read) await read; else await assert.rejects(stream.next(), isMalformedStream);
    await result;
    assert.equal(r.timers.size, 0);
    assert.equal(cancelFrames(r).length, 1);
  });
}

for (const alreadyRead of [false, true]) {
  test(`${alreadyRead ? 'duplicate' : 'unsolicited'} stream item cancels instead of buffering without credit`, async () => {
    const r = makeRuntime();
    const stream = r.window.lumen.stream('numbers', null);
    if (alreadyRead) {
      const first = stream.next(); r.streamFrame({ id: 1, item: 1 }); assert.equal((await first).value, 1);
    }
    r.streamFrame({ id: 1, item: 'no credit' });
    await assert.rejects(stream.next(), isMalformedStream);
    await assert.rejects(stream.result, isMalformedStream);
    for (let index = 0; index < 1000; index++) r.streamFrame({ id: 1, item: index });
    assert.equal((await stream.next()).done, true);
    assert.equal(cancelFrames(r).length, 1);
    assert.equal(r.timers.size, 0);
  });
}

test('late and duplicate stream terminal frames cannot change settled result or another stream', async () => {
  const r = makeRuntime();
  const first = r.window.lumen.stream('numbers', null);
  completeStream(r, 1, 'original');
  const second = r.window.lumen.stream('numbers', null);
  const read = second.next();
  completeStream(r, 1, 'replacement');
  r.streamFrame({ id: 1, item: 'late' });
  assert.equal(await first.result, 'original');
  assert.equal((await first.next()).value, 'original');
  assert.equal(r.timers.size, 1);
  completeStream(r, 2, 'second');
  assert.equal((await read).value, 'second');
});

test('invalid stream methods and options reject through iterator and result without IPC', async () => {
  const r = makeRuntime();
  for (const method of ['', null, undefined, 42, {}, []]) {
    const stream = r.window.lumen.stream(method, null);
    await assert.rejects(stream.next(), /Invalid method/);
    await assert.rejects(stream.result, /Invalid method/);
  }
  const invalid = r.window.lumen.stream('numbers', null, { signal: {} });
  await assert.rejects(invalid.next(), /Invalid abort signal/);
  await assert.rejects(invalid.result, /Invalid abort signal/);
  assert.equal(r.timers.size, 0);
  assert.deepEqual(r.outbound, []);
});

test('nonserializable stream arguments reject without listener, timer, or capacity leaks', async () => {
  const r = makeRuntime();
  const { signal, listeners } = trackedAbortController();
  const circular = {}; circular.self = circular;
  for (let index = 0; index < 40; index++) {
    const stream = r.window.lumen.stream('numbers', circular, signal);
    await assert.rejects(stream.next(), /circular/i);
    await assert.rejects(stream.result, /circular/i);
  }
  assert.equal(r.timers.size, 0);
  assert.equal(listeners.size, 0);
  assert.deepEqual(r.outbound, []);
});

test('stream registration transport failures cannot leak capacity or abort listeners', async () => {
  const r = makeRuntime({ throwTransport: true });
  const { signal, listeners } = trackedAbortController();
  for (let index = 0; index < 40; index++) {
    const stream = r.window.lumen.stream('numbers', null, signal);
    await assert.rejects(stream.next(), /mock transport unavailable/);
    await assert.rejects(stream.result, /mock transport unavailable/);
  }
  assert.equal(r.timers.size, 0);
  assert.equal(listeners.size, 0);
  assert.deepEqual(r.outbound, []);
});

test('navigation during stream argument serialization prevents registration', async () => {
  const r = makeRuntime();
  const stream = r.window.lumen.stream('numbers', { toJSON() { r.pagehide(); return null; } });
  await assert.rejects(stream.next(), /Page session ended/);
  await assert.rejects(stream.result, /Page session ended/);
  assert.deepEqual(r.outbound, []);
  assert.equal(r.timers.size, 0);
});

test('reentrant argument serialization cannot exceed the combined capacity bound', async () => {
  const r = makeRuntime();
  const calls = [];
  const stream = r.window.lumen.stream('numbers', { toJSON() {
    for (let index = 0; index < 32; index++) calls.push(r.window.lumen.call('echo', null));
    return null;
  } });
  await assert.rejects(stream.next(), /Too many pending calls/);
  assert.equal(r.outbound.length, 32);
  assert.equal(r.timers.size, 32);
  for (const frame of r.outbound) r.respond(frame.request.id, { Ok: null });
  await Promise.all(calls);
  assert.equal(r.timers.size, 0);
});

test('reentrant call serialization cannot exceed combined capacity with streams', async () => {
  const r = makeRuntime();
  const calls = Array.from({ length: 16 }, () => r.window.lumen.call('echo', null));
  const streams = [];
  const overflow = r.window.lumen.call('echo', { toJSON() {
    for (let index = 0; index < 16; index++) streams.push(r.window.lumen.stream('numbers', null));
    return null;
  } });
  await assert.rejects(overflow, /Too many pending calls/);
  assert.equal(r.outbound.length, 32);
  for (let id = 1; id <= 16; id++) r.respond(id, { Ok: null });
  await Promise.all(calls);
  await Promise.all(streams.map(stream => stream.return()));
  assert.equal(r.timers.size, 0);
});

test('pull transport failure rejects read and result and removes the live stream', async () => {
  const r = makeRuntime({ throwTransportAt: 2 });
  const { signal, listeners } = trackedAbortController();
  const stream = r.window.lumen.stream('numbers', null, signal);
  await assert.rejects(stream.next(), /mock transport unavailable/);
  await assert.rejects(stream.result, /mock transport unavailable/);
  assert.deepEqual(r.outbound, [
    { session: SESSION, stream: { id: 1, method: 'numbers', args: null } },
    { session: SESSION, cancel: 1 },
  ]);
  assert.equal(listeners.size, 0);
  assert.equal(r.timers.size, 0);
  assert.equal((await stream.next()).done, true);
});

test('cancellation transport failure never strands pending reads or cleanup', async () => {
  const r = makeRuntime({ throwTransportAt: 3 });
  const { signal, listeners } = trackedAbortController();
  const stream = r.window.lumen.stream('numbers', null, signal);
  const next = stream.next();
  await stream.return();
  assert.equal((await next).done, true);
  await assert.rejects(stream.result, isCancelled);
  assert.equal(listeners.size, 0);
  assert.equal(r.timers.size, 0);
});

test('return after successful completion leaves result fulfilled and sends no cancellation', async () => {
  const r = makeRuntime();
  const { controller, signal, listeners } = trackedAbortController();
  const stream = r.window.lumen.stream('numbers', null, signal);
  const next = stream.next();
  completeStream(r, 1, false);
  assert.equal((await next).value, false);
  await stream.return('discarded'); controller.abort();
  assert.equal(await stream.result, false);
  assert.equal(cancelFrames(r).length, 0);
  assert.equal(r.timers.size, 0);
  assert.equal(listeners.size, 0);
});

test('reentrant stream serialization cannot bypass the 16-stream bound', async () => {
  const r = makeRuntime();
  const streams = [];
  const overflow = r.window.lumen.stream('numbers', { toJSON() {
    for (let index = 0; index < 16; index++) streams.push(r.window.lumen.stream('numbers', null));
    return null;
  } });
  await assert.rejects(overflow.next(), /Too many live streams/);
  await assert.rejects(overflow.result, /Too many live streams/);
  assert.equal(r.outbound.length, 16);
  assert.equal(r.timers.size, 16);
  await Promise.all(streams.map(stream => stream.return()));
  assert.equal(r.timers.size, 0);
});

test('navigation during call serialization prevents an otherwise-valid late call', async () => {
  const r = makeRuntime();
  await assert.rejects(r.window.lumen.call('echo', { toJSON() { r.pagehide(); return null; } }), /Page session ended/);
  assert.equal(r.timers.size, 0);
  assert.deepEqual(r.outbound, []);
});

test('shared Rust-host wire fixture delivers items and final result with one credit per frame', async () => {
  const frames = JSON.parse(await readFile(new URL('../crates/lumendesk/tests/stream-frames.json', import.meta.url), 'utf8'));
  const r = makeRuntime();
  const stream = r.window.lumen.stream('numbers', { count: 3 });
  const received = [];
  let resultSettled = false;
  stream.result.then(() => { resultSettled = true; });
  for (const [index, frame] of frames.entries()) {
    assert.equal(resultSettled, false, 'final result stays pending until the host terminal frame');
    const next = stream.next();
    assert.equal(pullFrames(r).length, index + 1);
    assert.deepEqual(pullFrames(r).at(-1), { session: SESSION, pull: frame.id });
    r.streamFrame(frame);
    const response = await next;
    if ('item' in frame) {
      assert.deepEqual(normalized(response), { done: false, value: frame.item });
      received.push(response.value);
    } else {
      assert.deepEqual(normalized(response), { done: true, value: frame.result.Ok });
      assert.equal(await stream.result, frame.result.Ok);
    }
  }
  assert.deepEqual(received, [0, 1, 2]);
  assert.equal(await stream.result, 3);
  assert.equal(resultSettled, true);
  assert.equal((await stream.next()).done, true);
  assert.equal(pullFrames(r).length, frames.length);
  assert.equal(cancelFrames(r).length, 0);
  assert.equal(r.timers.size, 0);
});

test('host lifetime ceiling rejects a pending read even with a longer frontend timeout', async () => {
  const r = makeRuntime();
  const { controller, signal, listeners } = trackedAbortController();
  const stream = r.window.lumen.stream('slow', null, { signal, timeoutMs: 300000 });
  const predicate = error => error.code === 'backend' && /60s/.test(error.message);
  const next = assert.rejects(stream.next(), predicate);
  const result = assert.rejects(stream.result, predicate);
  assert.equal([...r.timers.values()][0].delay, 300000);
  r.streamFrame({ id: 1, done: true, result: { Err: { code: 'backend', message: 'Stream exceeded the host 60s lifetime cap' } } });
  await next; await result;
  assert.equal((await stream.next()).done, true);
  assert.equal(r.timers.size, 0);
  assert.equal(listeners.size, 0);
  controller.abort();
  assert.equal(cancelFrames(r).length, 0, 'host terminal errors need no duplicate cancellation');
});
