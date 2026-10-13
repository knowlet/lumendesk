(() => {
  'use strict';
  if (window.top !== window || window.lumen) return;
  const token = __SESSION__;
  const pending = new Map();
  const streams = new Map();
  const listeners = new Map();
  let nextId = 1;
  let closed = false;
  const transport = window.ipc && window.ipc.postMessage.bind(window.ipc);
  if (!transport) return;
  const rejectAll = () => {
    closed = true;
    for (const entry of pending.values()) {
      entry.cleanup();
      entry.reject(new Error('Page session ended'));
    }
    pending.clear();
    for (const entry of Array.from(streams.values())) entry.cancel(new Error('Page session ended'));
    listeners.clear();
  };
  window.addEventListener('pagehide', rejectAll, {once:true});
  const call = (method, args, options = {}) => new Promise((resolve, reject) => {
    if (closed) { reject(new Error('Page session ended')); return; }
    if (pending.size + streams.size >= 32) { reject(new Error('Too many pending calls or streams')); return; }
    if (typeof method !== 'string' || !method) { reject(new Error('Invalid method')); return; }
    const isSignal=value=>value && typeof value.aborted==='boolean' &&
      typeof value.addEventListener==='function' && typeof value.removeEventListener==='function';
    const signal=isSignal(options) ? options : options.signal;
    if(signal!==undefined && !isSignal(signal)) throw new TypeError('Invalid abort signal');
    const abortError=()=>{const error=new Error('Call aborted');error.name='AbortError';error.code='cancelled';return error;};
    if(signal && signal.aborted) {reject(abortError());return;}
    const id = nextId++;
    const timeout = Math.min(Math.max(options.timeoutMs || 30000, 1), 300000);
    // Serialize before registering a call: user-provided toJSON may abort it.
    const message=JSON.stringify({session:token,request:{id,method,args}});
    if(signal && signal.aborted) {reject(abortError());return;}
    if(closed) {reject(new Error('Page session ended'));return;}
    if(pending.size + streams.size >= 32) {reject(new Error('Too many pending calls or streams'));return;}
    let timer;
    const cleanup=()=>{clearTimeout(timer);if(signal)signal.removeEventListener('abort',onAbort);};
    const cancel=error=>{
      if(!pending.delete(id))return;
      cleanup();
      try { transport(JSON.stringify({session:token,cancel:id})); } catch (_) {}
      reject(error);
    };
    const onAbort=()=>cancel(abortError());
    timer=setTimeout(()=>cancel(new Error('Call timed out')),timeout);
    pending.set(id, {resolve,reject,cleanup});
    try {
      if(signal)signal.addEventListener('abort',onAbort,{once:true});
      if(signal && signal.aborted) {onAbort();return;}
      transport(message);
    } catch (error) { cleanup(); pending.delete(id); reject(error); }
  });
  const stream = (method, args, options = {}) => {
    let id, timer, signal, timeout, waiting;
    let sent = false;
    let resolveResult, rejectResult;
    const result = new Promise((resolve, reject) => { resolveResult = resolve; rejectResult = reject; });
    // Consumers may only use for-await. Preserve rejection for explicit .result
    // observers without creating an unhandled rejection for iterator-only use.
    result.catch(() => {});
    // There is no item queue. Only one unresolved next() owns a pull credit.
    // A terminal result may arrive immediately after the last credited item.
    let terminal;
    const active = () => streams.has(id);
    const abortError = () => {
      const error = new Error('Stream aborted');
      error.name = 'AbortError'; error.code = 'cancelled';
      return error;
    };
    const cleanup = () => {
      clearTimeout(timer);
      if (signal) {
        try { signal.removeEventListener('abort', onAbort); } catch (_) {}
      }
    };
    const finish = (kind, value, cancel = false) => {
      if (!streams.delete(id)) return;
      cleanup();
      terminal = { kind, value };
      if (kind === 'error') rejectResult(value);
      else if (cancel) rejectResult(abortError());
      else resolveResult(value);
      if (cancel && sent) {
        try { transport(JSON.stringify({ session: token, cancel: id })); } catch (_) {}
      }
      if (waiting) {
        const reader = waiting; waiting = undefined;
        terminal = { kind: 'done', value: undefined };
        if (kind === 'error') reader.reject(value);
        else reader.resolve({ done: true, value });
      }
    };
    const cancel = error => finish('error', error, true);
    const onAbort = () => cancel(abortError());
    const armTimeout = () => {
      clearTimeout(timer);
      // The same bounded timeout covers an outstanding pull or an idle consumer,
      // including an iterator constructed but never read.
      timer = setTimeout(() => {
        const error = new Error('Stream timed out'); error.code = 'timeout';
        cancel(error);
      }, timeout);
    };
    const malformed = () => {
      const error = new Error('Malformed backend stream response');
      error.code = 'malformed_response';
      cancel(error);
    };
    const receive = frame => {
      try {
        const owns = key => Object.prototype.hasOwnProperty.call(frame, key);
        const keys = Object.keys(frame);
        if (owns('item') && keys.length === 2 && owns('id')) {
          const value = frame.item;
          if (value === undefined || !waiting) { malformed(); return; }
          const reader = waiting; waiting = undefined;
          armTimeout();
          reader.resolve({ done: false, value });
        } else if (frame.done === true && keys.length === 3 && owns('id') && owns('done') && owns('result')) {
          const result = parseResult(frame.result);
          finish(result.error ? 'error' : 'done', result.error || result.value);
        } else malformed();
      } catch (_) { malformed(); }
    };
    const iterator = {
      [Symbol.asyncIterator]() { return this; },
      next() {
        if (terminal) {
          const result = terminal;
          terminal = { kind: 'done', value: undefined };
          return result.kind === 'error' ? Promise.reject(result.value) :
            Promise.resolve({ done: true, value: result.value });
        }
        if (waiting) {
          const error = new Error('Concurrent stream reads are not allowed'); error.code = 'concurrent_read';
          return Promise.reject(error);
        }
        return new Promise((resolve, reject) => {
          waiting = { resolve, reject };
          armTimeout();
          try { transport(JSON.stringify({ session: token, pull: id })); }
          catch (error) { cancel(error); }
        });
      },
      return(value) {
        finish('done', undefined, true);
        terminal = { kind: 'done', value: undefined };
        return Promise.resolve({ done: true, value });
      },
      throw(error) {
        finish('error', error, true);
        terminal = { kind: 'done', value: undefined };
        return Promise.reject(error);
      },
    };
    Object.defineProperty(iterator, 'result', { value: result, enumerable: true });
    try {
      if (closed) throw new Error('Page session ended');
      if (streams.size >= 16) throw new Error('Too many live streams');
      if (pending.size + streams.size >= 32) throw new Error('Too many pending calls or streams');
      if (typeof method !== 'string' || !method) throw new TypeError('Invalid method');
      const isSignal = value => value && typeof value.aborted === 'boolean' &&
        typeof value.addEventListener === 'function' && typeof value.removeEventListener === 'function';
      signal = isSignal(options) ? options : options.signal;
      if (signal !== undefined && !isSignal(signal)) throw new TypeError('Invalid abort signal');
      if (signal && signal.aborted) throw abortError();
      const duration = Number(options.timeoutMs || 30000);
      timeout = Number.isFinite(duration) ? Math.min(Math.max(duration, 1), 300000) : 30000;
      id = nextId++;
      const message = JSON.stringify({ session: token, stream: { id, method, args } });
      // Serialization may run user code: recheck authority and both capacity limits.
      if (signal && signal.aborted) throw abortError();
      if (closed) throw new Error('Page session ended');
      if (streams.size >= 16) throw new Error('Too many live streams');
      if (pending.size + streams.size >= 32) throw new Error('Too many pending calls or streams');
      streams.set(id, { receive, cancel });
      armTimeout();
      if (signal) signal.addEventListener('abort', onAbort, { once: true });
      if (signal && signal.aborted) onAbort();
      if (active()) { sent = true; transport(message); }
    } catch (error) {
      if (active()) cancel(error);
      else { cleanup(); terminal = { kind: 'error', value: error }; rejectResult(error); }
    }
    return iterator;
  };
  const parseResult = result => {
    const owns = (value, key) => Object.prototype.hasOwnProperty.call(value, key);
    const record = value => value !== null && typeof value === 'object' && !Array.isArray(value);
    if (!record(result) || Object.keys(result).length !== 1) throw new Error();
    if (owns(result, 'Ok')) {
      const value = result.Ok;
      if (value === undefined) throw new Error();
      return { value };
    }
    if (owns(result, 'Err')) {
      const details = result.Err;
      if (!record(details) || !owns(details, 'code') || typeof details.code !== 'string' || !details.code ||
        Object.keys(details).some(key => key !== 'code' && key !== 'message') ||
        (owns(details, 'message') && typeof details.message !== 'string')) throw new Error();
      const error = new Error(details.message || details.code);
      error.code = details.code;
      return { error };
    }
    throw new Error();
  };
  const on = (name, callback, options = {}) => {
    if (typeof name !== 'string' || typeof callback !== 'function') throw new TypeError('Invalid event listener');
    if (!listeners.has(name)) listeners.set(name,new Set());
    const entry={callback,once:!!options.once};
    listeners.get(name).add(entry);
    return () => { const set=listeners.get(name); if(set) {set.delete(entry); if(!set.size) listeners.delete(name);} };
  };
  Object.defineProperty(window,'__lumenResponse',{value:response=>{
    if(!response || typeof response!=='object' || Array.isArray(response)) return;
    const id=response.id;
    const entry=pending.get(id); if(!entry) return;
    let value, error;
    try {
      ({ value, error } = parseResult(response.result));
    } catch (_) {
      error=new Error('Malformed backend response');
      error.code='malformed_response';
    }
    // Validate first, then always settle the matching call after releasing it.
    // A broken response must never clear the only timer and strand its promise.
    pending.delete(id); entry.cleanup();
    if(error) entry.reject(error); else entry.resolve(value);
  },writable:false,configurable:false});
  Object.defineProperty(window, '__lumenStream', { value: frame => {
    // Unidentifiable, unknown and late frames never consume another stream's credit.
    try {
      if (!frame || typeof frame !== 'object' || Array.isArray(frame)) return;
      const id = frame.id;
      if (!Number.isSafeInteger(id) || id < 1) return;
      const entry = streams.get(id);
      if (entry) entry.receive(frame);
    } catch (_) {}
  }, writable: false, configurable: false });
  Object.defineProperty(window,'__lumenEvent',{value:(name,value)=>{
    const set=listeners.get(name); if(!set)return;
    for(const entry of Array.from(set)) {
      if(entry.once)set.delete(entry);
      try { entry.callback(value); } catch(error) { console.error(error); }
    }
  },writable:false,configurable:false});
  Object.defineProperty(window,'lumen',{value:Object.freeze({call,stream,on,runtime:Object.freeze(__RUNTIME__)}),writable:false,configurable:false});
  window.dispatchEvent(new CustomEvent('lumendesk-ready'));
})();
