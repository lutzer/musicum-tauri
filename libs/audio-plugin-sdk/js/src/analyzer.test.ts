// libs/audio-plugin-sdk/js/src/analyzer.test.ts
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

vi.mock('./audio-parser', () => ({
  parseAudio: vi.fn(),
}));

import { WasmAnalyzer } from './analyzer';
import { parseAudio } from './audio-parser';

// ── WasmAnalyzer tests ──────────────────────────────────────────────────────

describe('WasmAnalyzer', () => {
  /**
   * Build a mock WebAssembly.Instance whose __aa_* exports record calls.
   * The result JSON is pre-written at a fixed offset in the memory buffer.
   */
  function makeMockInstance(resultJson = '{"peak":0.5}') {
    const mem = { buffer: new ArrayBuffer(65536) };
    const calls: string[] = [];
    const resultBytes = new TextEncoder().encode(resultJson);
    const RESULT_OFFSET = 200;
    new Uint8Array(mem.buffer, RESULT_OFFSET, resultBytes.length).set(resultBytes);

    const exports: Record<string, unknown> = {
      memory: mem,
      __aa_alloc: (size: number) => {
        calls.push(`alloc(${size})`);
        return 8; // fixed ptr for testing
      },
      __aa_free: (ptr: number, len: number) => { calls.push(`free(${ptr},${len})`); },
      __aa_reset: () => { calls.push('reset'); },
      __aa_init: (ptr: number, len: number) => { calls.push(`init(${ptr},${len})`); },
      __aa_analyze: (_ptr: number, len: number) => { calls.push(`analyze(${len}samples)`); },
      __aa_result_ptr: () => { calls.push('result_ptr'); return RESULT_OFFSET; },
      __aa_result_len: () => { calls.push('result_len'); return resultBytes.length; },
    };

    return { instance: { exports } as unknown as WebAssembly.Instance, calls };
  }

  beforeEach(() => {
    vi.stubGlobal('WebAssembly', {
      ...WebAssembly,
      instantiate: vi.fn(),
    });
    vi.mocked(parseAudio).mockResolvedValue({
      numChannels: 1,
      sampleRate: 44100,
      samples: [new Float32Array([0.5, -0.5, 0.75])],
    });
  });

  afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); });

  it('run() calls reset, init, analyze, result_ptr, result_len', async () => {
    const { instance, calls } = makeMockInstance('{"peak":0.75}');
    (WebAssembly.instantiate as ReturnType<typeof vi.fn>).mockResolvedValue({ instance });
    vi.stubGlobal('fetch', vi.fn()
      .mockResolvedValueOnce({ ok: true, arrayBuffer: () => Promise.resolve(new ArrayBuffer(0)) })
      .mockResolvedValueOnce({ ok: true, arrayBuffer: () => Promise.resolve(new ArrayBuffer(0)) }),
    );

    const analyzer = new WasmAnalyzer('/plugin.wasm');
    const result = await analyzer.run('/audio.wav', '{"target_dbfs":-3}');

    expect(calls).toContain('reset');
    expect(calls.some(c => c.startsWith('init'))).toBe(true);
    expect(calls.some(c => c.startsWith('analyze'))).toBe(true);
    expect(calls).toContain('result_ptr');
    expect(calls).toContain('result_len');
    expect(result).toBe('{"peak":0.75}');
  });

  it('run() returns result JSON string', async () => {
    const { instance } = makeMockInstance('{"loudness":-14.0}');
    (WebAssembly.instantiate as ReturnType<typeof vi.fn>).mockResolvedValue({ instance });
    vi.mocked(parseAudio).mockResolvedValue({
      numChannels: 2,
      sampleRate: 48000,
      samples: [new Float32Array([0.1, 0.2]), new Float32Array([0.3, 0.4])],
    });
    vi.stubGlobal('fetch', vi.fn()
      .mockResolvedValueOnce({ ok: true, arrayBuffer: () => Promise.resolve(new ArrayBuffer(0)) })
      .mockResolvedValueOnce({ ok: true, arrayBuffer: () => Promise.resolve(new ArrayBuffer(0)) }),
    );

    const analyzer = new WasmAnalyzer('/normalize.wasm');
    const result = await analyzer.run('/audio.wav', '{}');
    expect(result).toBe('{"loudness":-14.0}');
  });

  it('run() throws when WASM fetch fails', async () => {
    vi.stubGlobal('fetch', vi.fn()
      .mockResolvedValueOnce({ ok: false, statusText: 'Not Found' }),
    );
    const analyzer = new WasmAnalyzer('/missing.wasm');
    await expect(analyzer.run('/audio.wav', '{}')).rejects.toThrow();
  });
});
