// libs/audio-plugin-sdk/js/src/plugin-loader.test.ts
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// ── shared helpers ───────────────────────────────────────────────────────────

/** Flush all pending microtasks by deferring to the next macrotask. */
const flushPromises = () => new Promise<void>((r) => setTimeout(r, 0));

const REALTIME_DESCRIPTOR = {
  id: 'gain', name: 'Gain', version: '1.0.0', mode: 'realtime' as const,
  parameters: [
    { type: 'float' as const, id: 'gain', name: 'Gain', min: 0, max: 2, default: 1 },
  ],
};

const ANALYZED_DESCRIPTOR = {
  id: 'normalize', name: 'Normalize', version: '1.0.0', mode: 'analyzed' as const,
  parameters: [
    { type: 'float' as const, id: 'peak', name: 'Peak', min: 0, max: 1, default: 0.9, computed: true },
  ],
};

function makeMockPort() {
  let handler: ((e: MessageEvent) => void) | null = null;
  const posted: unknown[] = [];
  return {
    port: {
      get onmessage() { return handler; },
      set onmessage(fn: any) { handler = fn; },
      postMessage: vi.fn((msg) => { posted.push(msg); }),
    },
    posted,
    simulateMessage: (data: unknown) => handler?.({ data } as MessageEvent),
  };
}

// ── AudioPluginHandleImpl ────────────────────────────────────────────────────

describe('AudioPluginHandleImpl', () => {
  // We test AudioPluginHandleImpl indirectly via AudioPlugin._handle after createNode.
  // Direct construction is internal; we use createPlugin to get an AudioPlugin, then createNode.

  let mockNode: ReturnType<typeof makeMockPort>;

  beforeEach(() => {
    mockNode = makeMockPort();
    vi.stubGlobal('fetch', vi.fn().mockImplementation((url: string) => {
      if (url.endsWith('.json')) {
        return Promise.resolve({ ok: true, json: () => Promise.resolve(REALTIME_DESCRIPTOR) });
      }
      return Promise.resolve({ ok: true, arrayBuffer: () => Promise.resolve(new ArrayBuffer(0)) });
    }));
    vi.stubGlobal('AudioWorkletNode', vi.fn(() => mockNode));
    vi.stubGlobal('URL', { createObjectURL: vi.fn(() => 'blob:test') });
    vi.stubGlobal('Blob', vi.fn());
    vi.stubGlobal('crypto', { randomUUID: vi.fn(() => 'test-uuid') });
  });
  afterEach(() => vi.unstubAllGlobals());

  async function makePluginWithNode() {
    const { createPlugin } = await import('./plugin-loader');
    const plugin = await createPlugin('/gain', '/audio.wav', { gain: 1.0 });
    const mockCtx = { audioWorklet: { addModule: vi.fn().mockResolvedValue(undefined) } };
    const nodePromise = plugin.createNode(mockCtx as unknown as AudioContext);
    await flushPromises();
    mockNode.simulateMessage({ type: 'ready' });
    await nodePromise;
    return { plugin };
  }

  it('handle is null before createNode', async () => {
    const { createPlugin } = await import('./plugin-loader');
    const plugin = await createPlugin('/gain', '/audio.wav', {});
    expect(plugin.handle).toBeNull();
  });

  it('handle is assigned after createNode', async () => {
    const { plugin } = await makePluginWithNode();
    expect(plugin.handle).not.toBeNull();
  });

  it('setParameter sends set_parameter to worklet', async () => {
    const { plugin } = await makePluginWithNode();
    plugin.handle!.setParameter('gain', 1.5);
    expect(mockNode.port.postMessage).toHaveBeenCalledWith(
      expect.objectContaining({ type: 'set_parameter', id: 'gain', value: 1.5 })
    );
  });

  it('getParameter resolves when worklet replies with parameter_value', async () => {
    const { plugin } = await makePluginWithNode();
    const promise = plugin.handle!.getParameter('gain');
    mockNode.simulateMessage({ type: 'parameter_value', id: 'gain', value: 0.75 });
    expect(await promise).toBe(0.75);
  });

  it('dispose disconnects the node', async () => {
    const { plugin } = await makePluginWithNode();
    const disconnect = vi.fn();
    (mockNode as any).disconnect = disconnect;
    plugin.handle!.dispose();
    expect(disconnect).toHaveBeenCalled();
  });
});

// ── AudioPluginImpl – params and createNode ──────────────────────────────────

