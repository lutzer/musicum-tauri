// // libs/audio-plugin-sdk/js/src/analyzer.ts
// import { parseAudio } from './audio-parser';

// const CHUNK_SIZE: number = 128;

// export class WasmAnalyzer {
//   constructor(private wasmUrl: string) {}


//   async run(audioUrl: string, paramsJson: string): Promise<string> {
//     const [wasmRes, audioRes] = await Promise.all([
//       fetch(`${this.wasmUrl}.wasm`),
//       fetch(audioUrl),
//     ]);
//     if (!wasmRes.ok) throw new Error(`WasmAnalyzer: failed to fetch ${this.wasmUrl}.wasm`);
//     if (!audioRes.ok) throw new Error(`WasmAnalyzer: failed to fetch audio at ${audioUrl}`);

//     // 2. Instantiate WASM and decode audio in parallel
//     const [{ instance }, audioBytes] = await Promise.all([
//       WebAssembly.instantiate(await wasmRes.arrayBuffer()),
//       audioRes.arrayBuffer(),
//     ]);
//     const exp = instance.exports as Record<string, (...args: unknown[]) => unknown> & {
//       memory: WebAssembly.Memory;
//     };

//     /* write bytes to memory buffer */
//     const _writeBytes = function(data: Uint8Array): [number, number] {
//       const exp = instance.exports as Record<string, Function>;
//       const mem = instance.exports.memory as WebAssembly.Memory;
//       const ptr = exp.__aa_alloc(data.length) as number;
//       new Uint8Array(mem.buffer, ptr, data.length).set(data);
//       return [ptr, data.length];
//     }

//     /* read bytes from memory buffer */
//     const _readString = function(ptr: number, len: number): string {
//       const mem = instance.exports.memory as WebAssembly.Memory;
//       return new TextDecoder().decode(new Uint8Array(mem.buffer, ptr, len));
//     }

//     // parse the audio file
//     const { numChannels, sampleRate, samples } = await parseAudio(audioBytes);

//     // Reset and initialise with plugin params
//     exp.__aa_reset();
//     const initEncoded = new TextEncoder().encode(paramsJson);
//     const [initPtr, initLen] = _writeBytes(initEncoded);
//     exp.__aa_init(initPtr, initLen);
//     exp.__aa_free(initPtr, initLen);

//     // Feed audio in chunks
//     // __aa_analyze expects len as f32 COUNT (same convention as __ap_process)
//     const frameData = new Float32Array(CHUNK_SIZE * numChannels);
//     for (let offset = 0; offset < samples[0].length; offset += CHUNK_SIZE) {
//       const frameSize = Math.min(CHUNK_SIZE, samples.length - offset);
//       for (let i = 0; i < frameSize; i++) {
//         for (let ch = 0; ch < numChannels; ch++) {
//           frameData[i * numChannels + ch] = samples[ch][offset+i];
//         }
//       }

//       // write to wasm instance
//       const bufferByteSize = frameSize * numChannels * 4
//       const [ptr] = _writeBytes(new Uint8Array(frameData.buffer, frameData.byteOffset, bufferByteSize));
//       const timestamp = offset / sampleRate;
//       exp.__aa_analyze(ptr, frameSize * numChannels, numChannels, sampleRate, timestamp);
//       exp.__aa_free(ptr, bufferByteSize);
//     }

//     // Retrieve JSON result
//     const len = exp.aa_result_len() as number;
//     const ptr = exp.aa_result_ptr() as number;
//     return _readString(ptr, len);
//   }
// }

import { parseAudio } from './audio-parser';

/**
 * Runs an AudioAnalyzer WASM binary on the main thread.
 *
 * Fetches the plugin's WASM fresh (independent of the AudioWorklet copy),
 * decodes audio from a URL, feeds it in chunks through the `aa_*` ABI,
 * and returns the result JSON string.
 *
 * Usage:
 * ```typescript
 * handle.onEvent(async (kind, payload) => {
 *   if (kind !== 'analyze') return;
 *   const analyzer = new WasmAnalyzer('/plugins/normalize');
 *   const result = await analyzer.run(payload, '/audio/my-file.wav');
 *   handle.receiveData('analysis_result', result);
 * });
 * ```
 */
export class WasmAnalyzer {
  constructor(private wasmUrl: string) {}

  async run(audioUrl: string, paramsJson: string, chunkSize = 4096): Promise<string> {
    // 1. Fetch WASM and audio in parallel
    const [wasmRes, audioRes] = await Promise.all([
      fetch(this.wasmUrl),
      fetch(audioUrl),
    ]);
    if (!wasmRes.ok) throw new Error(`WasmAnalyzer: failed to fetch ${this.wasmUrl}.wasm`);
    if (!audioRes.ok) throw new Error(`WasmAnalyzer: failed to fetch audio at ${audioUrl}`);

    // 2. Instantiate WASM and decode audio in parallel
    const [{ instance }, audioBytes] = await Promise.all([
      WebAssembly.instantiate(await wasmRes.arrayBuffer()),
      audioRes.arrayBuffer(),
    ]);
    const ex = instance.exports as Record<string, (...args: unknown[]) => unknown> & {
      memory: WebAssembly.Memory;
    };

    const { numChannels, sampleRate, samples } = await parseAudio(audioBytes);

    // 3. Interleave channel data: [L0, R0, L1, R1, …]
    const length = samples[0].length;
    const interleaved = new Float32Array(length * numChannels);
    for (let c = 0; c < numChannels; c++) {
      const ch = samples[c];
      for (let i = 0; i < length; i++) {
        interleaved[i * numChannels + c] = ch[i];
      }
    }

    const _writeF32 = function(data: Float32Array) {
      const byteLen = data.length * 4;
      const ptr = ex.__aa_alloc(byteLen) as number;
      new Float32Array(ex.memory.buffer, ptr, data.length).set(data);
      return { ptr, byteLen, sampleCount: data.length };
    }

    // Helper: write a UTF-8 string into WASM memory
    const _writeStr = (str: string): { ptr: number; len: number } => {
      const bytes = new TextEncoder().encode(str);
      const ptr = ex.__aa_alloc(bytes.length) as number;
      new Uint8Array(ex.memory.buffer, ptr, bytes.length).set(bytes);
      return { ptr, len: bytes.length };
    };

    const _readString = function(ptr: number, len: number): string {
      const mem = instance.exports.memory as WebAssembly.Memory;
      return new TextDecoder().decode(new Uint8Array(mem.buffer, ptr, len));
    }

    // 4. Initialize analyzer instance
    ex.__aa_reset();

    // 5. Forward plugin parameters
    if (paramsJson.length > 0) {
      const { ptr, len } = _writeStr(paramsJson);
      ex.__aa_init(ptr, len);
      ex.__aa_free(ptr, len);
    }

    // 6. Feed audio in chunks
    for (let frame = 0; frame < length; frame += chunkSize) {
      const end = Math.min(frame + chunkSize, length);
      const chunk = interleaved.subarray(frame * numChannels, end * numChannels);
      const { ptr, byteLen, sampleCount } = _writeF32(chunk);
      ex.__aa_analyze(ptr, sampleCount, numChannels, sampleRate, 0);
      ex.__aa_free(ptr, byteLen);
    }

    // 7. Read result JSON
    const resultLen = ex.__aa_result_len() as number;
    const resultPtr = ex.__aa_result_ptr() as number;
    return _readString(resultPtr, resultLen);
  }
}