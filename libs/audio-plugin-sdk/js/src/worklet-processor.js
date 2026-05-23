// libs/audio-plugin-sdk/js/src/worklet-processor.js
// Plain JS — loaded as a Blob URL at runtime, never compiled.
// Runs inside AudioWorkletGlobalScope (no DOM, no ES modules).

class AudioPluginProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this._wasm = null;
    this._ready = false;
    this.enabled = true;
    this.port.onmessage = (e) => this._onMessage(e.data);
  }

  // ── memory helpers ──────────────────────────────────────────────────────────

  // Write a JS string to WASM memory. Returns [ptr, byteLen].
  _writeString(str) {
    const len = str.length;
    const ptr = this._wasm.exports.__ap_alloc(len);
    const mem = new Uint8Array(this._wasm.exports.memory.buffer, ptr, len);
    for (let i = 0; i < len; i++) mem[i] = str.charCodeAt(i);
    return [ptr, len];
  }

  // Write a Float32Array to WASM memory. Returns ptr.
  // Allocates encoded.length * 4 bytes; always re-fetches memory.buffer after
  // alloc because alloc may trigger a memory.grow that detaches old views.
  _writeF32(data) {
    const byteLen = data.length * 4;
    const ptr = this._wasm.exports.__ap_alloc(byteLen);
    new Float32Array(this._wasm.exports.memory.buffer, ptr, data.length).set(data);
    return ptr;
  }

  // Copy bytes out of WASM memory as a new ArrayBuffer (safe to transfer).
  _readBytes(ptr, len) {
    return this._wasm.exports.memory.buffer.slice(ptr, ptr + len);
  }

  _free(ptr, byteLen) {
    this._wasm.exports.__ap_free(ptr, byteLen);
  }

  // ── message handler ─────────────────────────────────────────────────────────

  async _onMessage(msg) {
    switch (msg.type) {
      case 'init':            await this._init(msg.wasmBytes, msg.initialParams, msg.enabled); break;
      case 'set_parameter':   this._setParameter(msg.id, msg.value); break;
      case 'get_parameter':   this._getParameter(msg.id);          break;
      case 'set_enabled':     this.enabled = msg.enabled;           break;
      case 'set_data':        this._setData(msg.data);             break;
      case 'request_snapshot': this._requestSnapshot();            break;
    }
  }

  async _init(wasmBytes, initialParams, enabled = true) {
    const { instance } = await WebAssembly.instantiate(wasmBytes);
    this._wasm = instance;
    this._wasm.exports.__ap_new();
    this.enabled = enabled;
    this._ready = true;
    this.port.postMessage({ type: 'ready' });
    for (const [id, value] of Object.entries(initialParams)) {
      this._setParameter(id, value);
    }
  }

  _setParameter(id, value) {
    const [ptr, len] = this._writeString(id);
    this._wasm.exports.__ap_set_parameter(ptr, len, value);
    this._free(ptr, len);
  }

  _getParameter(id) {
    const [ptr, len] = this._writeString(id);
    const value = this._wasm.exports.__ap_get_parameter(ptr, len);
    this._free(ptr, len);
    this.port.postMessage({ type: 'parameter_value', id, value });
  }

  _requestSnapshot() {
    const packed = this._wasm.exports.__ap_render_snapshot(); // BigInt
    const ptr = Number(packed >> 32n);
    const len = Number(packed & 0xFFFFFFFFn);
    if (len === 0) return;
    const buffer = this._readBytes(ptr, len);
    this.port.postMessage({ type: 'snapshot', buffer }, [buffer]);
  }

  // ── audio processing ────────────────────────────────────────────────────────

  process(inputs, outputs) {
    if (!this._ready || !inputs[0]?.length) return true;

    const input = inputs[0];
    const output = outputs[0];

    //bypass if disabled
    if (!this.enabled) {
      for (let ch = 0; ch < input.length; ch++) output[ch].set(input[ch]);
      return true;
    }
    const channels = input.length;
    const frameCount = input[0].length; // always 128 in AudioWorklet
    const sampleCount = channels * frameCount;

    // Interleave planar channel arrays: [L0,R0, L1,R1, …]
    const interleaved = new Float32Array(sampleCount);
    for (let i = 0; i < frameCount; i++) {
      for (let ch = 0; ch < channels; ch++) {
        interleaved[i * channels + ch] = input[ch][i];
      }
    }

    // Write to WASM, process in-place, read back
    // __ap_process takes buf_len as f32 COUNT (not bytes)
    const bufPtr = this._writeF32(interleaved);
    this._wasm.exports.__ap_process(bufPtr, sampleCount, channels, sampleRate, currentTime);

    const processed = new Float32Array(this._wasm.exports.memory.buffer, bufPtr, sampleCount);
    for (let i = 0; i < frameCount; i++) {
      for (let ch = 0; ch < channels; ch++) {
        output[ch][i] = processed[i * channels + ch];
      }
    }

    this._free(bufPtr, sampleCount * 4);
    return true;
  }
}

// The caller appends: registerProcessor('<unique-name>', AudioPluginProcessor)
//registerProcessor('audio-plugin-processor', AudioPluginProcessor);
