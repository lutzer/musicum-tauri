import type {
    StructuralProcessorDescriptor,
    StructuralProcessorManager,
    StructuralProcessorState,
} from './types';
import {
    getWasm,
    writeString,
    writeFloat32Array,
    readFloat32Array,
    audioBufferToInterleaved,
    interleavedToAudioBuffer,
    type StructuralProcessorWasm,
} from './loader';

// Module-level WASM ref for synchronous operations (validate, time mapping).
// Populated lazily on first getWasm() resolution.
let _wasmInstance: StructuralProcessorWasm | null = null;
getWasm()
    .then((w) => {
        _wasmInstance = w;
    })
    .catch(() => {});

export function createStructuralProcessorManager(
    _baseUrl?: string, // kept for API compatibility; no longer used
): StructuralProcessorManager {
    let descriptors = new Map<string, StructuralProcessorDescriptor>();
    let currentStates: StructuralProcessorState[] = [];
    let rawBuffer: AudioBuffer | null = null;
    let processedDuration = 0;
    let disposed = false;

    // Load descriptors from static JSON eagerly (fast — no WASM needed)
    fetch('/structural-processor-descriptors.json')
        .then((r) => r.json())
        .then((descs: StructuralProcessorDescriptor[]) => {
            descriptors = new Map(descs.map((d) => [d.id, d]));
        })
        .catch(() => {
            /* non-fatal; UI will lack descriptors until WASM loads */
        });

    const manager: StructuralProcessorManager = {
        onChange: undefined,

        async sync(states: StructuralProcessorState[], buffer: AudioBuffer | null): Promise<void> {
            if (disposed) return;
            currentStates = states;
            rawBuffer = buffer;

            if (!rawBuffer) return;

            const wasm = await getWasm();
            _wasmInstance = wasm;
            const channels = rawBuffer.numberOfChannels;
            const sampleRate = rawBuffer.sampleRate;

            const interleaved = audioBufferToInterleaved(rawBuffer);
            const [samplesPtr, samplesLen] = writeFloat32Array(wasm, interleaved);

            const editsJson = buildEditsJson(states);
            const [editsPtr, editsLen] = writeString(wasm, editsJson);

            wasm.__sp_apply_chain(samplesPtr, samplesLen, sampleRate, channels, editsPtr, editsLen);

            wasm.__sp_free(samplesPtr, interleaved.byteLength);
            wasm.__sp_free(editsPtr, editsLen);

            const resultPtr = wasm.__sp_result_ptr();
            const resultLen = wasm.__sp_result_len();
            const resultSamples = readFloat32Array(wasm, resultPtr, resultLen);

            const result = interleavedToAudioBuffer(resultSamples, channels, sampleRate);
            processedDuration = result.duration;
            manager.onChange?.(result);
        },

        dispose(): void {
            disposed = true;
            rawBuffer = null;
        },

        getModule(type: string) {
            const descriptor = descriptors.get(type);
            if (!descriptor) return undefined;
            return {
                descriptor,
                validate(params: Record<string, number>): boolean {
                    if (!_wasmInstance) return true; // optimistic before WASM loads
                    const [tPtr, tLen] = writeString(_wasmInstance, type);
                    const [pPtr, pLen] = writeString(_wasmInstance, JSON.stringify(params));
                    const result = _wasmInstance.__sp_validate_edit(tPtr, tLen, pPtr, pLen);
                    _wasmInstance.__sp_free(tPtr, tLen);
                    _wasmInstance.__sp_free(pPtr, pLen);
                    return result === 1;
                },
            };
        },

        mapToSourceTime(processedTime: number): number {
            if (!_wasmInstance || !rawBuffer) return processedTime;
            const editsJson = buildEditsJson(currentStates);
            const [ePtr, eLen] = writeString(_wasmInstance, editsJson);
            const result = _wasmInstance.__sp_map_time_back(
                ePtr,
                eLen,
                processedTime,
                rawBuffer.duration,
            );
            _wasmInstance.__sp_free(ePtr, eLen);
            return result;
        },

        mapToProcessedTime(rawTime: number): number {
            if (!_wasmInstance || !rawBuffer) return rawTime;
            const editsJson = buildEditsJson(currentStates);
            const [ePtr, eLen] = writeString(_wasmInstance, editsJson);
            const result = _wasmInstance.__sp_map_time_forward(
                ePtr,
                eLen,
                rawTime,
                rawBuffer.duration,
            );
            _wasmInstance.__sp_free(ePtr, eLen);
            return result;
        },

        clampToAllowedTime(rawTime: number): number {
            const processed = manager.mapToProcessedTime(rawTime);
            const clamped = Math.max(0, Math.min(processed, processedDuration));
            return manager.mapToSourceTime(clamped);
        },

        isTimeReachable(rawTime: number): boolean {
            const processed = manager.mapToProcessedTime(rawTime);
            const back = manager.mapToSourceTime(processed);
            return Math.abs(back - rawTime) < 1e-6;
        },
    };

    return manager;
}

function buildEditsJson(states: StructuralProcessorState[]): string {
    return JSON.stringify(
        states.map((s) => ({
            type: s.type,
            enabled: s.enabled,
            parameters: s.parameters,
        })),
    );
}
