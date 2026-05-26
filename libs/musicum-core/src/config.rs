use std::path::{Path, PathBuf};
use serde::Deserialize;
use anyhow::{Context, Result};

#[derive(Debug, Deserialize)]
pub struct AppSettings {
    pub library: LibraryConfig,
}

#[derive(Debug, Deserialize)]
pub struct LibraryConfig {
    pub dir: String,
    pub files_dir: Option<String>,
    pub catalog_dir: Option<String>,
    pub generated_dir: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LibraryPaths {
    pub library_dir:   PathBuf,
    pub files_dir:     PathBuf,
    pub catalog_dir:   PathBuf,
    pub generated_dir: PathBuf,
}

pub fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".musicum").join("config.toml")
}

pub fn load() -> Result<AppSettings> {
    let path = config_path();
    if !path.exists() {
        write_default_config(&path)?;
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading config from {}", path.display()))?;
    toml::from_str(&text).context("parsing config.toml")
}

fn write_default_config(path: &Path) -> Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let default_dir = format!("{home}/Music/musicum");
    let template = format!(
        r#"# Musicum configuration

[library]
dir = "{default_dir}"

# Override individual subdirectories (uncomment to customize)
# files_dir = "{default_dir}/files"
# catalog_dir = "{default_dir}/catalog"
# generated_dir = "{default_dir}/.generated"
"#
    );
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, template)
        .with_context(|| format!("writing default config to {}", path.display()))
}

fn expand_tilde(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix("~/") {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        PathBuf::from(home).join(rest)
    } else {
        PathBuf::from(s)
    }
}

impl AppSettings {
    pub fn library_paths(&self) -> LibraryPaths {
        let library_dir = expand_tilde(&self.library.dir);
        let files_dir = self.library.files_dir.as_deref()
            .map(expand_tilde)
            .unwrap_or_else(|| library_dir.join("files"));
        let catalog_dir = self.library.catalog_dir.as_deref()
            .map(expand_tilde)
            .unwrap_or_else(|| library_dir.join("catalog"));
        let generated_dir = self.library.generated_dir.as_deref()
            .map(expand_tilde)
            .unwrap_or_else(|| library_dir.join(".generated"));
        LibraryPaths { library_dir, files_dir, catalog_dir, generated_dir }
    }
}

impl LibraryPaths {
    pub fn from_override(library_dir: &str) -> Self {
        let library_dir = expand_tilde(library_dir);
        let files_dir     = library_dir.join("files");
        let catalog_dir   = library_dir.join("catalog");
        let generated_dir = library_dir.join(".generated");
        LibraryPaths { library_dir, files_dir, catalog_dir, generated_dir }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn library_paths_defaults_from_dir() {
        let settings = AppSettings {
            library: LibraryConfig {
                dir: "/tmp/mylib".into(),
                files_dir: None,
                catalog_dir: None,
                generated_dir: None,
            },
        };
        let paths = settings.library_paths();
        assert_eq!(paths.files_dir,     PathBuf::from("/tmp/mylib/files"));
        assert_eq!(paths.catalog_dir,   PathBuf::from("/tmp/mylib/catalog"));
        assert_eq!(paths.generated_dir, PathBuf::from("/tmp/mylib/.generated"));
    }

    #[test]
    fn library_paths_respects_overrides() {
        let settings = AppSettings {
            library: LibraryConfig {
                dir: "/tmp/mylib".into(),
                files_dir: Some("/mnt/audio".into()),
                catalog_dir: None,
                generated_dir: Some("/mnt/gen".into()),
            },
        };
        let paths = settings.library_paths();
        assert_eq!(paths.files_dir,     PathBuf::from("/mnt/audio"));
        assert_eq!(paths.catalog_dir,   PathBuf::from("/tmp/mylib/catalog"));
        assert_eq!(paths.generated_dir, PathBuf::from("/mnt/gen"));
    }

    #[test]
    fn from_override_ignores_subdirs() {
        let paths = LibraryPaths::from_override("/tmp/override");
        assert_eq!(paths.files_dir,     PathBuf::from("/tmp/override/files"));
        assert_eq!(paths.catalog_dir,   PathBuf::from("/tmp/override/catalog"));
        assert_eq!(paths.generated_dir, PathBuf::from("/tmp/override/.generated"));
    }

    #[test]
    fn load_writes_default_config_if_missing() {
        let dir = tempdir().unwrap();
        std::env::set_var("HOME", dir.path().to_str().unwrap());
        let settings = load().unwrap();
        assert!(config_path().exists(), "default config should be written");
        assert!(settings.library.dir.contains("Music"));
    }
}
