import { createPlugin } from "./plugin-loader";
import type { AudioPlugin, AudioPluginManager, AudioPluginState } from "./types";


export function createPluginManager(audioUrl: string): AudioPluginManager {
    return new AudioPluginManagerImpl(audioUrl)
}

class AudioPluginManagerImpl implements AudioPluginManager {
  audioUrl: string;
  private _chain: AudioPluginChain | undefined;
  private _plugins: Map<string, AudioPlugin> = new Map();

  constructor(audioUrl: string) {
    this.audioUrl = audioUrl;
  }

  get plugins() { return this._plugins; }

  get chainConnected() { return this._chain !== undefined; }

  async sync(changedStates: AudioPluginState[]): Promise<void> {
    const prevIds = new Set(this._plugins.keys());
    const nextIds = new Set(changedStates.map((e) => e.id));

    let edits: ChainEdit[] = [];

    // Remove plugins for deleted edits
    for (const [id, plugin] of this._plugins) {
      if (!nextIds.has(id)) {
        plugin.handle?.dispose();
        this._plugins.delete(id);
        edits.push({ action: 'remove', id: id})
      }
    }

    // Add new plugins; update params for existing ones
    for (const state of changedStates) {
      if (!prevIds.has(state.id)) {
        const plugin = await createPlugin(`/plugins/${state.type}`, this.audioUrl, state.parameters);
        plugin.enabled = state.enabled;
        if (plugin.descriptor.mode === 'offline') continue;
        this._plugins.set(state.id, plugin);
        edits.push({ action: 'add', id: state.id})
      } else {
        const plugin = this._plugins.get(state.id);
        if (!plugin) continue;
        const changed: Record<string, number> = {};
        for (const [paramId, value] of Object.entries(state.parameters)) {
          if (plugin.params[paramId] !== value) {
            changed[paramId] = value;
          }
        }
        if (Object.keys(changed).length > 0) {
          plugin.writeParams(changed);
        }
        // update enabled
        plugin.enabled = state.enabled
      }
    }

    //update chain if something structural changed
    if (edits.length > 0) {
      this._chain?.sync(edits);
    }
  }

  getPlugin(editId: string): AudioPlugin | undefined {
    return this._plugins.get(editId);
  }

  dispose(): void {
    this._chain?.dispose()
    for (const plugin of this._plugins.values()) {
      plugin.handle?.dispose();
    }
    this._plugins.clear();
  }

  async createWorkletChain(ctx: AudioContext, source: AudioNode) : Promise<void> {
    if (this._chain) {
      // Reuse existing worklet nodes — just swap the source and rewire.
      // Creating new nodes would re-register worklet processors in the same
      // AudioContext, which throws NotSupportedError.
      this._chain.reconnectSource(source);
      return;
    }
    this._chain = new AudioPluginChain(ctx, source, this);
    const edits : ChainEdit[] = Array.from(this._plugins)
      .map(([key,_]) => {
        return { id: key, action: 'add' }
      })
    await this._chain.sync(edits);
  }
}

interface ChainNode {
  node: AudioWorkletNode;
  plugin: AudioPlugin;
}

interface ChainEdit {
  id: string,
  action: 'add' | 'remove'
}

export class AudioPluginChain {
  private ctx: AudioContext;
  private source: AudioNode;
  private manager: AudioPluginManager;
  private nodes: Map<string, ChainNode> = new Map();

  constructor(ctx: AudioContext, source: AudioNode, manager: AudioPluginManager) {
    this.ctx = ctx;
    this.source = source;
    this.manager = manager;
  }

  async sync(edits: ChainEdit[]): Promise<void> {
    // remove edits
    for (const edit of edits) {
      if (edit.action === 'remove') {
        const entry = this.nodes.get(edit.id);
        if (entry) {
          entry.node.disconnect();
          this.nodes.delete(edit.id);
        }
      }
    }

    // add edits
    for (const edit of edits) {
      if (edit.action === 'add' && !this.nodes.has(edit.id)) {
        const plugin = this.manager.plugins.get(edit.id);
        if (!plugin) continue;
        const node = await plugin.createNode(this.ctx);
        this.nodes.set(edit.id, { node, plugin });
      }
    }

    // reconnect 
    this.reconnect();
  }

  /** Swap the upstream source and rewire the graph without recreating worklet nodes. */
  reconnectSource(newSource: AudioNode): void {
    this.source.disconnect();
    this.source = newSource;
    this.reconnect();
  }

  private reconnect(): void {
    for (const { node } of this.nodes.values()) {
      node.disconnect();
    }

    let prev: AudioNode = this.source;
    for (const { node } of this.nodes.values()) {
      prev.connect(node);
      prev = node;
    }
    prev.connect(this.ctx.destination);
  }

  dispose(): void {
    this.source.disconnect();
    for (const { node } of this.nodes.values()) {
      node.disconnect();
    }
    this.nodes.clear();
  }
}
