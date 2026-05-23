mod processors;

use std::io::{self, Read};

use processors::{
    crop::CropProcessor, cut::CutProcessor, slice::SliceProcessor, trim::TrimProcessor,
};
use structural_processor_sdk::{
    chain::{apply_chain, descriptors_json, Edit},
    ProcessorEntry,
};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};

fn registry() -> Vec<ProcessorEntry> {
    vec![
        ProcessorEntry::of::<TrimProcessor>(),
        ProcessorEntry::of::<CutProcessor>(),
        ProcessorEntry::of::<SliceProcessor>(),
        ProcessorEntry::of::<CropProcessor>(),
    ]
}

fn read_wav(path: &str) -> (Vec<f32>, u32, u16) {
    let mut reader = WavReader::open(path).expect("failed to open input WAV");
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        SampleFormat::Float => reader
            .samples::<f32>()
            .map(|s| s.expect("WAV read error"))
            .collect(),
        SampleFormat::Int => {
            let bits = spec.bits_per_sample as u32;
            let scale = (1_u32 << (bits - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.expect("WAV read error") as f32 / scale)
                .collect()
        }
    };
    (samples, spec.sample_rate, spec.channels)
}

fn write_wav(path: &str, samples: &[f32], sample_rate: u32, channels: u16) {
    let spec = WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer = WavWriter::create(path, spec).expect("failed to create output WAV");
    for &s in samples {
        writer.write_sample(s).expect("WAV write error");
    }
    writer.finalize().expect("WAV finalise error");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() == 2 && args[1] == "--descriptors" {
        println!("{}", descriptors_json(&registry()));
        return;
    }

    if args.len() != 3 {
        eprintln!("Usage: structural-processor <input.wav> <output.wav>");
        eprintln!("       structural-processor --descriptors");
        std::process::exit(1);
    }

    let input_path = &args[1];
    let output_path = &args[2];

    let mut edits_json = String::new();
    io::stdin()
        .read_to_string(&mut edits_json)
        .expect("failed to read stdin");
    let edits: Vec<Edit> = serde_json::from_str(&edits_json).expect("invalid edits JSON");

    let (samples, sample_rate, channels) = read_wav(input_path);
    let result = apply_chain(&registry(), &samples, sample_rate, channels, &edits);
    write_wav(output_path, &result, sample_rate, channels);
}
