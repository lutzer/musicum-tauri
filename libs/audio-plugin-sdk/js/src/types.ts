export type AudioPluginMode = 'realtime' | 'offline' | 'analyzed';

export type AudioPluginParameterDescriptor =
  | { type: 'float'; id: string; name: string; min: number; max: number; default: number; step?: number; unit?: string; disabled?: boolean; hidden?: boolean }
  | { type: 'bool';  id: string; name: string; default: boolean; disabled?: boolean; hidden?: boolean }
  | { type: 'action'; id: string; name: string, disabled?: boolean }
  | { type: 'canvas'; id: string; name: string; aspect_ratio?: number, disabled?: boolean };

export interface AudioPluginDescriptor {
  id: string;
  name: string;
  version: string;
  mode: AudioPluginMode;
  parameters: AudioPluginParameterDescriptor[];
}

export interface AudioPlugin {
  descriptor: AudioPluginDescriptor;
  handle: AudioPluginHandle | null;
  analyzer: AudioPluginAnalyzer | null;
  renderer: AudioPluginRenderer | null;
  params: Record<string, number>;
  
  get enabled(): boolean;
  set enabled(enabled: boolean);

  createNode(ctx: AudioContext): Promise<AudioWorkletNode>;
  initRenderer(canvases: HTMLCanvasElement[]): void;
  readParams(): Promise<Record<string, number>>;
  writeParams(params: Record<string, number>): void;
}

export interface AudioPluginAnalyzer {
  audioUrl: string;
  runAnalysis(params: Record<string, number>): Promise<Record<string,number> | null>;
}

export interface AudioPluginHandle {
  dispose(): void;
  getParameter(id: string): Promise<number>;
  setParameter(id: string, value: number): void;
}

export interface AudioPluginRenderer {
  init(canvasElements: { canvasId: string; canvas: HTMLCanvasElement }[]): Promise<void>;
  render(data: ArrayBuffer, canvasId: string, canvas: HTMLCanvasElement): void;
}

export interface AudioPluginManager {
    audioUrl: string;
    
    get plugins(): Map<string, AudioPlugin>

    sync(changedStates: AudioPluginState[]): Promise<void>
    createWorkletChain(ctx: AudioContext, source: AudioNode) : Promise<void>;
    dispose(): void;

    get chainConnected(): boolean
}

export interface AudioPluginState {
	id: string;
  type: string;
	enabled: boolean;
	parameters: Record<string, number>;
}
