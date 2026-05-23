// libs/audio-plugin-sdk/js/src/plugin-loader.ts
import processorSrc from './worklet-processor.js?raw';
import type {
  AudioPlugin,
  AudioPluginAnalyzer,
  AudioPluginDescriptor,
  AudioPluginHandle,
  AudioPluginRenderer,
} from './types';
import { WasmAnalyzer } from './analyzer';

function toPlainObject<T extends Record<string, unknown>>(input: T): T {
  return Object.fromEntries(Object.entries(input)) as T;
}

export async function loadPluginDescriptor(url: string): Promise<AudioPluginDescriptor> {
  const res = await fetch(`${url}.json`);
  if (!res.ok) throw new Error(`Failed to fetch plugin descriptor at ${url}.json: ${res.statusText}`);
  return res.json() as Promise<AudioPluginDescriptor>;
}

// ── AudioPluginHandleImpl ────────────────────────────────────────────────────

class AudioPluginHandleImpl implements AudioPluginHandle {
  private readonly _node: AudioWorkletNode;
  private _pendingGetParam = new Map<string, (v: number) => void>();

  constructor(node: AudioWorkletNode) {
    this._node = node;
  }

  /** Called by AudioPluginImpl when a parameter_value message arrives from the worklet. */
  _receiveParamValue(id: string, value: number): void {
    const resolve = this._pendingGetParam.get(id);
    if (resolve) {
      this._pendingGetParam.delete(id);
      resolve(value);
    }
  }

  dispose(): void {
    this._node.disconnect();
  }

  getParameter(id: string): Promise<number> {
    return new Promise<number>((resolve) => {
      this._pendingGetParam.set(id, resolve);
      this._node.port.postMessage({ type: 'get_parameter', id });
    });
  }

  setParameter(id: string, value: number): void {
    this._node.port.postMessage({ type: 'set_parameter', id, value });
  }
}

// ── AudioPluginAnalyzerImpl ──────────────────────────────────────────────────

class AudioPluginAnalyzerImpl implements AudioPluginAnalyzer {
  readonly audioUrl: string;

  private readonly _wasmUrl: string;
  private _analyzing = false;

  constructor(wasmUrl: string, audioUrl: string) {
    this._wasmUrl = wasmUrl;
    this.audioUrl = audioUrl;
  }

  async runAnalysis(params: Record<string, number>): Promise<Record<string, number> | null> {
    if (this._analyzing) return null;
    this._analyzing = true;
    let result = null;
    try {
      result = await this._doAnalysis(params);
    } catch (err) {
      console.error('[AudioPluginAnalyzer] analysis failed:', err);
    }
    this._analyzing = false;
    return result;
  }

  private async _doAnalysis(params: Record<string, number>): Promise<Record<string, number>> {
    const analyzer = new WasmAnalyzer(this._wasmUrl);
    const result = await analyzer.run(this.audioUrl, JSON.stringify(params));
    const json = JSON.parse(result);
    return json;
  }
}

// ── AudioPluginImpl ──────────────────────────────────────────────────────────

class AudioPluginImpl implements AudioPlugin {
  readonly descriptor: AudioPluginDescriptor;
  handle: AudioPluginHandleImpl | null = null;
  analyzer: AudioPluginAnalyzerImpl | null;
  renderer: AudioPluginRenderer | null;
  params: Record<string, number>;

  private readonly _wasmUrl: string;
  private _enabled = true;
  private _canvases: { canvasId: string; canvas: HTMLCanvasElement }[] = [];
  private _rafId: number | null = null;
  private _processorName: string;

  constructor(
    descriptor: AudioPluginDescriptor,
    wasmUrl: string,
    analyzer: AudioPluginAnalyzerImpl | null,
    renderer: AudioPluginRenderer | null,
    initialParams: Record<string, number>,
  ) {
    this.descriptor = descriptor;
    this._wasmUrl = wasmUrl;
    this.analyzer = analyzer;
    this.renderer = renderer;
    this.params = { ...initialParams };
    this._processorName = `ap-${descriptor.id}-${crypto.randomUUID()}`;
  }

  get enabled(): boolean {
    return this._enabled;
  }
  set enabled(enabled: boolean) {
    if (enabled == this._enabled) return;
    this._enabled = enabled;
    this.handle?.['_node'].port.postMessage({ type: 'set_enabled', enabled });
  }

