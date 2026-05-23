type AudioData = {
  numChannels: number;
  sampleRate: number;
  samples: Float32Array[]; // per channel
};

/**
 * WebAudio decode (supports MP3, OGG, WAV)
 */
async function decodeWithWebAudio(buffer: ArrayBuffer): Promise<AudioData> {
  const ctx = new OfflineAudioContext(1,1,44100);
  const audioBuffer = await ctx.decodeAudioData(buffer.slice(0));

  const samples: Float32Array[] = [];

  for (let ch = 0; ch < audioBuffer.numberOfChannels; ch++) {
    samples.push(audioBuffer.getChannelData(ch));
  }

  return {
    numChannels: audioBuffer.numberOfChannels,
    sampleRate: audioBuffer.sampleRate,
    samples,
  };
}

/**
 * 🔥 Unified API
 */
export async function parseAudio(buffer: ArrayBuffer): Promise<AudioData> {
  try {
    return decodeWithWebAudio(buffer);
  } catch {
    throw new Error("Unsupported audio format");
  }
}