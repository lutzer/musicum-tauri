export type StructuralProcessorParameterDescriptor =
    | { type: 'time'; id: string; name: string; default: number }
    | { type: 'int'; id: string; name: string; default: number; min: number; max: number };

export interface StructuralProcessorDescriptor {
    id: string;
    name: string;
    parameters: StructuralProcessorParameterDescriptor[];
}

export interface StructuralProcessorState {
    id: string; // instance UUID
    type: string; // maps to processor id: 'trim', 'cut', etc.
    enabled: boolean;
    parameters: Record<string, number>;
}

export interface StructuralProcessorManager {
    onChange?: (processed: AudioBuffer) => void;
    sync(states: StructuralProcessorState[], rawBuffer: AudioBuffer | null): Promise<void>;
    dispose(): void;
    /** Returns a minimal shim for validation. Descriptor data comes from static JSON. */
    getModule(
        type: string,
    ):
        | {
              descriptor: StructuralProcessorDescriptor;
              validate(params: Record<string, number>): boolean;
          }
        | undefined;
    mapToSourceTime(processedTime: number): number;
    mapToProcessedTime(rawTime: number): number;
    clampToAllowedTime(rawTime: number): number;
    isTimeReachable(rawTime: number): boolean;
}
