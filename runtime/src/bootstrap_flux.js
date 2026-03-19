
function serializeArrayBuffer(buffer) {
  const view = new Uint8Array(buffer);
  let bytesStr = "";
  for (let i = 0; i < view.length; i++) {
    bytesStr += String.fromCharCode(view[i]);
  }
  return btoa(bytesStr);
}

function deserializeArrayBuffer(b64) {
  const bytesStr = atob(b64);
  const buf = new Uint8Array(bytesStr.length);
  for (let i = 0; i < bytesStr.length; i++) {
    buf[i] = bytesStr.charCodeAt(i);
  }
  return buf.buffer;
}

const Flux = {
  serializeArrayBuffer,
  deserializeArrayBuffer,
  
  serialize(value) {
    if (value === null || typeof value !== 'object') return value;
    
    if (value instanceof ArrayBuffer) {
      return { __flux_type: 'ArrayBuffer', bytes: serializeArrayBuffer(value) };
    }
    
    if (ArrayBuffer.isView(value)) {
      return { 
        __flux_type: 'TypedArray', 
        constructor: value.constructor.name,
        bytes: serializeArrayBuffer(value.buffer.slice(value.byteOffset, value.byteOffset + value.byteLength))
      };
    }
    
    if (value instanceof Error) {
      return { 
        __flux_type: 'Error', 
        name: value.name, 
        message: value.message, 
        stack: value.stack 
      };
    }

    if (value.constructor && value.constructor.name === "CryptoKey") {
       return { __flux_type: 'CryptoKey_opaque' }; // Special handling in crypto wrapper
    }
    
    if (Array.isArray(value)) {
      return value.map(v => Flux.serialize(v));
    }
    
    const result = {};
    for (const key of Object.keys(value)) {
      result[key] = Flux.serialize(value[key]);
    }
    return result;
  },
  
  deserialize(value) {
    if (value === null || typeof value !== 'object') return value;
    
    if (value.__flux_type === 'ArrayBuffer') {
      return deserializeArrayBuffer(value.bytes);
    }
    
    if (value.__flux_type === 'TypedArray') {
      const buffer = deserializeArrayBuffer(value.bytes);
      const Ctor = globalThis[value.constructor] || Uint8Array;
      return new Ctor(buffer);
    }
    
    if (value.__flux_type === 'Error') {
      const err = new Error(value.message);
      err.name = value.name;
      err.stack = value.stack;
      return err;
    }
    
    if (Array.isArray(value)) {
      return value.map(v => Flux.deserialize(v));
    }
    
    const result = {};
    for (const key of Object.keys(value)) {
      result[key] = Flux.deserialize(value[key]);
    }
    return result;
  }
};

globalThis.Flux = globalThis.Flux || {};
Object.assign(globalThis.Flux, Flux);
