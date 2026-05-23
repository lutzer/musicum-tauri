use audio_plugin_sdk::AudioPlugin;

fn main() {
    println!("{}", gain::GainPlugin::descriptor().to_json());
}
