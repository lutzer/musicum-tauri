import { describe, it, expect, vi, beforeEach } from 'vitest';

// Mock WASM loader before importing manager
const mockWasm = {
    memory: { buffer: new ArrayBuffer(65536) },
    __sp_alloc: vi.fn((_size: number) => 0),
    __sp_free: vi.fn(),
    __sp_apply_chain: vi.fn(),
    __sp_result_ptr: vi.fn(() => 0),
    __sp_result_len: vi.fn(() => 4), // 1 f32 sample
    __sp_descriptors_init: vi.fn(),
    __sp_descriptors_ptr: vi.fn(() => 0),
    __sp_descriptors_len: vi.fn(() => 0),
    __sp_validate_edit: vi.fn(() => 1),
    __sp_map_time_forward: vi.fn((_eP: number, _eL: number, t: number) => t),
    __sp_map_time_back: vi.fn((_eP: number, _eL: number, t: number) => t),
};

vi.mock('./loader', async () => {
    const actual = await vi.importActual<typeof import('./loader')>('./loader');
    return {
        ...actual,
        getWasm: vi.fn().mockResolvedValue(mockWasm),
        writeString: vi.fn().mockReturnValue([0, 0]),
        writeFloat32Array: vi.fn().mockReturnValue([0, 4]),
        readFloat32Array: vi.fn().mockReturnValue(new Float32Array([0])),
        audioBufferToInterleaved: vi.fn().mockReturnValue(new Float32Array(4)),
        interleavedToAudioBuffer: vi.fn().mockReturnValue({
            duration: 2.0,
            length: 88200,
            numberOfChannels: 1,
            sampleRate: 44100,
            getChannelData: vi.fn().mockReturnValue(new Float32Array(88200)),
        } as unknown as AudioBuffer),
    };
});

function makeMockBuffer(duration = 2.0): AudioBuffer {
    return {
        duration,
        length: Math.floor(duration * 44100),
        numberOfChannels: 1,
        sampleRate: 44100,
        getChannelData: vi.fn().mockReturnValue(new Float32Array(Math.floor(duration * 44100))),
    } as unknown as AudioBuffer;
}

describe('createStructuralProcessorManager', () => {
    let createStructuralProcessorManager: typeof import('./manager').createStructuralProcessorManager;

    beforeEach(async () => {
        vi.clearAllMocks();
        vi.resetModules();
        const mod = await import('./manager');
        createStructuralProcessorManager = mod.createStructuralProcessorManager;
    });

    it('calls onChange after sync with a buffer', async () => {
        const manager = createStructuralProcessorManager();
        const onChange = vi.fn();
        manager.onChange = onChange;
        await manager.sync([], makeMockBuffer());
        expect(onChange).toHaveBeenCalledOnce();
    });

    it('does not call onChange when buffer is null', async () => {
        const manager = createStructuralProcessorManager();
        const onChange = vi.fn();
        manager.onChange = onChange;
        await manager.sync([], null);
        expect(onChange).not.toHaveBeenCalled();
    });

    it('does nothing after dispose', async () => {
        const manager = createStructuralProcessorManager();
        const onChange = vi.fn();
        manager.onChange = onChange;
        manager.dispose();
        await manager.sync([], makeMockBuffer());
        expect(onChange).not.toHaveBeenCalled();
    });

    it('identity time mapping when WASM not yet loaded', () => {
        const manager = createStructuralProcessorManager();
        expect(manager.mapToProcessedTime(1.5)).toBe(1.5);
        expect(manager.mapToSourceTime(1.5)).toBe(1.5);
    });
});
