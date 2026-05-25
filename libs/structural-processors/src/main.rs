mod processors;

use std::io::{self, Read};

use structural_processor_sdk::{
    AudioSource, VecAudioSource, build_chain,
    chain::{descriptors_json, Edit},
};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};

fn read_wav(path: &str) -> (Vec<f32>, u32, u16) {
    let mut reader = WavReader::open(path).expect("failed to open input WAV");
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap()).collect(),
        SampleFormat::Int => {
            let scale = (1_u32 << (spec.bits_per_sample as u32 - 1)) as f32;
            reader.samples::<i32>().map(|s| s.unwrap() as f32 / scale).collect()
        }
    };
    (samples, spec.sample_rate, spec.channels)
}

fn write_wav(path: &str, samples: &[f32], sample_rate: u32, channels: u16) {
    let spec = WavSpec { channels, sample_rate, bits_per_sample: 32, sample_format: SampleFormat::Float };
    let mut w = WavWriter::create(path, spec).expect("failed to create output WAV");
    for &s in samples { w.write_sample(s).unwrap(); }
    w.finalize().unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() == 2 && args[1] == "--descriptors" {
        println!("{}", descriptors_json(&structural_processors::registry()));
        return;
    }

    if args.len() != 3 {
        eprintln!("Usage: structural-processor <input.wav> <output.wav>");
        eprintln!("       structural-processor --descriptors");
        std::process::exit(1);
    }

    let (samples, sample_rate, channels) = read_wav(&args[1]);
    let total = samples.len();
    let source: Box<dyn AudioSource> = Box::new(VecAudioSource::new(samples, sample_rate, channels));

    let mut edits_json = String::new();
    io::stdin().read_to_string(&mut edits_json).expect("failed to read stdin");
    let edits: Vec<Edit> = serde_json::from_str(&edits_json).expect("invalid edits JSON");

    let mut chain = build_chain(source, &edits, &structural_processors::registry());
    let output = chain.read_at(0.0, total);
    write_wav(&args[2], &output, sample_rate, channels);
}
