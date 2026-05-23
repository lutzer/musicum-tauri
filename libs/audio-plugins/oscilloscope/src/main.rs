use audio_plugin_sdk::AudioPlugin;

fn main() {
    println!("{}", oscilloscope::OscilloscopePlugin::descriptor().to_json());
}
