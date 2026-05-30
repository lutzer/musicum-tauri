use std::path::{PathBuf};
use std::sync::OnceLock;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub library: LibraryConfig,
    pub general: GeneralConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryConfig {
    pub files_dir: PathBuf,
    pub catalog_dir: PathBuf,
    pub generated_dir: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GeneralConfig {
    pub hidden_sidecars: bool,
}

pub fn home_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    return PathBuf::from(home);
}

pub fn default_config_path() -> PathBuf {
    home_dir().join(".musicum").join("config.toml")
}

impl Default for Config {
    fn default() -> Self {
        let home = home_dir();
        let default_dir = home.join("Music").join("musicum");
        Self {
            library: LibraryConfig {
                files_dir: default_dir.join("files"),
                catalog_dir: default_dir.join("catalog"),
                generated_dir: default_dir.join(".generated")
            },
            general: GeneralConfig {
                hidden_sidecars: true
            },
        }
    }
}

static INSTANCE: OnceLock<Config> = OnceLock::new();

pub fn init(config_path: Option<PathBuf>) {
    let resolved_path = config_path.unwrap_or_else(default_config_path);
    let config = Config::load_from(resolved_path);
    INSTANCE.set(config).expect("Config already initialized");
}

impl Config {

    pub fn get() -> &'static Config {
       INSTANCE.get_or_init(|| {
            Config::load_from(default_config_path())
        }) 
    }

    fn load_from(config_path: PathBuf) -> Config {
        if !config_path.exists() {
            // Ensure parent dirs exist
            if let Some(parent) = config_path.parent() {
                std::fs::create_dir_all(parent)
                    .unwrap_or_else(|e| panic!("Failed to create config dir '{parent:?}': {e}"));
            }
            let default_toml = toml::to_string_pretty(&Config::default())
                .expect("Failed to serialize default config");
            std::fs::write(&config_path, &default_toml)
                .unwrap_or_else(|e| panic!("Failed to write default config to '{config_path:?}': {e}"));
            println!("No config found — wrote defaults to '{config_path:?}'");
        }

        let contents = std::fs::read_to_string(&config_path)
            .unwrap_or_else(|e| panic!("Failed to read config file '{config_path:?}': {e}"));

        toml::from_str(&contents)
            .unwrap_or_else(|e| panic!("Failed to parse config file '{config_path:?}': {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values() {
        let config = Config::default();
        let base = home_dir().join("Music").join("musicum");
        assert_eq!(config.library.files_dir, base.join("files"));
        assert_eq!(config.library.catalog_dir, base.join("catalog"));
        assert_eq!(config.library.generated_dir, base.join(".generated"));
        assert!(config.general.hidden_sidecars);
    }

    #[test]
    fn load_from_missing_path_creates_file_with_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        assert!(!path.exists());
        let config = Config::load_from(path.clone());

        assert!(path.exists(), "config file should have been written");
        let base = home_dir().join("Music").join("musicum");
        assert_eq!(config.library.files_dir, base.join("files"));
        assert_eq!(config.library.catalog_dir, base.join("catalog"));
        assert_eq!(config.library.generated_dir, base.join(".generated"));
        assert!(config.general.hidden_sidecars);
    }

    #[test]
    fn load_from_existing_path_loads_custom_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        std::fs::write(&path, r#"
[library]
files_dir = "/tmp/myfiles"
catalog_dir = "/tmp/mycatalog"
generated_dir = "/tmp/mygenerated"

[general]
hidden_sidecars = false
"#).unwrap();

        let config = Config::load_from(path);

        assert_eq!(config.library.files_dir, PathBuf::from("/tmp/myfiles"));
        assert_eq!(config.library.catalog_dir, PathBuf::from("/tmp/mycatalog"));
        assert_eq!(config.library.generated_dir, PathBuf::from("/tmp/mygenerated"));
        assert!(!config.general.hidden_sidecars);
    }
}