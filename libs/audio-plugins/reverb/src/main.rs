use audio_plugin_sdk::AudioPlugin;

fn main() {
    println!("{}", reverb::ReverbPlugin::descriptor().to_json());
}