  async createNode(ctx: AudioContext): Promise<AudioWorkletNode> {
    const wasmRes = await fetch(this._wasmUrl);
    if (!wasmRes.ok) throw new Error(`Failed to fetch plugin WASM at ${this._wasmUrl}: ${wasmRes.statusText}`);
    const wasmBytes: ArrayBuffer = await wasmRes.arrayBuffer();

    const source = `${processorSrc}\nregisterProcessor('${this._processorName}', AudioPluginProcessor);`;
    const blob = new Blob([source], { type: 'application/javascript' });
    const blobUrl = URL.createObjectURL(blob);
    await ctx.audioWorklet.addModule(blobUrl);

    const node = new AudioWorkletNode(ctx, this._processorName);

    return new Promise<AudioWorkletNode>((resolve) => {
      node.port.onmessage = (e) => {
        if (e.data.type === 'ready') {
          const handle = new AudioPluginHandleImpl(node);
          this.handle = handle;
          node.port.onmessage = (e) => this._handleMessage(e.data);
          if (this.renderer) this._startRaf(node);
          resolve(node);
        }
      };
      node.port.postMessage({ type: 'init', wasmBytes, initialParams: toPlainObject(this.params), enabled: this._enabled });
    });
  }

  initRenderer(canvases: HTMLCanvasElement[]): void {
    const canvasParams = this.descriptor.parameters.filter((p) => p.type === 'canvas');
    this._canvases = canvases.map((canvas, i) => ({
      canvasId: canvasParams[i]?.id ?? `canvas_${i}`,
      canvas,
    }));
    if (this.renderer && this._canvases.length > 0) {
      this.renderer.init(this._canvases);
      if (this.handle) this._startRaf((this.handle as AudioPluginHandleImpl)['_node']);
    }
  }

  async readParams(): Promise<Record<string, number>> {
    if (!this.handle) return { ...this.params };
    const paramIds = this.descriptor.parameters
      .filter((p) => p.type === 'float' || p.type === 'bool')
      .map((p) => p.id);
    const entries = await Promise.all(
      paramIds.map(async (id) => [id, await this.handle!.getParameter(id)] as [string, number]),
    );
    for (const [id, value] of entries) {
      this.params[id] = value;
    }
    return { ...this.params };
  }

  writeParams(params: Record<string, number>): void {
    for (const [id, value] of Object.entries(params)) {
      this.params[id] = value;
      this.handle?.setParameter(id, value);
    }
  }

  private _handleMessage(msg: Record<string, unknown>): void {
    switch (msg.type) {
      case 'parameter_value':
        this.handle?._receiveParamValue(msg.id as string, msg.value as number);
        break;
      case 'snapshot':
        for (const { canvasId, canvas } of this._canvases) {
          this.renderer?.render(msg.buffer as ArrayBuffer, canvasId, canvas);
        }
        break;
    }
  }

  private _startRaf(node: AudioWorkletNode): void {
    if (this._rafId !== null) cancelAnimationFrame(this._rafId);
    const tick = () => {
      node.port.postMessage({ type: 'request_snapshot' });
      this._rafId = requestAnimationFrame(tick);
    };
    this._rafId = requestAnimationFrame(tick);
  }
}

// ── createPlugin factory ──────────────────────────────────────────────────────

export async function createPlugin(
  pluginUrl: string,
  audioUrl: string,
  initialParams: Record<string, number>,
): Promise<AudioPlugin> {
  const descriptor = await loadPluginDescriptor(pluginUrl);
  const wasmUrl = `${pluginUrl}.wasm`;

  let renderer: AudioPluginRenderer | null = null;
  if (descriptor.parameters.some((p) => p.type === 'canvas')) {
    const mod = await import(/* @vite-ignore */ `${pluginUrl}.renderer.js`);
    renderer = mod.default ?? mod.renderer;
  }

  const analyzer: AudioPluginAnalyzerImpl | null =
    descriptor.mode === 'analyzed' ? new AudioPluginAnalyzerImpl(wasmUrl, audioUrl) : null;

  // Merge descriptor defaults under initialParams (initialParams wins)
  const defaults: Record<string, number> = {};
  for (const p of descriptor.parameters) {
    if ((p.type === 'float' || p.type === 'bool') && !(p.id in initialParams)) {
      defaults[p.id] = p.type === 'bool' ? (p.default ? 1 : 0) : p.default;
    }
  }
  const mergedParams = { ...defaults, ...initialParams };

  return new AudioPluginImpl(descriptor, wasmUrl, analyzer, renderer, mergedParams);
}
