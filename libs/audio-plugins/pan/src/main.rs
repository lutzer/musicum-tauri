use audio_plugin_sdk::AudioPlugin;

fn main() {
    println!("{}", pan::PanPlugin::descriptor().to_json());
}
