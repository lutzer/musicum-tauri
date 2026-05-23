use audio_plugin_sdk::AudioPlugin;

fn main() {
    println!("{}", normalize::NormalizePlugin::descriptor().to_json());
}
