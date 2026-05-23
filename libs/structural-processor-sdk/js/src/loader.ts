export interface StructuralProcessorWasm {
    memory: WebAssembly.Memory;
    __sp_alloc(size: number): number;
    __sp_free(ptr: number, len: number): void;
    __sp_apply_chain(
        samplesPtr: number,
        samplesLen: number,
        sampleRate: number,
        channels: number,
        editsPtr: number,
        editsLen: number,
    ): void;
    __sp_result_ptr(): number;
    __sp_result_len(): number;
    __sp_descriptors_init(): void;
    __sp_descriptors_ptr(): number;
    __sp_descriptors_len(): number;
    __sp_validate_edit(
        typePtr: number,
        typeLen: number,
        paramsPtr: number,
        paramsLen: number,
    ): number;
    __sp_map_time_forward(
        editsPtr: number,
        editsLen: number,
        t: number,
        duration: number,
    ): number;
    __sp_map_time_back(
        editsPtr: number,
        editsLen: number,
        t: number,
        duration: number,
    ): number;
}

let wasmInstance: StructuralProcessorWasm | null = null;
let wasmPromise: Promise<StructuralProcessorWasm> | null = null;

export async function getWasm(
    wasmUrl = '/structural-processor.wasm',
): Promise<StructuralProcessorWasm> {
    if (wasmInstance) return wasmInstance;
    if (wasmPromise) return wasmPromise;
    wasmPromise = WebAssembly.instantiateStreaming(fetch(wasmUrl)).then((result) => {
        wasmInstance = result.instance.exports as unknown as StructuralProcessorWasm;
        return wasmInstance;
    });
    return wasmPromise;
}

// ── Memory helpers ────────────────────────────────────────────────────────────

export function writeString(wasm: StructuralProcessorWasm, s: string): [number, number] {
    const encoded = new TextEncoder().encode(s);
    const ptr = wasm.__sp_alloc(encoded.byteLength);
    new Uint8Array(wasm.memory.buffer, ptr, encoded.byteLength).set(encoded);
    return [ptr, encoded.byteLength];
}

export function writeFloat32Array(
    wasm: StructuralProcessorWasm,
    arr: Float32Array,
): [number, number] {
    const byteLen = arr.byteLength;
    const ptr = wasm.__sp_alloc(byteLen);
    new Float32Array(wasm.memory.buffer, ptr, arr.length).set(arr);
    return [ptr, arr.length]; // length in f32 elements, not bytes
}

export function readFloat32Array(
    wasm: StructuralProcessorWasm,
    ptr: number,
    len: number,
): Float32Array {
    return new Float32Array(wasm.memory.buffer, ptr, len).slice(); // copy out before any alloc
}

/** Convert an AudioBuffer to interleaved f32. */
export function audioBufferToInterleaved(buffer: AudioBuffer): Float32Array {
    const ch = buffer.numberOfChannels;
    const frames = buffer.length;
    const out = new Float32Array(frames * ch);
    for (let c = 0; c < ch; c++) {
        const channelData = buffer.getChannelData(c);
        for (let f = 0; f < frames; f++) {
            out[f * ch + c] = channelData[f];
        }
    }
    return out;
}

/** Convert interleaved f32 back to an AudioBuffer. */
export function interleavedToAudioBuffer(
    samples: Float32Array,
    channels: number,
    sampleRate: number,
): AudioBuffer {
    const frames = Math.max(1, Math.floor(samples.length / channels));
    const ctx = new OfflineAudioContext(channels, frames, sampleRate);
    const buffer = ctx.createBuffer(channels, frames, sampleRate);
    for (let c = 0; c < channels; c++) {
        const channelData = buffer.getChannelData(c);
        for (let f = 0; f < frames; f++) {
            channelData[f] = samples[f * channels + c];
        }
    }
    return buffer;
}
