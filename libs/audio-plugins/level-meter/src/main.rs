use audio_plugin_sdk::AudioPlugin;

fn main() {
    println!("{}", level_meter::LevelMeter::descriptor().to_json());
}
