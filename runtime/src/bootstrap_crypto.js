import "ext:deno_web/00_infra.js";
import "ext:deno_web/01_mimesniff.js";
import "ext:deno_web/02_event.js";
import { structuredClone } from "ext:deno_web/02_structured_clone.js";
globalThis.structuredClone = structuredClone || (v => {
  try {
    return Flux.deserialize(Flux.serialize(v));
  } catch (e) {
    return JSON.parse(JSON.stringify(v));
  }
});
import "ext:deno_web/02_timers.js";
import "ext:deno_web/03_abort_signal.js";
import "ext:deno_web/04_global_interfaces.js";
import "ext:deno_web/05_base64.js";
import "ext:deno_web/06_streams.js";
import "ext:deno_web/08_text_encoding.js";
import "ext:deno_web/09_file.js";
import "ext:deno_web/10_filereader.js";
import "ext:deno_web/12_location.js";
import "ext:deno_web/13_message_port.js";
import "ext:deno_web/14_compression.js";
import { performance, Performance } from "ext:deno_web/15_performance.js";
globalThis.performance = performance;
globalThis.Performance = Performance;
import "ext:deno_web/16_image_data.js";
import "ext:deno_web/01_urlpattern.js";
import "ext:deno_web/01_broadcast_channel.js";

import { crypto } from "ext:deno_crypto/00_crypto.js";
import "ext:flux_runtime_ext/bootstrap_flux.js";



const originalGetRandomValues = crypto.getRandomValues.bind(crypto);
crypto.getRandomValues = function(array) {
  if (!globalThis.__FLUX_EXECUTION_ID__) {
    throw new Error("Flux: IO outside execution context");
  }
  const eid = __flux_eid();
  const replay = Deno.core.ops.op_flux_crypto_replay(eid);
  if (replay.has_recorded) {
    if (replay.response && replay.response.bytes) {
      const bytesStr = atob(replay.response.bytes);
      const view = new Uint8Array(array.buffer, array.byteOffset, array.byteLength);
      for (let i = 0; i < bytesStr.length; i++) {
        view[i] = bytesStr.charCodeAt(i);
      }
    }
    return array;
  }

  // Live Mode
  const result = Deno.core.ops.op_random_bytes(eid, array.byteLength);
  const bytesStr = atob(result.bytes);
  const view = new Uint8Array(array.buffer, array.byteOffset, array.byteLength);
  for (let i = 0; i < bytesStr.length; i++) {
    view[i] = bytesStr.charCodeAt(i);
  }
  return array;
};

// Intercept all subtle crypto methods for telemetry!
const subtleMethods = ['digest', 'generateKey', 'sign', 'verify', 'deriveBits', 'deriveKey', 'encrypt', 'decrypt', 'wrapKey', 'unwrapKey', 'exportKey', 'importKey'];

function sanitizeAlgorithm(alg) {
  if (!alg) return alg;
  const clone = Object.assign({}, alg);
  if (clone.publicExponent && clone.publicExponent instanceof Uint8Array) {
    clone.publicExponent = Array.from(clone.publicExponent);
  }
  return clone;
}

function deserializeAlgorithm(alg) {
  if (!alg) return alg;
  const clone = Object.assign({}, alg);
  if (clone.publicExponent && Array.isArray(clone.publicExponent)) {
    clone.publicExponent = new Uint8Array(clone.publicExponent);
  }
  return clone;
}

for (const method of subtleMethods) {
  if (typeof crypto.subtle[method] === 'function') {
    const original = crypto.subtle[method].bind(crypto.subtle);
    crypto.subtle[method] = async function(...args) {
      try {
        if (!globalThis.__FLUX_EXECUTION_ID__) {
          throw new Error("Flux: IO outside execution context");
        }
        const eid = __flux_eid();
        let replay;
        try {
            replay = Deno.core.ops.op_flux_crypto_replay(eid);
        } catch (replayErr) {
            throw replayErr;
        }
        
        if (replay.has_recorded) {
          const response = replay.response;
          if (response && response.type === 'ArrayBuffer') {
            return Flux.deserializeArrayBuffer(response.bytes);
          } else if (response && response.type === 'CryptoKey') {
            return await original('jwk', response.jwk, deserializeAlgorithm(response.algorithm), response.extractable, response.usages);
          } else if (response && response.type === 'KeyPair') {
            const publicKey = await original('jwk', response.publicKey.jwk, deserializeAlgorithm(response.publicKey.algorithm), response.publicKey.extractable, response.publicKey.usages);
            const privateKey = await original('jwk', response.privateKey.jwk, deserializeAlgorithm(response.privateKey.algorithm), response.privateKey.extractable, response.privateKey.usages);
            return { publicKey, privateKey };
          }
          return response;
        }

        let callArgs = [...args];
        if (method === 'generateKey' && callArgs.length >= 2) callArgs[1] = true;
        if (method === 'importKey' && callArgs.length >= 4) {
          const alg = callArgs[2];
          const algName = typeof alg === 'string' ? alg : (alg && alg.name);
          if (algName !== "PBKDF2") callArgs[3] = true;
        }
        if (method === 'deriveKey' && callArgs.length >= 4) callArgs[3] = true;
        if (method === 'unwrapKey' && callArgs.length >= 6) callArgs[5] = true;

        let result;
        try {
          result = await original(...callArgs);
        } catch (err) {
          throw err;
        }
        
        let serializedResult;
        try {
          if (result instanceof ArrayBuffer) {
            serializedResult = { type: 'ArrayBuffer', bytes: Flux.serializeArrayBuffer(result) };
          } else if (result && result.constructor && result.constructor.name === "CryptoKey") {
            if (result.extractable) {
              const jwk = await crypto.subtle.exportKey('jwk', result);
              serializedResult = { type: 'CryptoKey', jwk, algorithm: sanitizeAlgorithm(result.algorithm), extractable: result.extractable, usages: result.usages };
            } else {
              serializedResult = { type: 'CryptoKey_unextractable' };
            }
          } else if (result && result.publicKey && result.privateKey) {
            let pubJwk = null;
            let privJwk = null;
            if (result.publicKey.extractable) {
                pubJwk = { jwk: await crypto.subtle.exportKey('jwk', result.publicKey), algorithm: sanitizeAlgorithm(result.publicKey.algorithm), extractable: result.publicKey.extractable, usages: result.publicKey.usages };
            }
            if (result.privateKey.extractable) {
                privJwk = { jwk: await crypto.subtle.exportKey('jwk', result.privateKey), algorithm: sanitizeAlgorithm(result.privateKey.algorithm), extractable: result.privateKey.extractable, usages: result.privateKey.usages };
            }
            serializedResult = { type: 'KeyPair', publicKey: pubJwk, privateKey: privJwk };
          } else {
            serializedResult = result;
          }
        } catch (serializeErr) {
          throw serializeErr;
        }

        Deno.core.ops.op_flux_crypto_record(
          eid, 
          replay.call_index, 
          `crypto.subtle.${method}`, 
          {}, 
          serializedResult
        );

        return result;
      } catch (outerErr) {
        throw outerErr;
      }
    };
  }
}

globalThis.crypto = crypto;
