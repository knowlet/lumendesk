(() => {
  'use strict';
  if (window.top !== window || window.lumen) return;
  const token = __SESSION__;
  const pending = new Map();
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
    pending.clear(); listeners.clear();
  };
  window.addEventListener('pagehide', rejectAll, {once:true});
  const call = (method, args, options = {}) => new Promise((resolve, reject) => {
    if (closed) { reject(new Error('Page session ended')); return; }
    if (pending.size >= 32) { reject(new Error('Too many pending calls')); return; }
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
    const owns=(value,key)=>Object.prototype.hasOwnProperty.call(value,key);
    const record=value=>value!==null && typeof value==='object' && !Array.isArray(value);
    let value, error;
    try {
      const result=response.result;
      if(!record(result) || Object.keys(result).length!==1) throw new Error();
      if(owns(result,'Ok')) {
        value=result.Ok;
        if(value===undefined) throw new Error();
      } else if(owns(result,'Err')) {
        const details=result.Err;
        if(!record(details) || !owns(details,'code') || typeof details.code!=='string' || !details.code ||
          Object.keys(details).some(key=>key!=='code' && key!=='message') ||
          (owns(details,'message') && typeof details.message!=='string')) throw new Error();
        error=new Error(details.message || details.code);
        error.code=details.code;
      } else throw new Error();
    } catch (_) {
      error=new Error('Malformed backend response');
      error.code='malformed_response';
    }
    // Validate first, then always settle the matching call after releasing it.
    // A broken response must never clear the only timer and strand its promise.
    pending.delete(id); entry.cleanup();
    if(error) entry.reject(error); else entry.resolve(value);
  },writable:false,configurable:false});
  Object.defineProperty(window,'__lumenEvent',{value:(name,value)=>{
    const set=listeners.get(name); if(!set)return;
    for(const entry of Array.from(set)) {
      if(entry.once)set.delete(entry);
      try { entry.callback(value); } catch(error) { console.error(error); }
    }
  },writable:false,configurable:false});
  Object.defineProperty(window,'lumen',{value:Object.freeze({call,on,runtime:Object.freeze(__RUNTIME__)}),writable:false,configurable:false});
  window.dispatchEvent(new CustomEvent('lumendesk-ready'));
})();