describe('AudioPluginImpl._params', () => {
  let mockNode: ReturnType<typeof makeMockPort>;

  beforeEach(() => {
    mockNode = makeMockPort();
    vi.stubGlobal('fetch', vi.fn().mockImplementation((url: string) => {
      if (url.endsWith('.json'))
        return Promise.resolve({ ok: true, json: () => Promise.resolve(REALTIME_DESCRIPTOR) });
      return Promise.resolve({ ok: true, arrayBuffer: () => Promise.resolve(new ArrayBuffer(0)) });
    }));
    vi.stubGlobal('AudioWorkletNode', vi.fn(() => mockNode));
    vi.stubGlobal('URL', { createObjectURL: vi.fn(() => 'blob:test') });
    vi.stubGlobal('Blob', vi.fn());
    vi.stubGlobal('crypto', { randomUUID: vi.fn(() => 'test-uuid') });
  });
  afterEach(() => vi.unstubAllGlobals());

  it('_params is initialised from initialParams passed to createPlugin', async () => {
    const { createPlugin } = await import('./plugin-loader');
    const plugin = await createPlugin('/gain', '/audio.wav', { gain: 0.5 });
    expect(plugin.params).toEqual({ gain: 0.5 });
  });

  it('writeParams merges into _params without a node', async () => {
    const { createPlugin } = await import('./plugin-loader');
    const plugin = await createPlugin('/gain', '/audio.wav', { gain: 1.0 });
    plugin.writeParams({ gain: 0.3 });
    expect(plugin.params.gain).toBe(0.3);
  });

  it('writeParams calls setParameter on handle when handle exists', async () => {
    const { createPlugin } = await import('./plugin-loader');
    const plugin = await createPlugin('/gain', '/audio.wav', { gain: 1.0 });
    const mockCtx = { audioWorklet: { addModule: vi.fn().mockResolvedValue(undefined) } };
    const nodePromise = plugin.createNode(mockCtx as unknown as AudioContext);
    await flushPromises();
    mockNode.simulateMessage({ type: 'ready' });
    await nodePromise;

    plugin.writeParams({ gain: 0.8 });
    expect(mockNode.port.postMessage).toHaveBeenCalledWith(
      expect.objectContaining({ type: 'set_parameter', id: 'gain', value: 0.8 })
    );
  });

  it('readParams returns _params when no handle exists', async () => {
    const { createPlugin } = await import('./plugin-loader');
    const plugin = await createPlugin('/gain', '/audio.wav', { gain: 0.6 });
    const result = await plugin.readParams();
    expect(result).toEqual({ gain: 0.6 });
  });

  it('readParams reads live values from worklet when handle exists and updates _params', async () => {
    const { createPlugin } = await import('./plugin-loader');
    const plugin = await createPlugin('/gain', '/audio.wav', { gain: 1.0 });
    const mockCtx = { audioWorklet: { addModule: vi.fn().mockResolvedValue(undefined) } };
    const nodePromise = plugin.createNode(mockCtx as unknown as AudioContext);
    await flushPromises();
    mockNode.simulateMessage({ type: 'ready' });
    await nodePromise;

    const readPromise = plugin.readParams();
    // Worklet replies for each param in descriptor
    mockNode.simulateMessage({ type: 'parameter_value', id: 'gain', value: 1.23 });
    const result = await readPromise;
    expect(result.gain).toBe(1.23);
    expect(plugin.params.gain).toBe(1.23);
  });

  it('createNode sends _params as initialParams in the init message', async () => {
    const { createPlugin } = await import('./plugin-loader');
    const plugin = await createPlugin('/gain', '/audio.wav', { gain: 0.7 });
    const mockCtx = { audioWorklet: { addModule: vi.fn().mockResolvedValue(undefined) } };
    const nodePromise = plugin.createNode(mockCtx as unknown as AudioContext);
    await flushPromises();
    mockNode.simulateMessage({ type: 'ready' });
    await nodePromise;

    const initCall = (mockNode.port.postMessage as ReturnType<typeof vi.fn>).mock.calls
      .find((c) => c[0].type === 'init');
    expect(initCall).toBeDefined();
    expect(initCall![0].initialParams).toEqual({ gain: 0.7 });
  });
});

// ── AudioPluginAnalyzerImpl ──────────────────────────────────────────────────

describe('AudioPluginAnalyzerImpl', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn().mockImplementation((url: string) => {
      if (url.endsWith('.json'))
        return Promise.resolve({ ok: true, json: () => Promise.resolve(ANALYZED_DESCRIPTOR) });
      return Promise.resolve({ ok: true, arrayBuffer: () => Promise.resolve(new ArrayBuffer(0)) });
    }));
    vi.stubGlobal('crypto', { randomUUID: vi.fn(() => 'test-uuid') });
  });
  afterEach(() => vi.unstubAllGlobals());

  it('_analyzer is null for realtime plugins', async () => {
    vi.stubGlobal('fetch', vi.fn().mockImplementation((url: string) => {
      if (url.endsWith('.json'))
        return Promise.resolve({ ok: true, json: () => Promise.resolve(REALTIME_DESCRIPTOR) });
      return Promise.resolve({ ok: true, arrayBuffer: () => Promise.resolve(new ArrayBuffer(0)) });
    }));
    const { createPlugin } = await import('./plugin-loader');
    const plugin = await createPlugin('/gain', '/audio.wav', {});
    expect(plugin.analyzer).toBeNull();
  });

  it('_analyzer is created for analyzed plugins', async () => {
    const { createPlugin } = await import('./plugin-loader');
    const plugin = await createPlugin('/normalize', '/audio.wav', {});
    expect(plugin.analyzer).not.toBeNull();
    expect(plugin.analyzer!.audioUrl).toBe('/audio.wav');
  });
});

// ── loadPluginDescriptor ──────────────────────────────────────────────────────

describe('loadPluginDescriptor', () => {
  afterEach(() => vi.unstubAllGlobals());

  it('fetches descriptor JSON and returns it', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
      ok: true,
      json: () => Promise.resolve(REALTIME_DESCRIPTOR),
    }));
    const { loadPluginDescriptor } = await import('./plugin-loader');
    const desc = await loadPluginDescriptor('/gain');
    expect(desc.id).toBe('gain');
    expect(desc.mode).toBe('realtime');
    expect(fetch).toHaveBeenCalledWith('/gain.json');
  });

  it('throws when fetch fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false, statusText: 'Not Found' }));
    const { loadPluginDescriptor } = await import('./plugin-loader');
    await expect(loadPluginDescriptor('/missing')).rejects.toThrow('Not Found');
  });
});
